#!/usr/bin/env python3
"""App icon from the source ball: cuts out the ball, puts it on a square and writes PNGs of several sizes and an .ico.

The source is a 121x124 render with a hard edge, so the alpha is recomputed from an analytic ellipse
(smooth edges at any size), and the color comes from the source smeared past the ball edge first.

Usage: python3 tools/make_app_icon.py docs/assets/logo-source/blueball.png crates/chequer-app/assets/icon
"""
import sys
from pathlib import Path

import numpy as np
from PIL import Image

SIZES = (16, 24, 32, 48, 64, 128, 256)
ICO_SIZES = (16, 32, 48, 64, 128, 256)
BALL_SHARE = 0.94
SUPERSAMPLE = 8
BLEED_PASSES = 6


def bleed_color(rgba: np.ndarray) -> np.ndarray:
    """Fills transparent pixels near the edge with neighbor colors, otherwise resizing pulls black into the ball edge."""
    rgba = rgba.copy()
    for _ in range(BLEED_PASSES):
        opaque = rgba[..., 3] > 0
        padded = np.pad(rgba[..., :3].astype(np.float32) * opaque[..., None], ((1, 1), (1, 1), (0, 0)))
        weight = np.pad(opaque.astype(np.float32), 1)
        total = sum(padded[dy:dy + rgba.shape[0], dx:dx + rgba.shape[1]] for dy in range(3) for dx in range(3))
        count = sum(weight[dy:dy + rgba.shape[0], dx:dx + rgba.shape[1]] for dy in range(3) for dx in range(3))
        fill = ~opaque & (count > 0)
        rgba[fill, :3] = (total[fill] / count[fill][:, None]).astype(np.uint8)
        rgba[fill, 3] = 1
    rgba[..., 3] = np.where(rgba[..., 3] == 1, 0, rgba[..., 3])
    return rgba


def ellipse_alpha(width: int, height: int, cx: float, cy: float, rx: float, ry: float) -> np.ndarray:
    big_w, big_h = width * SUPERSAMPLE, height * SUPERSAMPLE
    ys, xs = np.mgrid[0:big_h, 0:big_w]
    x = (xs + 0.5) / SUPERSAMPLE
    y = (ys + 0.5) / SUPERSAMPLE
    inside = ((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2 <= 1.0
    coverage = inside.reshape(height, SUPERSAMPLE, width, SUPERSAMPLE).mean(axis=(1, 3))
    return (coverage * 255).round().astype(np.uint8)


def main(source: Path, out_dir: Path) -> None:
    image = Image.open(source).convert("RGBA")
    pixels = np.array(image)
    ys, xs = np.where(pixels[..., 3] > 0)
    left, right, top, bottom = xs.min(), xs.max() + 1, ys.min(), ys.max() + 1
    ball = pixels[top:bottom, left:right]
    side = max(ball.shape[:2])
    square = np.zeros((side, side, 4), dtype=np.uint8)
    off_y, off_x = (side - ball.shape[0]) // 2, (side - ball.shape[1]) // 2
    square[off_y:off_y + ball.shape[0], off_x:off_x + ball.shape[1]] = ball
    color = Image.fromarray(bleed_color(square)).convert("RGB")
    rx, ry = ball.shape[1] / 2, ball.shape[0] / 2
    out_dir.mkdir(parents=True, exist_ok=True)
    frames = []
    for size in SIZES:
        target = round(size * BALL_SHARE)
        margin = (size - target) / 2
        scaled = color.resize((target, target), Image.LANCZOS)
        canvas = Image.new("RGB", (size, size))
        canvas.paste(scaled, (round(margin), round(margin)))
        scale = target / side
        alpha = ellipse_alpha(size, size, size / 2, size / 2, rx * scale, ry * scale)
        frame = canvas.convert("RGBA")
        frame.putalpha(Image.fromarray(alpha))
        frame.save(out_dir / f"chequer-{size}.png", optimize=True)
        frames.append(frame)
    largest = frames[-1]
    largest.save(out_dir / "chequer.ico", sizes=[(s, s) for s in ICO_SIZES])


if __name__ == "__main__":
    main(Path(sys.argv[1]), Path(sys.argv[2]))
