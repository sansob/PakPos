"""Generates the PakPos app icon in every format the packages need.

Usage: python3 scripts/make_icons.py   (requires Pillow)
"""
from pathlib import Path
from PIL import Image, ImageDraw

NAVY = (30, 49, 112, 255)
NAVY_DARK = (15, 27, 63, 255)
ORANGE = (247, 148, 29, 255)
WHITE = (255, 255, 255, 255)
ROOT = Path(__file__).resolve().parent.parent / "assets"


def draw(size: int) -> Image.Image:
    s = size * 4  # supersample for smooth edges
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pad = s * 0.06
    d.rounded_rectangle([pad, pad, s - pad, s - pad], radius=s * 0.22, fill=NAVY)
    # subtle lower shade
    d.rounded_rectangle([pad, s * 0.55, s - pad, s - pad], radius=s * 0.22, fill=NAVY_DARK)
    d.rectangle([pad, s * 0.55, s - pad, s * 0.7], fill=NAVY_DARK)
    # envelope
    ex0, ey0, ex1, ey1 = s * 0.2, s * 0.3, s * 0.8, s * 0.72
    d.rounded_rectangle([ex0, ey0, ex1, ey1], radius=s * 0.05, fill=ORANGE)
    w = max(2, int(s * 0.035))
    cx = (ex0 + ex1) / 2
    d.line([(ex0 + w, ey0 + w), (cx, s * 0.53), (ex1 - w, ey0 + w)], fill=NAVY_DARK, width=w, joint="curve")
    # "send" arrow dot
    r = s * 0.075
    d.ellipse([ex1 - r * 1.2, ey0 - r * 1.2, ex1 + r * 0.8, ey0 + r * 0.8], fill=WHITE)
    return img.resize((size, size), Image.LANCZOS)


def main() -> None:
    ROOT.mkdir(exist_ok=True)
    big = draw(1024)
    big.save(ROOT / "icon-1024.png")
    for n in (16, 32, 48, 64, 128, 256, 512):
        draw(n).save(ROOT / f"icon-{n}.png")
    big.save(ROOT / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    big.save(ROOT / "icon.icns")
    print("icons written to", ROOT)


if __name__ == "__main__":
    main()
