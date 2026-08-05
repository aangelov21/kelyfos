"""Generate the NSIS/WiX installer artwork from the app icon.

NSIS wants plain 24-bit BMPs at fixed sizes; anything with an alpha channel or
a non-standard header renders as garbage or is silently ignored, so every image
is flattened onto an opaque background and saved as RGB.
"""
from PIL import Image, ImageDraw, ImageFont

ICON = "src-tauri/icons/icon.png"
OUT = "src-tauri/installer"

BG = (29, 33, 48)        # the icon's own background
BG_DEEP = (21, 24, 36)   # slightly darker, for the sidebar gradient
ACCENT = (167, 139, 250)
INK = (29, 33, 48)
WHITE = (255, 255, 255)

FONT = "/System/Library/Fonts/HelveticaNeue.ttc"


def font(size, index=0):
    try:
        return ImageFont.truetype(FONT, size, index=index)
    except Exception:
        return ImageFont.load_default()


def icon(size):
    return Image.open(ICON).convert("RGBA").resize((size, size), Image.LANCZOS)


def centred(draw, text, fnt, cx, y, fill):
    left, top, right, bottom = draw.textbbox((0, 0), text, font=fnt)
    draw.text((cx - (right - left) / 2 - left, y), text, font=fnt, fill=fill)
    return bottom - top


def sidebar():
    """164x314, shown on the welcome and finish pages. Full-bleed dark, so it
    reads as the app's own surface rather than a logo pasted on grey."""
    w, h = 164, 314
    img = Image.new("RGB", (w, h), BG)
    d = ImageDraw.Draw(img)

    # Vertical gradient, subtle enough to look like lighting rather than a ramp.
    for y in range(h):
        t = y / (h - 1)
        d.line(
            [(0, y), (w, y)],
            fill=tuple(round(BG[i] + (BG_DEEP[i] - BG[i]) * t) for i in range(3)),
        )

    mark = icon(84)
    img.paste(mark, ((w - 84) // 2, 74), mark)

    centred(d, "KELYFOS", font(15, index=1), w / 2, 182, WHITE)
    # Letterspacing by hand: PIL has no tracking, and the tagline needs to read
    # as a label rather than a word.
    tag, fnt = "S S H   W O R K S P A C E S", font(8)
    centred(d, tag, fnt, w / 2, 204, ACCENT)

    d.line([(52, 226), (112, 226)], fill=(58, 63, 84), width=1)
    return img


def header():
    """150x57, top corner of every wizard page. The strip behind it is system
    white, so this matches rather than punching a dark hole in it."""
    w, h = 150, 57
    img = Image.new("RGB", (w, h), WHITE)
    d = ImageDraw.Draw(img)

    mark = icon(36)
    img.paste(mark, (12, (h - 36) // 2), mark)
    d.text((56, 19), "Kelyfos", font=font(15, index=1), fill=INK)
    return img


def wix_banner():
    """493x58, the MSI's top banner."""
    w, h = 493, 58
    img = Image.new("RGB", (w, h), WHITE)
    d = ImageDraw.Draw(img)
    mark = icon(36)
    img.paste(mark, (18, (h - 36) // 2), mark)
    d.text((64, 20), "Kelyfos", font=font(16, index=1), fill=INK)
    return img


def wix_dialog():
    """493x312, the MSI's welcome page. Left third is artwork, the rest is left
    empty because WiX draws its own text over it."""
    w, h = 493, 312
    img = Image.new("RGB", (w, h), WHITE)
    d = ImageDraw.Draw(img)

    panel = 164
    for y in range(h):
        t = y / (h - 1)
        d.line(
            [(0, y), (panel, y)],
            fill=tuple(round(BG[i] + (BG_DEEP[i] - BG[i]) * t) for i in range(3)),
        )

    mark = icon(84)
    img.paste(mark, ((panel - 84) // 2, 84), mark)
    centred(d, "KELYFOS", font(15, index=1), panel / 2, 192, WHITE)
    return img


if __name__ == "__main__":
    import os

    os.makedirs(OUT, exist_ok=True)
    for name, im in [
        ("sidebar.bmp", sidebar()),
        ("header.bmp", header()),
        ("wix-banner.bmp", wix_banner()),
        ("wix-dialog.bmp", wix_dialog()),
    ]:
        path = os.path.join(OUT, name)
        im.convert("RGB").save(path, "BMP")
        print(f"{path}  {im.size[0]}x{im.size[1]}")
