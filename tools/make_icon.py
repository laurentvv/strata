"""Generates the Strata icon (assets/icon.ico and assets/icon.png). Own artwork, no third-party assets.

Three stacked rock layers ("strata") of different widths on a dark rounded tile; the widths read as
a bar chart of disk usage. Run: python tools/make_icon.py
"""
from PIL import Image, ImageDraw, ImageFilter
import os

S = 1024  # supersampled canvas

def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))

def tile():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    grad = Image.new("RGBA", (S, S))
    px = grad.load()
    top, bot = (28, 38, 52), (12, 16, 24)
    for y in range(S):
        c = lerp(top, bot, y / S)
        for x in range(S):
            px[x, y] = c + (255,)
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle((24, 24, S - 24, S - 24), radius=210, fill=255)
    img.paste(grad, (0, 0), mask)
    return img

def slab(img, x0, x1, y, h, base, light):
    """A layer: lighter top face, darker front face, rounded ends."""
    d = ImageDraw.Draw(img)
    top_h = int(h * 0.38)
    d.rounded_rectangle((x0, y, x1, y + h), radius=int(h * 0.30), fill=base)
    d.rounded_rectangle((x0, y, x1, y + top_h + int(h * 0.30)), radius=int(h * 0.30), fill=light)
    d.rectangle((x0 + int(h * 0.30), y + top_h, x1 - int(h * 0.30), y + h - int(h * 0.30)), fill=base)
    d.rounded_rectangle((x0, y + top_h, x1, y + h), radius=int(h * 0.30), fill=base)

def build():
    img = tile()
    # (left, right, colour, light colour): widths differ like a usage chart
    # Tightly stacked, slightly shifted layers like rock strata; the shifts keep a chart-like rhythm.
    layers = [
        (170, 800, (43, 181, 160), (118, 224, 200)),   # teal
        (224, 854, (242, 163, 58), (255, 208, 128)),   # amber
        (150, 700, (70, 110, 150), (126, 166, 204)),   # slate blue
    ]
    h, gap, y = 205, 14, 205
    for x0, x1, base, light in layers:
        slab(img, x0, x1, y, h, base, light)
        y += h + gap
    return img

def main():
    big = build()
    out = os.path.join(os.path.dirname(__file__), "..", "assets")
    os.makedirs(out, exist_ok=True)
    big.resize((256, 256), Image.LANCZOS).save(os.path.join(out, "icon.png"))
    sizes = [16, 24, 32, 48, 64, 128, 256]
    frames = [big.resize((s, s), Image.LANCZOS) for s in sizes]
    frames[-1].save(os.path.join(out, "icon.ico"), format="ICO", sizes=[(s, s) for s in sizes], append_images=frames[:-1])

if __name__ == "__main__":
    main()
