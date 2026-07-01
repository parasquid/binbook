import hashlib
from pathlib import Path
import subprocess

from PIL import Image

from binbook.constants import PixelFormat, SectionId, WaveformHint
from binbook.images import storage_image_to_logical
from binbook.reader import BinBookReader
from binbook.rle import decode_packbits


FIXTURE = Path("firmware/crates/binbook-fw/fixtures/nav_probe.binbook")
FIXTURE_COPIES = (
    FIXTURE,
    Path("crates/binbook-core/tests/fixtures/nav_probe.binbook"),
    Path("crates/xteink-x4-display/tests/fixtures/nav_probe.binbook"),
)
PAGE_LABEL_BOX = (70, 170, 410, 360)


def _decode_x4_native_page(reader: BinBookReader, page_index: int) -> Image.Image:
    page = reader.pages[page_index]
    assert page.pixel_format == PixelFormat.GRAY2_PACKED
    assert (page.stored_width, page.stored_height) == (800, 480)

    planes = []
    for slot in (0, 1, 2):
        start = reader.header.page_data_offset + page.plane_dir.offsets[slot]
        end = start + page.plane_dir.sizes[slot]
        plane = decode_packbits(reader.data[start:end])
        assert len(plane) == 100 * 480
        planes.append(plane)

    luma = bytearray(800 * 480)
    for y in range(480):
        row = y * 100
        for x in range(800):
            ram_x = 799 - x
            mask = 0x80 >> (ram_x % 8)
            msb = 1 if planes[0][row + ram_x // 8] & mask else 0
            lsb = 1 if planes[1][row + ram_x // 8] & mask else 0
            base = 1 if planes[2][row + ram_x // 8] & mask else 0
            if base:
                gray = 3
            elif not msb:
                gray = 0
            elif lsb:
                gray = 1
            else:
                gray = 2
            luma[y * 800 + x] = (0, 85, 170, 255)[gray]

    storage = Image.frombytes("L", (800, 480), bytes(luma))
    return storage_image_to_logical(
        storage,
        logical_width=480,
        logical_height=800,
        logical_to_physical_rotation=270,
    )


def test_nav_probe_has_sixteen_numbered_pages():
    reader = BinBookReader.open(FIXTURE, validate=True)

    assert len(reader.pages) == 16
    assert len(reader.page_chunks) == 16 * 3 * 30
    assert len(reader.page_transitions) == 2 * (16 - 1)
    assert [page.page_number for page in reader.pages] == list(range(16))
    assert SectionId.FONT_RESOURCE_INDEX in reader.sections


def test_nav_probe_fixture_copies_are_byte_identical():
    payloads = [path.read_bytes() for path in FIXTURE_COPIES]
    assert payloads[1:] == payloads[:-1]


def test_nav_probe_builder_requires_an_explicit_rust_compiler():
    result = subprocess.run(
        ["python", "firmware/scripts/build-nav-probe-fixture.py", "--help"],
        text=True,
        capture_output=True,
    )
    assert result.returncode == 0, result.stderr
    assert "--compiler" in result.stdout


def test_every_nav_probe_page_keeps_orientation_and_gray_frame():
    reader = BinBookReader.open(FIXTURE)

    for page_index in range(16):
        image = _decode_x4_native_page(reader, page_index)
        assert max(image.crop((0, 0, 480, 10)).get_flattened_data()) == 0
        assert max(image.crop((0, 790, 480, 800)).get_flattened_data()) == 0
        assert max(image.crop((0, 0, 10, 800)).get_flattened_data()) == 0
        assert max(image.crop((470, 0, 480, 800)).get_flattened_data()) == 0
        assert image.getpixel((240, 400)) == 0
        assert [image.getpixel((x, 535)) for x in (140, 205, 270, 335)] == [
            0,
            85,
            170,
            255,
        ]

        for box in (
            (10, 10, 240, 400),
            (240, 10, 470, 400),
            (10, 400, 240, 790),
            (240, 400, 470, 790),
        ):
            assert min(image.crop(box).get_flattened_data()) < 255


def test_nav_probe_pages_have_unique_labels_and_images():
    reader = BinBookReader.open(FIXTURE)
    images = [_decode_x4_native_page(reader, index) for index in range(16)]

    label_bytes = [image.crop(PAGE_LABEL_BOX).tobytes() for image in images]
    assert len(set(label_bytes)) == 16
    assert len({hashlib.sha256(image.tobytes()).digest() for image in images}) == 16


def test_nav_probe_page_0_is_a_full_panel_orientation_target():
    reader = BinBookReader.open(FIXTURE)
    display = reader._section_data(SectionId.DISPLAY_PROFILE)
    assert display[53] == WaveformHint.SSD1677_STAGED_GRAY2
    image = _decode_x4_native_page(reader, 0)

    assert image.size == (480, 800)
    assert max(image.crop((0, 0, 480, 10)).get_flattened_data()) == 0
    assert max(image.crop((0, 790, 480, 800)).get_flattened_data()) == 0
    assert max(image.crop((0, 0, 10, 800)).get_flattened_data()) == 0
    assert max(image.crop((470, 0, 480, 800)).get_flattened_data()) == 0
    assert image.getpixel((240, 400)) == 0
    assert [image.getpixel((x, 535)) for x in (140, 205, 270, 335)] == [
        0,
        85,
        170,
        255,
    ]

    quadrants = (
        (10, 10, 240, 400),
        (240, 10, 470, 400),
        (10, 400, 240, 790),
        (240, 400, 470, 790),
    )
    for box in quadrants:
        assert min(image.crop(box).get_flattened_data()) < 255

    right_half = image.crop((240, 10, 470, 790))
    assert sum(pixel < 255 for pixel in right_half.get_flattened_data()) > 10_000


def test_nav_probe_page_1_is_a_true_black_white_checkerboard():
    reader = BinBookReader.open(FIXTURE)
    image = _decode_x4_native_page(reader, 1)

    assert [image.getpixel((x, y)) for x in (80, 180, 360) for y in (80, 620)] == [
        255,
        0,
        0,
        255,
        255,
        0,
    ]


def test_nav_probe_pages_keep_their_assigned_dominant_patterns():
    reader = BinBookReader.open(FIXTURE)
    images = [_decode_x4_native_page(reader, index) for index in range(16)]

    assert [images[2].getpixel((x, 430)) for x in (60, 180, 300, 420)] == [
        0,
        85,
        170,
        255,
    ]
    assert [images[4].getpixel((60, y)) for y in (80, 280, 480, 680)] == [
        0,
        85,
        170,
        255,
    ]
    assert images[5].getpixel((80, 732)) == 0
    assert images[6].getpixel((80, 68)) == 0
    assert images[7].getpixel((80, 732)) == 0
    assert images[8].getpixel((75, 75)) == 0
    assert [images[9].getpixel((x, 430)) for x in (60, 80)] == [0, 255]
    assert [images[10].getpixel((60, y)) for y in (420, 440)] == [0, 255]
    assert [images[11].getpixel((x, y)) for x, y in ((80, 140), (420, 140), (80, 680), (420, 680))] == [
        0,
        85,
        170,
        255,
    ]
    sparse_black = sum(pixel == 0 for pixel in images[12].crop((20, 370, 460, 760)).get_flattened_data())
    dense_black = sum(pixel == 0 for pixel in images[13].crop((20, 370, 460, 760)).get_flattened_data())
    assert 0 < sparse_black < dense_black
    assert images[14].getpixel((60, 60)) == 0
    assert [images[15].getpixel((x, y)) for x in (80, 180, 360) for y in (80, 620)] == [
        0,
        255,
        255,
        0,
        0,
        255,
    ]


def test_nav_probe_page_3_uses_a_larger_font():
    reader = BinBookReader.open(FIXTURE)
    output = Path("/tmp/nav_probe_page3.png")
    reader.decode_page_to_png(3, output)

    image = Image.open(output).convert("L")
    threshold = 220
    dark_rows = [
        any(image.getpixel((x, y)) < threshold for x in range(image.width))
        for y in range(image.height)
    ]

    band_count = 0
    in_band = False
    for dark in dark_rows:
        if dark and not in_band:
            band_count += 1
            in_band = True
        elif not dark:
            in_band = False

    assert band_count <= 10


def test_nav_probe_transition_masks_compare_decompressed_fast_base_chunks():
    reader = BinBookReader.open(FIXTURE)
    base_chunks = {}
    for chunk in reader.page_chunks:
        if chunk.plane_slot != 2:
            continue
        start = reader.header.page_data_offset + chunk.page_data_offset
        compressed = reader.data[start : start + chunk.compressed_size]
        decoded = decode_packbits(compressed)
        assert len(decoded) == chunk.uncompressed_size
        base_chunks[(chunk.page_number, chunk.chunk_index)] = decoded

    for transition in reader.page_transitions:
        expected_mask = 0
        changed = []
        for chunk_index in range(30):
            if base_chunks[(transition.from_page_number, chunk_index)] != base_chunks[
                (transition.to_page_number, chunk_index)
            ]:
                expected_mask |= 1 << chunk_index
                changed.append(chunk_index)
        assert transition.changed_chunk_mask == expected_mask
        assert transition.first_changed_chunk == changed[0]
        assert transition.changed_chunk_count == changed[-1] - changed[0] + 1
