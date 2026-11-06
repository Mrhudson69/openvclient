#!/usr/bin/env python3
"""Generate OpenVClient's icon set and installer artwork.

Outputs (committed, so builds do not need Pillow):
  assets/icon/openvclient.png        1024x1024 master
  assets/icon/openvclient.icns       macOS (via iconutil, macOS only)
  assets/icon/openvclient.ico        Windows (16-256 px)
  assets/icon/openvclient-256.png    Linux desktop icon
  assets/installer/dmg-background.png      660x420 (+ @2x) DMG window
  assets/installer/wizard-large-<w>x<h>.png  Inno Setup side image, one per DPI step
  assets/installer/wizard-small-<n>.png      Inno Setup header icon, one per DPI step

Colours follow design-system/openvclient/MASTER.md (slate + green accent).
Run: python3 scripts/make-icons.py
     python3 scripts/make-icons.py --windows-installer   (wizard images only)
"""

import os
import shutil
import subprocess
import sys
import tempfile

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICON_DIR = os.path.join(ROOT, "assets", "icon")
INST_DIR = os.path.join(ROOT, "assets", "installer")

SLATE_950 = (2, 6, 23)
SLATE_900 = (15, 23, 42)
SLATE_800 = (30, 41, 59)
SLATE_700 = (51, 65, 85)
SLATE_400 = (148, 163, 184)
SLATE_100 = (241, 245, 249)
GREEN_500 = (34, 197, 94)
GREEN_400 = (74, 222, 128)
GREEN_700 = (21, 128, 61)


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def vertical_gradient(size, top, bottom):
    w, h = size
    img = Image.new("RGB", size)
    px = img.load()
    for y in range(h):
        c = lerp(top, bottom, y / max(1, h - 1))
        for x in range(w):
            px[x, y] = c
    return img


def icon_master(S=1024):
    """Squircle tile with a stacked-database mark and a connection node."""
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    # macOS icon grid: ~824 px body inside 1024, soft drop shadow.
    inset = int(S * 0.098)
    body = (inset, inset, S - inset, S - inset)
    radius = int(S * 0.185)
    shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle((body[0], body[1] + S * 0.012, body[2], body[3] + S * 0.012), radius, fill=(0, 0, 0, 150))
    img.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(S * 0.02)))

    tile = vertical_gradient((body[2] - body[0], body[3] - body[1]), SLATE_800, SLATE_950).convert("RGBA")
    mask = Image.new("L", tile.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, tile.size[0] - 1, tile.size[1] - 1), radius, fill=255)
    img.paste(tile, body[:2], mask)
    d = ImageDraw.Draw(img)
    # Hairline highlight on the tile edge.
    d.rounded_rectangle(body, radius, outline=(255, 255, 255, 28), width=max(2, S // 256))

    # Database: three stacked discs.
    cx = S * 0.5
    w = S * 0.40
    disc_h = S * 0.085
    gap = S * 0.115
    top = S * 0.30
    for i in range(3):
        y = top + i * gap
        shade = lerp(GREEN_400, GREEN_700, i / 2.5)
        # Side of the disc.
        d.rectangle((cx - w / 2, y + disc_h / 2, cx + w / 2, y + disc_h / 2 + gap * 0.62), fill=shade + (255,))
        d.ellipse((cx - w / 2, y + gap * 0.62, cx + w / 2, y + gap * 0.62 + disc_h), fill=shade + (255,))
        # Top face.
        d.ellipse((cx - w / 2, y, cx + w / 2, y + disc_h), fill=lerp(GREEN_400, SLATE_100, 0.25) + (255,))
        d.ellipse((cx - w / 2 * 0.86, y + disc_h * 0.12, cx + w / 2 * 0.86, y + disc_h * 0.88), fill=GREEN_500 + (255,))

    # Connection: a line to a node, bottom-right, suggesting "client".
    lw = int(S * 0.026)
    base_y = top + 2 * gap + gap * 0.62 + disc_h
    nx, ny = S * 0.73, S * 0.76
    d.line([(cx + w * 0.30, base_y - disc_h * 0.2), (cx + w * 0.30, ny), (nx, ny)], fill=SLATE_400 + (255,), width=lw, joint="curve")
    r = S * 0.052
    d.ellipse((nx - r, ny - r, nx + r, ny + r), fill=SLATE_100 + (255,))
    d.ellipse((nx - r * 0.48, ny - r * 0.48, nx + r * 0.48, ny + r * 0.48), fill=GREEN_500 + (255,))
    return img


def font(size, bold=False):
    here = os.path.join(ROOT, "crates", "openvclient-app", "assets", "fonts", "IBMPlexSans.ttf")
    try:
        f = ImageFont.truetype(here, size)
        if bold:
            try:
                f.set_variation_by_axes([700])
            except Exception:
                pass
        return f
    except Exception:
        return ImageFont.load_default()


def dmg_background(scale=1):
    W, H = 660 * scale, 420 * scale
    img = vertical_gradient((W, H), SLATE_900, SLATE_950).convert("RGBA")
    d = ImageDraw.Draw(img)
    d.text((W / 2, 52 * scale), "OpenVClient", fill=SLATE_100, font=font(28 * scale, True), anchor="mm")
    d.text((W / 2, 86 * scale), "Drag the app onto Applications to install", fill=SLATE_400, font=font(15 * scale), anchor="mm")
    # Arrow between the two icon slots (app at x=170, Applications at x=490, y=230).
    y = 230 * scale
    x0, x1 = 262 * scale, 398 * scale
    d.line([(x0, y), (x1 - 16 * scale, y)], fill=GREEN_500, width=6 * scale)
    d.polygon([(x1, y), (x1 - 24 * scale, y - 14 * scale), (x1 - 24 * scale, y + 14 * scale)], fill=GREEN_500)
    d.text((W / 2, H - 30 * scale), "Open-source · github.com/Mrhudson69/openvclient", fill=SLATE_700, font=font(12 * scale), anchor="mm")
    return img


# Inno Setup 6.6+ image areas at 100/125/150/200/250 % DPI (default font and
# WizardSizePercent). Setup picks the closest file, so text stays sharp.
WIZARD_LARGE_SIZES = [(202, 386), (269, 515), (336, 643), (430, 824), (534, 1022)]
WIZARD_SMALL_SIZES = [58, 77, 97, 124, 159]


def wizard_large(W=202, H=386):
    """Welcome/Finish side panel: dark slate in both light and dark wizards."""
    s = W / 202
    img = vertical_gradient((W, H), SLATE_800, SLATE_950).convert("RGBA")
    ic_size = int(132 * s)
    ic = icon_master(1024).resize((ic_size, ic_size), Image.LANCZOS)
    img.alpha_composite(ic, ((W - ic_size) // 2, int(92 * s)))
    d = ImageDraw.Draw(img)
    d.text((W / 2, 258 * s), "OpenVClient", fill=SLATE_100, font=font(int(21 * s), True), anchor="mm")
    d.text((W / 2, 284 * s), "Databases & infrastructure", fill=SLATE_400, font=font(int(12 * s)), anchor="mm")
    return img.convert("RGB")


def wizard_small(S=58):
    """Header icon. Transparent, so it sits on the light or dark top panel."""
    return icon_master(1024).resize((S, S), Image.LANCZOS)


def windows_wizard_images():
    for w, h in WIZARD_LARGE_SIZES:
        wizard_large(w, h).save(os.path.join(INST_DIR, f"wizard-large-{w}x{h}.png"), optimize=True)
    for s in WIZARD_SMALL_SIZES:
        wizard_small(s).save(os.path.join(INST_DIR, f"wizard-small-{s}.png"), optimize=True)


def main():
    os.makedirs(ICON_DIR, exist_ok=True)
    os.makedirs(INST_DIR, exist_ok=True)
    if "--windows-installer" in sys.argv:
        windows_wizard_images()
        print("Inno Setup wizard images written to assets/installer/")
        return
    master = icon_master(1024)
    master.save(os.path.join(ICON_DIR, "openvclient.png"))
    master.resize((256, 256), Image.LANCZOS).save(os.path.join(ICON_DIR, "openvclient-256.png"))
    master.save(os.path.join(ICON_DIR, "openvclient.ico"), sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])

    if sys.platform == "darwin" and shutil.which("iconutil"):
        with tempfile.TemporaryDirectory() as tmp:
            iconset = os.path.join(tmp, "openvclient.iconset")
            os.makedirs(iconset)
            for size in (16, 32, 128, 256, 512):
                master.resize((size, size), Image.LANCZOS).save(os.path.join(iconset, f"icon_{size}x{size}.png"))
                master.resize((size * 2, size * 2), Image.LANCZOS).save(os.path.join(iconset, f"icon_{size}x{size}@2x.png"))
            subprocess.run(["iconutil", "-c", "icns", iconset, "-o", os.path.join(ICON_DIR, "openvclient.icns")], check=True)
    else:
        print("iconutil not available: keeping the committed openvclient.icns")

    dmg_background(1).save(os.path.join(INST_DIR, "dmg-background.png"))
    dmg_background(2).save(os.path.join(INST_DIR, "dmg-background@2x.png"))
    windows_wizard_images()
    print("icons and installer artwork written to assets/")


if __name__ == "__main__":
    main()
