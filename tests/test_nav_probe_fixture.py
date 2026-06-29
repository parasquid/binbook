from pathlib import Path

from PIL import Image

from binbook.constants import PixelFormat, SectionId, WaveformHint
from binbook.images import storage_image_to_logical
from binbook.reader import BinBookReader
from binbook.rle import decode_packbits


FIXTURE = Path("firmware/crates/binbook-fw/fixtures/nav_probe.binbook")


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

    assert [image.getpixel((x, y)) for x in (80, 240, 400) for y in (80, 240)] == [
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
