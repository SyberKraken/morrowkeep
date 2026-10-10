"""Writes assets/morrowkeep.ico (several sizes) and assets/morrowkeep.png (256 px, used for toast
notifications): the same green check icon as tray::icon_rgba.

Pure standard library (no Pillow). Run: python assets/make_icon.py
"""
import math
import os
import struct
import zlib


def icon_rgba(size):
    s = float(size)

    def seg_dist(p, a, b):
        abx, aby = b[0] - a[0], b[1] - a[1]
        t = ((p[0] - a[0]) * abx + (p[1] - a[1]) * aby) / (abx * abx + aby * aby)
        t = min(max(t, 0.0), 1.0)
        cx, cy = a[0] + t * abx, a[1] + t * aby
        return math.hypot(p[0] - cx, p[1] - cy)

    r = s * 0.22
    rows = []
    for y in range(size):
        row = bytearray()
        for x in range(size):
            p = (x + 0.5, y + 0.5)
            dx = max(r - p[0], p[0] - (s - r), 0.0)
            dy = max(r - p[1], p[1] - (s - r), 0.0)
            inside = -min(max(math.hypot(dx, dy) - r, -0.5), 0.5) + 0.5
            check = min(
                seg_dist(p, (s * 0.27, s * 0.53), (s * 0.43, s * 0.69)),
                seg_dist(p, (s * 0.43, s * 0.69), (s * 0.74, s * 0.33)),
            )
            w = min(max(s * 0.07 - check + 0.5, 0.0), 1.0)
            c = lambda g: int(g + (255.0 - g) * w)
            row += bytes([c(46.0), c(160.0), c(103.0), int(inside * 255.0)])
        rows.append(bytes(row))
    return rows


def png(size):
    raw = b"".join(b"\x00" + row for row in icon_rgba(size))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


sizes = [16, 24, 32, 48, 64, 128, 256]
images = [png(n) for n in sizes]
header = struct.pack("<HHH", 0, 1, len(sizes))
offset = 6 + 16 * len(sizes)
entries = b""
for n, data in zip(sizes, images):
    entries += struct.pack("<BBBBHHII", n % 256, n % 256, 0, 0, 1, 32, len(data), offset)
    offset += len(data)
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "morrowkeep.ico")
with open(out, "wb") as f:
    f.write(header + entries + b"".join(images))
print("wrote", out)

png_out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "morrowkeep.png")
with open(png_out, "wb") as f:
    f.write(png(256))
print("wrote", png_out)
