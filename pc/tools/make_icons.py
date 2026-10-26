import math
import struct
import sys
import zlib
from pathlib import Path

SIZES = [16, 20, 24, 32, 40, 48, 64, 256]
COLORS = {
    "phone-media": ((124, 77, 255), (41, 121, 255)),
    "phone-media-off": ((158, 158, 158), (97, 97, 97)),
}


def in_rounded_square(x, y, inset=0.03, radius=0.22):
    left, right = inset, 1 - inset
    cx = min(max(x, left + radius), right - radius)
    cy = min(max(y, left + radius), right - radius)
    return (x - cx) ** 2 + (y - cy) ** 2 <= radius**2 and left <= x <= right and left <= y <= right


def in_ellipse(x, y, cx, cy, rx, ry, angle):
    c, s = math.cos(angle), math.sin(angle)
    dx, dy = x - cx, y - cy
    u, v = dx * c + dy * s, -dx * s + dy * c
    return (u / rx) ** 2 + (v / ry) ** 2 <= 1


def in_convex(x, y, points):
    for (x1, y1), (x2, y2) in zip(points, points[1:] + points[:1]):
        if (x2 - x1) * (y - y1) - (y2 - y1) * (x - x1) < 0:
            return False
    return True


TILT = math.radians(-22)
NOTE = [
    lambda x, y: in_ellipse(x, y, 0.335, 0.705, 0.125, 0.09, TILT),
    lambda x, y: in_ellipse(x, y, 0.645, 0.625, 0.125, 0.09, TILT),
    lambda x, y: 0.405 <= x <= 0.46 and 0.29 <= y <= 0.69,
    lambda x, y: 0.715 <= x <= 0.77 and 0.21 <= y <= 0.61,
    lambda x, y: in_convex(x, y, [(0.405, 0.29), (0.77, 0.21), (0.77, 0.315), (0.405, 0.395)]),
]


def render(size, top, bottom):
    samples = 8 if size <= 64 else 3
    pixels = bytearray()
    for py in range(size):
        for px in range(size):
            tile = note = 0
            for sy in range(samples):
                for sx in range(samples):
                    x = (px + (sx + 0.5) / samples) / size
                    y = (py + (sy + 0.5) / samples) / size
                    if in_rounded_square(x, y):
                        tile += 1
                        if any(shape(x, y) for shape in NOTE):
                            note += 1
            total = samples * samples
            t = min(max((px + py) / (2 * size - 2), 0), 1)
            base = [round(a + (b - a) * t) for a, b in zip(top, bottom)]
            if tile == 0:
                pixels += bytes(4)
                continue
            mix = note / tile
            color = [round(c + (255 - c) * mix) for c in base]
            pixels += bytes(color + [round(255 * tile / total)])
    return bytes(pixels)


def png(size, rgba):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    rows = b"".join(b"\0" + rgba[y * size * 4 : (y + 1) * size * 4] for y in range(size))
    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")


def ico(images):
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = data = b""
    for size, image in images:
        side = 0 if size == 256 else size
        entries += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(image), offset)
        data += image
        offset += len(image)
    return header + entries + data


def main():
    assets = Path(__file__).resolve().parent.parent / "assets"
    assets.mkdir(exist_ok=True)
    preview = Path(sys.argv[1]) if len(sys.argv) > 1 else None
    for name, (top, bottom) in COLORS.items():
        images = [(size, png(size, render(size, top, bottom))) for size in SIZES]
        (assets / f"{name}.ico").write_bytes(ico(images))
        if preview:
            preview.mkdir(parents=True, exist_ok=True)
            for size, image in images:
                (preview / f"{name}-{size}.png").write_bytes(image)
        print(f"{name}.ico: {', '.join(str(size) for size in SIZES)}")


if __name__ == "__main__":
    main()
