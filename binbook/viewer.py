from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageDraw

from .pixels import gray2_to_luma, unpack_gray2
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
    pixels = unpack_gray2(packed, page.stored_width, page.stored_height)
    image = Image.new("RGB", (page.stored_width, page.stored_height))
    image.putdata([(gray2_to_luma(value),) * 3 for value in pixels])
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


def launch_viewer(
    path: Path | str,
    *,
    page: int = 0,
    show_chrome: bool = True,
    debug_content_box: bool = False,
) -> None:
    try:
        import tkinter as tk
        from PIL import ImageTk
    except ImportError as exc:
        raise RuntimeError("Tkinter is required for binbook view") from exc

    reader = BinBookReader.open(path)
    state = ViewerState(len(reader.pages), page)

    root = tk.Tk()
    root.title(f"BinBook Viewer - {Path(path).name}")
    root.resizable(False, False)

    image_label = tk.Label(root)
    image_label.pack()
    status = tk.Label(root, anchor="center")
    status.pack(fill="x")
    photo_ref: dict[str, ImageTk.PhotoImage] = {}

    def refresh() -> None:
        image = render_page_image(
            reader,
            state.current_page,
            show_chrome=show_chrome,
            debug_content_box=debug_content_box,
        )
        photo = ImageTk.PhotoImage(image)
        photo_ref["image"] = photo
        image_label.configure(image=photo)
        status.configure(text=f"Page {state.current_page + 1} of {state.page_count}")

    def on_key(event: tk.Event) -> None:
        if event.keysym in {"Right", "Down", "space", "Page_Down"}:
            state.next_page()
            refresh()
        elif event.keysym in {"Left", "Up", "BackSpace", "Page_Up"}:
            state.previous_page()
            refresh()
        elif event.keysym in {"Home"}:
            state.jump_to_page(0)
            refresh()
        elif event.keysym in {"End"}:
            state.jump_to_page(state.page_count - 1)
            refresh()
        elif event.keysym in {"Escape", "q"}:
            root.destroy()

    root.bind("<Key>", on_key)
    refresh()
    root.mainloop()
