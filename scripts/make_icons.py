"""Generates the PakPos app icon in every format the packages need.

The source is assets/icon.svg (the Pak Pos face). It is rendered once at
1024 px with headless Chromium, then scaled down with Pillow:

    python3 scripts/make_icons.py            # needs Pillow + Chromium/Chrome
    CHROME=/path/to/chrome python3 scripts/make_icons.py

Without a browser it reuses assets/icon-1024.png as the master.
"""
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent / "assets"
SVG = ROOT / "icon.svg"
MASTER = ROOT / "icon-1024.png"


def find_chrome() -> str | None:
    candidates = [os.environ.get("CHROME"), "/opt/pw-browsers/chromium-1194/chrome-linux/chrome"]
    candidates += [shutil.which(n) for n in ("chromium", "chromium-browser", "google-chrome", "chrome")]
    return next((c for c in candidates if c and Path(c).exists()), None)


def render_svg() -> Image.Image | None:
    chrome = find_chrome()
    if not chrome:
        return None
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "shot.png"
        # Taller window: headless screenshots lose a strip at the bottom.
        subprocess.run(
            [chrome, "--headless", "--no-sandbox", "--disable-gpu", "--hide-scrollbars",
             "--default-background-color=00000000", "--window-size=1024,1224",
             f"--screenshot={out}", SVG.as_uri()],
            check=True, capture_output=True,
        )
        return Image.open(out).convert("RGBA").crop((0, 0, 1024, 1024))


def main() -> None:
    big = render_svg()
    if big is None:
        print("no Chromium found: using", MASTER)
        big = Image.open(MASTER).convert("RGBA")
    else:
        big.save(MASTER)
    for n in (16, 32, 48, 64, 128, 256, 512):
        big.resize((n, n), Image.LANCZOS).save(ROOT / f"icon-{n}.png")
    big.save(ROOT / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    big.save(ROOT / "icon.icns")
    print("icons written to", ROOT)


if __name__ == "__main__":
    main()
