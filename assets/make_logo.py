"""Generates the TIDE logo (original design): a white rounded square with
black dots drawn along two waves. Writes logo.png (512 px) and icon.ico.

Pure Python (no Pillow): python make_logo.py
"""
import math
import os
import struct
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))


def dots():
    """Dot centres and radii in a 1.0 x 1.0 box."""
    out = []
    # Upper wave: 7 dots, growing towards the crest.
    for i in range(7):
        t = i / 6
        x = 0.17 + t * 0.66
        y = 0.43 - 0.11 * math.sin(t * math.pi * 1.5 + 0.2)
        r = 0.040 + 0.020 * math.sin(t * math.pi)
        out.append((x, y, r))
    # Lower wave: 6 smaller dots, shifted half a step.
    for i in range(6):
        t = i / 5
        x = 0.225 + t * 0.55
        y = 0.64 - 0.09 * math.sin(t * math.pi * 1.5 + 0.9)
        r = 0.030 + 0.012 * math.sin(t * math.pi)
        out.append((x, y, r))
    return out


def inside_rounded(x, y, size, radius):
    cx = min(max(x, radius), size - radius)
    cy = min(max(y, radius), size - radius)
    return (x - cx) ** 2 + (y - cy) ** 2 <= radius ** 2


def render(size, rounded=True):
    """RGBA bytes, 4x4 supersampled."""
    ss = 4
    radius = size * 0.22 if rounded else 0
    ds = [(x * size, y * size, r * size) for x, y, r in dots()]
    rows = []
    for py in range(size):
        row = bytearray()
        for px in range(size):
            cover = 0  # white-square coverage
            black = 0  # dot coverage
            for sy in range(ss):
                for sx in range(ss):
                    x = px + (sx + 0.5) / ss
                    y = py + (sy + 0.5) / ss
                    if rounded and not inside_rounded(x, y, size, radius):
                        continue
                    cover += 1
                    for dx, dy, r in ds:
                        if (x - dx) ** 2 + (y - dy) ** 2 <= r * r:
                            black += 1
                            break
            n = ss * ss
            alpha = cover / n
            if cover == 0:
                row += bytes((0, 0, 0, 0))
                continue
            # Colour of the covered part: white mixed with black dots.
            shade = 1.0 - black / cover
            v = int(round(255 * shade))
            row += bytes((v, v, v, int(round(255 * alpha))))
        rows.append(bytes(row))
    return rows


def png(rows, size):
    raw = b"".join(b"\x00" + r for r in rows)

    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def ico(images):
    """ICO with PNG-compressed entries (Windows Vista and later)."""
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    data = b""
    for size, blob in images:
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset + len(data))
        data += blob
    return header + entries + data


def main():
    big = png(render(512), 512)
    with open(os.path.join(HERE, "logo.png"), "wb") as f:
        f.write(big)
    images = [(s, png(render(s), s)) for s in (16, 24, 32, 48, 64, 128, 256)]
    with open(os.path.join(HERE, "icon.ico"), "wb") as f:
        f.write(ico(images))
    print("logo.png and icon.ico written")


if __name__ == "__main__":
    main()
