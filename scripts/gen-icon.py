#!/usr/bin/env python3
"""Draw a small gauge icon for the tray app."""

import struct
import zlib
from pathlib import Path


def chunk(tag: bytes, data: bytes) -> bytes:
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)


def write_png(path: Path, size: int) -> bytes:
    raw = bytearray()
    for y in range(size):
        raw.append(0)
        for x in range(size):
            raw.extend(pixel(x, y, size))
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")
    path.write_bytes(png)
    return png


def pixel(x: int, y: int, size: int) -> bytes:
    nx = (x + 0.5) / size
    ny = (y + 0.5) / size
    radius = ((nx - 0.5) ** 2 + (ny - 0.5) ** 2) ** 0.5
    corner = rounded(nx, ny)
    if not corner:
        return (0, 0, 0, 0)
    if 0.30 < radius < 0.40:
        angle = angle_of(nx, ny)
        if in_arc(angle):
            return (142, 182, 255, 255)
        return (42, 49, 66, 255)
    if radius <= 0.12:
        return (142, 182, 255, 255)
    return (20, 24, 34, 255)


def rounded(nx: float, ny: float) -> bool:
    margin = 0.08
    radius = 0.18
    left, right = margin, 1 - margin
    top, bottom = margin, 1 - margin
    if nx < left or nx > right or ny < top or ny > bottom:
        return False
    cx = min(max(nx, left + radius), right - radius)
    cy = min(max(ny, top + radius), bottom - radius)
    return (nx - cx) ** 2 + (ny - cy) ** 2 <= radius ** 2


def angle_of(nx: float, ny: float) -> float:
    import math

    return math.degrees(math.atan2(ny - 0.5, nx - 0.5)) % 360


def in_arc(angle: float) -> bool:
    # Clockwise-looking gauge from the lower left, through the top, to the right.
    start = 130
    end = 400
    swept = (angle - start) % 360
    return swept <= (end - start) % 360


def write_ico(path: Path, images: list[tuple[int, bytes]]) -> None:
    count = len(images)
    header = struct.pack("<HHH", 0, 1, count)
    entries = b""
    offset = 6 + 16 * count
    blobs = b""
    for size, blob in images:
        dim = size if size < 256 else 0
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
        blobs += blob
    path.write_bytes(header + entries + blobs)


def main() -> None:
    out = Path(__file__).resolve().parents[1] / "apps" / "desktop" / "src-tauri" / "icons"
    out.mkdir(parents=True, exist_ok=True)
    blobs = []
    for size, name in ((32, "32x32.png"), (128, "128x128.png"), (256, "128x128@2x.png"), (256, "icon.png")):
        blobs.append((size, write_png(out / name, size)))
    write_ico(out / "icon.ico", [(32, blobs[0][1]), (256, blobs[2][1])])


if __name__ == "__main__":
    main()
