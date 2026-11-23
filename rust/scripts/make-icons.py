#!/usr/bin/env python3
"""Regenerate the app icons from one square source image.

    python3 rust/scripts/make-icons.py [source.png]

Default source: rust/ewitool-app/assets/icons/icon-source.png (ideally
1024x1024). Writes, next to it:
  logo.png   256 px, the window icon (built into the app)
  logo.icns  macOS app icon (16-1024 px)
  logo.ico   Windows icon (16-256 px)

The artwork is placed on macOS's standard icon shape: a rounded square
824 px wide on a 1024 px transparent canvas, with a soft shadow, so it
lines up with other apps in the Dock. Needs Pillow (pip install pillow)
and macOS's iconutil for the .icns.
"""
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ICONS = Path(__file__).resolve().parent.parent / "ewitool-app" / "assets" / "icons"
CANVAS, BODY, RADIUS = 1024, 824, 185  # Apple's macOS icon grid


def rounded_icon(source: Image.Image) -> Image.Image:
    art = source.convert("RGBA").resize((BODY, BODY), Image.LANCZOS)
    mask = Image.new("L", (BODY, BODY), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, BODY - 1, BODY - 1), RADIUS, fill=255)

    icon = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    offset = (CANVAS - BODY) // 2
    # Soft drop shadow beneath the body.
    shadow = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    shadow.paste((0, 0, 0, 90), (offset, offset + 12), mask)
    icon = Image.alpha_composite(icon, shadow.filter(ImageFilter.GaussianBlur(14)))
    body = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    body.paste(art, (offset, offset), mask)
    return Image.alpha_composite(icon, body)


def main() -> None:
    source = Path(sys.argv[1]) if len(sys.argv) > 1 else ICONS / "icon-source.png"
    icon = rounded_icon(Image.open(source))

    icon.resize((256, 256), Image.LANCZOS).save(ICONS / "logo.png")
    icon.save(ICONS / "logo.ico", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)])

    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / "logo.iconset"
        iconset.mkdir()
        for n in (16, 32, 128, 256, 512):
            icon.resize((n, n), Image.LANCZOS).save(iconset / f"icon_{n}x{n}.png")
            icon.resize((n * 2, n * 2), Image.LANCZOS).save(iconset / f"icon_{n}x{n}@2x.png")
        out = Path(tmp) / "logo.icns"
        subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(out)], check=True)
        shutil.copy(out, ICONS / "logo.icns")
    print(f"Wrote logo.png, logo.ico and logo.icns in {ICONS}")


if __name__ == "__main__":
    main()
