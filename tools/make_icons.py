import math
import struct
import sys
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SIZES = [16, 20, 24, 32, 40, 48, 64, 256]
UNIT = 128

TILE = (4, 4, 124, 124, 28)
PHONE = (22, 20, 68, 108, 13)
CUTOUT = [(39, 51), (58, 64), (39, 77)]
WAVE_CENTER = (70, 64)
WAVES = [(16, 1.0), (30, 0.55)]
WAVE_SPAN = math.radians(45)

PALETTES = {
    "phone-media": [(0.0, "#86cecb"), (0.6, "#137a7f"), (1.0, "#0f6569")],
    "phone-media-off": [(0.0, "#c4c4c4"), (0.6, "#6e6e6e"), (1.0, "#5c5c5c")],
}


def rgb(color):
    return tuple(int(color[i : i + 2], 16) for i in (1, 3, 5))


def gradient(stops, t):
    t = min(max(t, 0.0), 1.0)
    for (t0, c0), (t1, c1) in zip(stops, stops[1:]):
        if t <= t1:
            k = (t - t0) / (t1 - t0)
            return [a + (b - a) * k for a, b in zip(rgb(c0), rgb(c1))]
    return list(rgb(stops[-1][1]))


def in_rounded_rect(x, y, rect):
    left, top, right, bottom, radius = rect
    if not (left <= x <= right and top <= y <= bottom):
        return False
    cx = min(max(x, left + radius), right - radius)
    cy = min(max(y, top + radius), bottom - radius)
    return (x - cx) ** 2 + (y - cy) ** 2 <= radius**2


def segment_distance(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - ax - t * dx, py - ay - t * dy)


def near_triangle(x, y, points, grow):
    signs = []
    for (ax, ay), (bx, by) in zip(points, points[1:] + points[:1]):
        signs.append((bx - ax) * (y - ay) - (by - ay) * (x - ax) >= 0)
    if all(signs) or not any(signs):
        return True
    return any(
        segment_distance(x, y, ax, ay, bx, by) <= grow
        for (ax, ay), (bx, by) in zip(points, points[1:] + points[:1])
    )


def wave_alpha(x, y, width):
    cx, cy = WAVE_CENTER
    angle = math.atan2(y - cy, x - cx)
    distance = math.hypot(x - cx, y - cy)
    for radius, alpha in WAVES:
        if abs(angle) <= WAVE_SPAN and abs(distance - radius) <= width / 2:
            return alpha
        for sign in (-1, 1):
            ex = cx + radius * math.cos(WAVE_SPAN)
            ey = cy + sign * radius * math.sin(WAVE_SPAN)
            if math.hypot(x - ex, y - ey) <= width / 2:
                return alpha
    return 0.0


def render(size, stops):
    small = size <= 24
    grow = 4.0 if small else 3.0
    wave_width = 9.5 if small else 7.0
    samples = 8 if size <= 64 else 3
    left, top, right, bottom, _ = TILE
    pixels = bytearray()
    for py in range(size):
        for px in range(size):
            red = green = blue = covered = 0.0
            for sy in range(samples):
                for sx in range(samples):
                    x = (px + (sx + 0.5) / samples) * UNIT / size
                    y = (py + (sy + 0.5) / samples) * UNIT / size
                    if not in_rounded_rect(x, y, TILE):
                        continue
                    t = ((x - left) + (y - top)) / ((right - left) + (bottom - top))
                    color = gradient(stops, t)
                    if in_rounded_rect(x, y, PHONE) and not near_triangle(x, y, CUTOUT, grow):
                        white = 1.0
                    else:
                        white = wave_alpha(x, y, wave_width)
                    color = [c + (255 - c) * white for c in color]
                    red, green, blue, covered = red + color[0], green + color[1], blue + color[2], covered + 1
            if covered == 0:
                pixels += bytes(4)
                continue
            alpha = covered / (samples * samples)
            pixels += bytes([round(red / covered), round(green / covered), round(blue / covered), round(255 * alpha)])
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


def number(value):
    return f"{value:.2f}".rstrip("0").rstrip(".")


def rounded_triangle_path(points, grow):
    cx = sum(x for x, _ in points) / 3
    cy = sum(y for _, y in points) / 3
    normals = []
    for (ax, ay), (bx, by) in zip(points, points[1:] + points[:1]):
        nx, ny = by - ay, -(bx - ax)
        length = math.hypot(nx, ny)
        nx, ny = nx / length, ny / length
        if (ax - cx) * nx + (ay - cy) * ny < 0:
            nx, ny = -nx, -ny
        normals.append((nx, ny))
    commands = []
    for index, ((ax, ay), (bx, by)) in enumerate(zip(points, points[1:] + points[:1])):
        nx, ny = normals[index]
        nnx, nny = normals[(index + 1) % 3]
        start = (ax + nx * grow, ay + ny * grow)
        end = (bx + nx * grow, by + ny * grow)
        following = (bx + nnx * grow, by + nny * grow)
        if index == 0:
            commands.append(f"M{number(start[0])},{number(start[1])}")
        commands.append(f"L{number(end[0])},{number(end[1])}")
        commands.append(f"A{number(grow)},{number(grow)} 0 0 1 {number(following[0])},{number(following[1])}")
    return " ".join(commands) + " Z"


def rounded_rect_path(rect):
    left, top, right, bottom, r = rect
    return (
        f"M{left + r},{top} H{right - r} A{r},{r} 0 0 1 {right},{top + r} V{bottom - r} "
        f"A{r},{r} 0 0 1 {right - r},{bottom} H{left + r} A{r},{r} 0 0 1 {left},{bottom - r} "
        f"V{top + r} A{r},{r} 0 0 1 {left + r},{top} Z"
    )


def arc_path(radius):
    cx, cy = WAVE_CENTER
    x = cx + radius * math.cos(WAVE_SPAN)
    dy = radius * math.sin(WAVE_SPAN)
    return f"M{number(x)},{number(cy - dy)} A{radius},{radius} 0 0 1 {number(x)},{number(cy + dy)}"


def android_foreground():
    left, top, right, bottom = PHONE[0], PHONE[1], WAVE_CENTER[0] + WAVES[-1][0] + 3.5, PHONE[3]
    scale = 0.6
    translate_x = 54 - (left + right) / 2 * scale
    translate_y = 54 - (top + bottom) / 2 * scale
    phone = rounded_rect_path(PHONE) + " " + rounded_triangle_path(CUTOUT, 3)
    waves = "\n".join(
        f'        <path\n            android:pathData="{arc_path(radius)}"\n'
        f'            android:strokeColor="#FFFFFF"\n            android:strokeAlpha="{alpha}"\n'
        f'            android:strokeWidth="7"\n            android:strokeLineCap="round" />'
        for radius, alpha in WAVES
    )
    return f"""<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="108dp"
    android:height="108dp"
    android:viewportWidth="108"
    android:viewportHeight="108">
    <group
        android:scaleX="{scale}"
        android:scaleY="{scale}"
        android:translateX="{number(translate_x)}"
        android:translateY="{number(translate_y)}">
        <path
            android:fillColor="#FFFFFF"
            android:fillType="evenOdd"
            android:pathData="{phone}" />
{waves}
    </group>
</vector>
"""


def android_background(stops):
    items = "\n".join(f'                <item android:offset="{offset}" android:color="{color.upper()}" />' for offset, color in stops)
    return f"""<vector xmlns:android="http://schemas.android.com/apk/res/android"
    xmlns:aapt="http://schemas.android.com/aapt"
    android:width="108dp"
    android:height="108dp"
    android:viewportWidth="108"
    android:viewportHeight="108">
    <path android:pathData="M0,0 H108 V108 H0 Z">
        <aapt:attr name="android:fillColor">
            <gradient
                android:type="linear"
                android:startX="18"
                android:startY="18"
                android:endX="90"
                android:endY="90">
{items}
            </gradient>
        </aapt:attr>
    </path>
</vector>
"""


ADAPTIVE_ICON = """<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@drawable/ic_launcher_background" />
    <foreground android:drawable="@drawable/ic_launcher_foreground" />
    <monochrome android:drawable="@drawable/ic_launcher_foreground" />
</adaptive-icon>
"""


def write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(content, bytes):
        path.write_bytes(content)
    else:
        path.write_text(content, encoding="utf-8", newline="\n")


def main():
    preview = Path(sys.argv[1]) if len(sys.argv) > 1 else None
    for name, stops in PALETTES.items():
        images = [(size, png(size, render(size, stops))) for size in SIZES]
        write(ROOT / "pc" / "assets" / f"{name}.ico", ico(images))
        if preview:
            for size, image in images:
                write(preview / f"{name}-{size}.png", image)
        print(f"pc/assets/{name}.ico: {', '.join(str(size) for size in SIZES)}")
    res = ROOT / "android" / "app" / "src" / "main" / "res"
    write(res / "drawable" / "ic_launcher_foreground.xml", android_foreground())
    write(res / "drawable" / "ic_launcher_background.xml", android_background(PALETTES["phone-media"]))
    write(res / "mipmap-anydpi" / "ic_launcher.xml", ADAPTIVE_ICON)
    print("android/app/src/main/res: ic_launcher (адаптивная)")


if __name__ == "__main__":
    main()
