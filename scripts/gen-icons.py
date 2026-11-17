"""Generate the OpenShell app icons (PNG + ICO) without third-party packages.

Usage: python scripts/gen-icons.py apps/desktop/src-tauri/icons
"""
import os
import struct
import sys
import zlib

BG = (31, 77, 58)        # deep green
FRAME = (236, 235, 231)  # off white
ACCENT = (111, 191, 148) # light green


def rounded_rect(x, y, x0, y0, x1, y1, r):
    if x < x0 or x > x1 or y < y0 or y > y1:
        return False
    cx = min(max(x, x0 + r), x1 - r)
    cy = min(max(y, y0 + r), y1 - r)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def sample(u, v):
    """Color at normalized coordinates (0..1) or None for transparent."""
    if not rounded_rect(u, v, 0.04, 0.04, 0.96, 0.96, 0.2):
        return None
    # window frame
    if rounded_rect(u, v, 0.2, 0.24, 0.8, 0.76, 0.06):
        if v < 0.34:
            return ACCENT  # title bar
        if rounded_rect(u, v, 0.25, 0.39, 0.75, 0.71, 0.03):
            # "managed services" dots inside the window
            for cx in (0.36, 0.5, 0.64):
                if (u - cx) ** 2 + (v - 0.55) ** 2 <= 0.045 ** 2:
                    return ACCENT
            return BG
        return FRAME
    return BG


def render(size, ss=4):
    rows = []
    for y in range(size):
        row = bytearray([0])
        for x in range(size):
            acc = [0, 0, 0, 0]
            for sy in range(ss):
                for sx in range(ss):
                    c = sample((x + (sx + 0.5) / ss) / size, (y + (sy + 0.5) / ss) / size)
                    if c:
                        acc[0] += c[0]; acc[1] += c[1]; acc[2] += c[2]; acc[3] += 255
            n = ss * ss
            a = acc[3] / n
            if a:
                cov = acc[3] / 255
                row += bytes([int(acc[0] / cov), int(acc[1] / cov), int(acc[2] / cov), int(a)])
            else:
                row += bytes(4)
        rows.append(bytes(row))
    return b"".join(rows)


def png(size):
    raw = render(size)
    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def ico(images):
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries, data = b"", b""
    for size, blob in images:
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset + len(data))
        data += blob
    return header + entries + data


def main(out):
    os.makedirs(out, exist_ok=True)
    sizes = {16: None, 32: None, 48: None, 64: None, 128: None, 256: None}
    for s in sizes:
        sizes[s] = png(s)
    files = {"32x32.png": sizes[32], "128x128.png": sizes[128], "128x128@2x.png": png(256), "icon.png": sizes[256]}
    for name, blob in files.items():
        with open(os.path.join(out, name), "wb") as f:
            f.write(blob)
    with open(os.path.join(out, "icon.ico"), "wb") as f:
        f.write(ico([(s, sizes[s]) for s in (16, 32, 48, 64, 128, 256)]))
    print("icons written to", out)


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "icons")
