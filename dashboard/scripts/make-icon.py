"""Draws the dashboard's source icon (an eye on a rounded square) as a PNG.

Run once, then `npx tauri icon icon-source.png -o src-tauri/icons` makes the
platform icon sizes. Uses only the standard library.
"""
import math
import struct
import sys
import zlib

SIZE = 1024
BG = (30, 41, 59)        # slate
EYE = (241, 245, 249)    # near white
IRIS = (56, 189, 248)    # sky
PUPIL = (15, 23, 42)


def pixel(x, y):
    cx = cy = SIZE / 2
    # rounded-square background
    r, half = SIZE * 0.2, SIZE / 2
    dx, dy = max(abs(x - cx) - (half - r), 0), max(abs(y - cy) - (half - r), 0)
    if math.hypot(dx, dy) > r:
        return (0, 0, 0, 0)
    # eye: intersection of two large circles (a lens)
    lens_r, offset = SIZE * 0.5, SIZE * 0.27
    in_lens = math.hypot(x - cx, y - (cy - offset)) < lens_r and \
        math.hypot(x - cx, y - (cy + offset)) < lens_r
    d = math.hypot(x - cx, y - cy)
    if in_lens:
        if d < SIZE * 0.07:
            return PUPIL + (255,)
        if d < SIZE * 0.17:
            return IRIS + (255,)
        return EYE + (255,)
    return BG + (255,)


def main(path):
    rows = []
    for y in range(SIZE):
        row = bytearray([0])
        for x in range(SIZE):
            row.extend(pixel(x + 0.5, y + 0.5))
        rows.append(bytes(row))
    raw = b"".join(rows)

    def chunk(kind, data):
        c = struct.pack(">I", len(data)) + kind + data
        return c + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "icon-source.png")
