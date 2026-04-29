from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageDraw

from .constants import PixelFormat
from .pixels import gray1_to_luma, gray2_to_luma, unpack_gray1, unpack_gray2
from .reader import BinBookReader


@dataclass
class ViewerState:
    page_count: int
    current_page: int = 0

    def __post_init__(self) -> None:
        if self.page_count < 1:
            raise ValueError("viewer requires at least one page")
        self.current_page = self._clamp(self.current_page)

    def next_page(self) -> int:
        self.current_page = self._clamp(self.current_page + 1)
        return self.current_page

    def previous_page(self) -> int:
        self.current_page = self._clamp(self.current_page - 1)
        return self.current_page

    def jump_to_page(self, page_number: int) -> int:
        self.current_page = self._clamp(page_number)
        return self.current_page

    def _clamp(self, page_number: int) -> int:
        return max(0, min(self.page_count - 1, page_number))


def render_page_image(
    reader: BinBookReader,
    page_number: int,
    *,
    show_chrome: bool = True,
    debug_content_box: bool = False,
) -> Image.Image:
    packed, page = reader.decode_page_bytes(page_number)
    if page.pixel_format == PixelFormat.GRAY1_PACKED:
        pixels = unpack_gray1(packed, page.stored_width, page.stored_height)
        lumas = [gray1_to_luma(value) for value in pixels]
    elif page.pixel_format == PixelFormat.GRAY2_PACKED:
        pixels = unpack_gray2(packed, page.stored_width, page.stored_height)
        lumas = [gray2_to_luma(value) for value in pixels]
    else:
        raise ValueError(f"unsupported page pixel format: {page.pixel_format}")
    image = Image.new("RGB", (page.stored_width, page.stored_height))
    image.putdata([(value,) * 3 for value in lumas])
    draw = ImageDraw.Draw(image)
    if debug_content_box:
        draw.rectangle((page.placement_x, page.placement_y, page.stored_width - 1, page.stored_height - 1), outline=(255, 0, 0), width=2)
    if show_chrome:
        label = f"{page_number + 1} / {len(reader.pages)}"
        bbox = draw.textbbox((0, 0), label)
        x = max(4, (page.stored_width - (bbox[2] - bbox[0])) // 2)
        y = page.stored_height - (bbox[3] - bbox[1]) - 10
        draw.rectangle((x - 6, y - 4, x + (bbox[2] - bbox[0]) + 6, y + (bbox[3] - bbox[1]) + 4), fill=(255, 255, 255))
        draw.text((x, y), label, fill=(0, 0, 0))
    return image


def image_to_surface(image: Image.Image):
    import pygame

    rgb = image.convert("RGB")
    return pygame.image.frombuffer(rgb.tobytes(), rgb.size, "RGB").copy()


def launch_viewer(
    path: Path | str,
    *,
    page: int = 0,
    show_chrome: bool = True,
    debug_content_box: bool = False,
) -> None:
    try:
        import pygame
    except ImportError as exc:
        raise RuntimeError("Pygame is required for binbook view") from exc

    reader = BinBookReader.open(path)
    state = ViewerState(len(reader.pages), page)

    pygame.init()
    pygame.display.set_caption(f"BinBook Viewer - {Path(path).name}")
    screen = pygame.display.set_mode((reader.pages[0].stored_width, reader.pages[0].stored_height))
    clock = pygame.time.Clock()

    def refresh() -> None:
        image = render_page_image(
            reader,
            state.current_page,
            show_chrome=show_chrome,
            debug_content_box=debug_content_box,
        )
        screen.blit(image_to_surface(image), (0, 0))
        pygame.display.flip()

    refresh()
    running = True
    while running:
        for event in pygame.event.get():
            if event.type == pygame.QUIT:
                running = False
            elif event.type == pygame.KEYDOWN:
                if event.key in {pygame.K_RIGHT, pygame.K_DOWN, pygame.K_SPACE, pygame.K_PAGEDOWN}:
                    state.next_page()
                    refresh()
                elif event.key in {pygame.K_LEFT, pygame.K_UP, pygame.K_BACKSPACE, pygame.K_PAGEUP}:
                    state.previous_page()
                    refresh()
                elif event.key == pygame.K_HOME:
                    state.jump_to_page(0)
                    refresh()
                elif event.key == pygame.K_END:
                    state.jump_to_page(state.page_count - 1)
                    refresh()
                elif event.key in {pygame.K_ESCAPE, pygame.K_q}:
                    running = False
        clock.tick(30)
    pygame.quit()
