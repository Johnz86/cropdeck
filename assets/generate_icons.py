"""Render the CropDeck icon from one geometry definition into SVG, PNG and ICO assets."""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw

CANVAS = 1024
SUPERSAMPLE = 4
CORNER_RADIUS = 224
BACKGROUND = "#1C2331"
STRIP = "#55637F"
ACCENT = "#F5B942"
STRIP_BOX = (392, 96, 632, 928)
BRACKET_STROKE = 96
BRACKET_ARM = 408
CROP_BOX = (232, 232, 792, 792)
PNG_SIZES = (16, 32, 48, 64, 128, 256, 512)
ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)

ASSETS = Path(__file__).resolve().parent
HICOLOR = ASSETS / "linux" / "icons" / "hicolor"


def bracket_rectangles() -> list[tuple[int, int, int, int]]:
    left, top, right, bottom = CROP_BOX
    stroke = BRACKET_STROKE
    return [
        (left, top, left + BRACKET_ARM, top + stroke),
        (left, top, left + stroke, top + BRACKET_ARM),
        (right - BRACKET_ARM, bottom - stroke, right, bottom),
        (right - stroke, bottom - BRACKET_ARM, right, bottom),
    ]


def render_master() -> Image.Image:
    scale = SUPERSAMPLE
    size = CANVAS * scale
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle((0, 0, size - 1, size - 1), CORNER_RADIUS * scale, fill=BACKGROUND)
    draw.rectangle(tuple(v * scale for v in STRIP_BOX), fill=STRIP)
    for box in bracket_rectangles():
        draw.rectangle(tuple(v * scale for v in box), fill=ACCENT)
    return image


def write_svg(path: Path) -> None:
    strip = STRIP_BOX
    rects = "\n".join(
        f'  <rect x="{l}" y="{t}" width="{r - l}" height="{b - t}" fill="{ACCENT}"/>'
        for l, t, r, b in bracket_rectangles()
    )
    path.write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {CANVAS} {CANVAS}">\n'
        f'  <rect width="{CANVAS}" height="{CANVAS}" rx="{CORNER_RADIUS}" fill="{BACKGROUND}"/>\n'
        f'  <rect x="{strip[0]}" y="{strip[1]}" width="{strip[2] - strip[0]}" '
        f'height="{strip[3] - strip[1]}" fill="{STRIP}"/>\n'
        f"{rects}\n"
        "</svg>\n",
        encoding="utf-8",
    )


def downscale(master: Image.Image, size: int) -> Image.Image:
    return master.resize((size, size), Image.Resampling.LANCZOS)


def write_hicolor_pngs(master: Image.Image) -> None:
    for size in PNG_SIZES:
        target = HICOLOR / f"{size}x{size}" / "apps" / "cropdeck.png"
        target.parent.mkdir(parents=True, exist_ok=True)
        downscale(master, size).save(target, optimize=True)


def write_ico(master: Image.Image, path: Path) -> None:
    frames = [downscale(master, size) for size in ICO_SIZES]
    frames[-1].save(
        path,
        format="ICO",
        sizes=[(size, size) for size in ICO_SIZES],
        append_images=frames[:-1],
    )


def main() -> None:
    master = render_master()
    scalable = HICOLOR / "scalable" / "apps"
    scalable.mkdir(parents=True, exist_ok=True)
    write_svg(scalable / "cropdeck.svg")
    write_hicolor_pngs(master)
    write_ico(master, ASSETS / "windows" / "cropdeck.ico")


if __name__ == "__main__":
    main()
