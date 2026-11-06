#!/usr/bin/env python3
"""Generate the OpenVClient logo set (SVG) from one definition.

Mark: "Vee", a friendly database cylinder whose front is a bold V (for
OpenVClient) that doubles as its smile, on a yellow sticker tile with a thick
outline and a hard offset shadow, matching the website's cartoon infra style.

Variants
  light: ink outline + ink shadow, for light backgrounds
  dark:  cream outline + coral shadow, for dark backgrounds
Wordmark: "OpenVClient" in IBM Plex Sans Bold (the app's UI font), converted
to outlines so it renders identically everywhere; the V is brand green.

Outputs (assets/brand/):
  openvclient-mark-{light,dark}.svg
  openvclient-logo-{light,dark}.svg          horizontal lockup
  openvclient-logo-stacked-{light,dark}.svg  stacked lockup
  openvclient-wordmark-{light,dark}.svg
  favicon.svg                                switches with prefers-color-scheme

Needs fontTools: python3 -m pip install fonttools
Run: python3 scripts/make-logo.py
"""

import os
from io import StringIO

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "assets", "brand")
FONT = os.path.join(ROOT, "crates", "openvclient-app", "assets", "fonts", "IBMPlexSans.ttf")

# Brand palette (matches the website tokens and the app's accent green).
INK = "#1A1612"
CREAM = "#FFF7E6"
YELLOW = "#FFDB58"
GREEN = "#22C55E"
GREEN_LIGHT = "#86EFAC"
TEAL = "#4ECDC4"
CORAL = "#FF6B6B"

THEMES = {
    "light": {"line": INK, "shadow": INK, "text": INK},
    "dark": {"line": CREAM, "shadow": CORAL, "text": CREAM},
}


def mark_group(theme, x=0, y=0, s=1.0):
    """The mark drawn in a 512x512 box, placed at (x, y) and scaled by s."""
    t = THEMES[theme]
    line, shadow = t["line"], t["shadow"]
    # The pupils and V outline stay ink in both themes: they sit on the green body.
    return f'''<g transform="translate({x} {y}) scale({s})">
  <rect x="58" y="58" width="420" height="420" rx="112" fill="{shadow}"/>
  <rect x="34" y="34" width="420" height="420" rx="112" fill="{YELLOW}" stroke="{line}" stroke-width="20"/>
  <path d="M112 156 V352 A132 46 0 0 0 376 352 V156 Z" fill="{GREEN}" stroke="{INK}" stroke-width="18" stroke-linejoin="round"/>
  <ellipse cx="244" cy="156" rx="132" ry="46" fill="{GREEN_LIGHT}" stroke="{INK}" stroke-width="18"/>
  <path d="M170 272 H214 L244 340 L274 272 H318 L268 384 H220 Z" fill="{CREAM}" stroke="{INK}" stroke-width="16" stroke-linejoin="round"/>
  <circle cx="202" cy="236" r="19" fill="#FFFFFF" stroke="{INK}" stroke-width="10"/>
  <circle cx="286" cy="236" r="19" fill="#FFFFFF" stroke="{INK}" stroke-width="10"/>
  <circle cx="206" cy="239" r="8" fill="{INK}"/>
  <circle cx="290" cy="239" r="8" fill="{INK}"/>
  <path d="M398 74 L410 104 L440 116 L410 128 L398 158 L386 128 L356 116 L386 104 Z" fill="{TEAL}" stroke="{line}" stroke-width="10" stroke-linejoin="round"/>
</g>'''


class Wordmark:
    """Outlined "OpenVClient" text from the bold instance of IBM Plex Sans."""

    def __init__(self):
        f = TTFont(FONT)
        self.font = instantiateVariableFont(f, {"wght": 700, "wdth": 100}) if "fvar" in f else f
        self.glyphs = self.font.getGlyphSet()
        self.cmap = self.font.getBestCmap()
        self.upm = self.font["head"].unitsPerEm
        self.hmtx = self.font["hmtx"]
        os2 = self.font["OS/2"]
        self.cap = getattr(os2, "sCapHeight", 0) or 700

    def paths(self, text, size, tracking=-0.01):
        """Yield (char, svg path d) at `size` px, baseline y=0, starting x=0."""
        scale = size / self.upm
        x = 0.0
        out = []
        for ch in text:
            name = self.cmap[ord(ch)]
            buf = SVGPathPen(self.glyphs)
            # Font units are y-up; SVG is y-down.
            self.glyphs[name].draw(TransformPen(buf, (scale, 0, 0, -scale, x, 0)))
            out.append((ch, buf.getCommands()))
            x += self.hmtx[name][0] * scale + tracking * size
        return out, x - tracking * size

    def svg(self, theme, size, x=0, y=0):
        """Group with the wordmark; y is the baseline. Returns (svg, width)."""
        t = THEMES[theme]
        glyphs, width = self.paths("OpenVClient", size)
        parts = []
        for ch, d in glyphs:
            if not d:
                continue
            if ch == "V":
                # The brand V: green with an outline, like the mark's face.
                parts.append(f'<path d="{d}" fill="{GREEN}" stroke="{t["line"]}" stroke-width="{size * 0.045:.2f}" stroke-linejoin="round" paint-order="stroke"/>')
            else:
                parts.append(f'<path d="{d}" fill="{t["text"]}"/>')
        return f'<g transform="translate({x} {y})">{"".join(parts)}</g>', width


def doc(w, h, body, title):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w:.0f} {h:.0f}" width="{w:.0f}" height="{h:.0f}" role="img" aria-label="{title}"><title>{title}</title>{body}</svg>\n'


def main():
    os.makedirs(OUT, exist_ok=True)
    wm = Wordmark()
    for theme in THEMES:
        # Mark alone.
        with open(os.path.join(OUT, f"openvclient-mark-{theme}.svg"), "w") as f:
            f.write(doc(512, 512, mark_group(theme), "OpenVClient"))

        # Horizontal lockup: mark 160 px, wordmark cap height ~ 0.44 of mark.
        size = 132
        word, ww = wm.svg(theme, size)
        mark_px, gap, pad = 160, 30, 8
        cap_px = wm.cap / wm.upm * size
        baseline = pad + mark_px / 2 + cap_px / 2
        w = pad + mark_px + gap + ww + pad
        h = pad * 2 + mark_px
        body = mark_group(theme, pad, pad, mark_px / 512) + word.replace('<g transform="translate(0 0)">', f'<g transform="translate({pad + mark_px + gap:.1f} {baseline:.1f})">', 1)
        with open(os.path.join(OUT, f"openvclient-logo-{theme}.svg"), "w") as f:
            f.write(doc(w, h, body, "OpenVClient"))

        # Stacked lockup: mark above, wordmark centred under it.
        size_s = 96
        word_s, ws = wm.svg(theme, size_s)
        mark_s = 256
        w2 = max(ws, mark_s) + pad * 2
        cap_s = wm.cap / wm.upm * size_s
        h2 = pad + mark_s + 28 + cap_s + pad * 2
        body2 = mark_group(theme, (w2 - mark_s) / 2, pad, mark_s / 512) + word_s.replace('<g transform="translate(0 0)">', f'<g transform="translate({(w2 - ws) / 2:.1f} {pad + mark_s + 28 + cap_s:.1f})">', 1)
        with open(os.path.join(OUT, f"openvclient-logo-stacked-{theme}.svg"), "w") as f:
            f.write(doc(w2, h2, body2, "OpenVClient"))

        # Wordmark alone.
        word_w, www = wm.svg(theme, 120)
        cap_w = wm.cap / wm.upm * 120
        with open(os.path.join(OUT, f"openvclient-wordmark-{theme}.svg"), "w") as f:
            f.write(doc(www + 8, cap_w + 16, word_w.replace('<g transform="translate(0 0)">', f'<g transform="translate(4 {cap_w + 8:.1f})">', 1), "OpenVClient"))

    # Favicon: light mark, outline/shadow switch in dark browser themes.
    fav = mark_group("light").replace(f'stroke="{INK}" stroke-width="20"', 'class="edge" stroke-width="20"', 1)
    fav = fav.replace(f'<rect x="58" y="58" width="420" height="420" rx="112" fill="{INK}"/>', '<rect class="shade" x="58" y="58" width="420" height="420" rx="112"/>', 1)
    style = f"<style>.edge{{stroke:{INK}}}.shade{{fill:{INK}}}@media (prefers-color-scheme: dark){{.edge{{stroke:{CREAM}}}.shade{{fill:{CORAL}}}}}</style>"
    with open(os.path.join(OUT, "favicon.svg"), "w") as f:
        f.write(doc(512, 512, style + fav, "OpenVClient"))
    print("logo set written to", OUT)


if __name__ == "__main__":
    main()
