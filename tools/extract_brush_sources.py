import io
import json
import sys
import urllib.parse
import urllib.request
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter
from scipy import ndimage
from skimage.feature import peak_local_max
from skimage.segmentation import watershed

API = "https://commons.wikimedia.org/w/api.php"
AGENT = {"User-Agent": "chequer-brush-sources/1.0 (huecoder@proton.me)"}
DOWNLOAD_WIDTH = 3840
OUTPUT_DIR = Path(__file__).resolve().parent.parent / "crates/chequer-core/assets/sources"
MAX_STAMP_SIDE = 512
MAX_STONE_SIDE = 256
STONE_LEVEL = 0.5
STONE_MIN_DISTANCE = 60
STONE_MIN_AREA = 60_000
STONE_PICKS = (1, 2, 5)

LIGHTNING = ("File:Lightning In Sky.jpg", 1, [])
TRUNK_COLUMNS = (330, 560)
LOOP_HALF_WIDTH = 150
MAX_LOOP_SIDE = 1280
OSTEOSPERMUM = ("File:White daisy (Unsplash).jpg", 7.68, [(170, 95, 350, 172)])
STONES = "File:Light grey stones.jpg"


def download(title, width=DOWNLOAD_WIDTH):
    query = {"action": "query", "format": "json", "titles": title, "prop": "imageinfo", "iiprop": "url", "iiurlwidth": width}
    info = json.load(urllib.request.urlopen(urllib.request.Request(f"{API}?{urllib.parse.urlencode(query)}", headers=AGENT), timeout=30))
    page = next(iter(info["query"]["pages"].values()))["imageinfo"][0]
    data = urllib.request.urlopen(urllib.request.Request(page["thumburl"], headers=AGENT), timeout=120).read()
    return Image.open(io.BytesIO(data)).convert("RGB")


def smooth_step(low, high, values):
    fraction = np.clip((values - low) / (high - low), 0.0, 1.0)
    return fraction * fraction * (3.0 - 2.0 * fraction)


def save_mask(alpha, path, max_side):
    image = Image.fromarray((np.clip(alpha, 0.0, 1.0) * 255).round().astype("uint8"), "L")
    image.thumbnail((max_side, max_side), Image.LANCZOS)
    image.save(path, optimize=True)


def bright_on_dark(image, crop, scale, low, high, fill_holes):
    left, top, right, bottom = (round(value * scale) for value in crop)
    pixels = np.asarray(image.crop((left, top, right, bottom)), dtype=np.float32) / 255.0
    alpha = smooth_step(low, high, pixels.max(axis=2))
    if fill_holes:
        solid = ndimage.binary_fill_holes(alpha > 0.3)
        alpha = np.where(solid, np.maximum(alpha, 0.9), alpha)
    return alpha


def lightning():
    title, _, _ = LIGHTNING
    gray = download(title, 960).convert("L")
    brightness = np.asarray(gray, dtype=np.float32)
    background = np.asarray(gray.filter(ImageFilter.GaussianBlur(25)), dtype=np.float32)
    contrast = brightness - background
    alpha = smooth_step(0.06, 0.55, np.clip(contrast / np.percentile(contrast, 99.9), 0.0, 1.0))
    trunk = ndimage.gaussian_filter1d(alpha[:, TRUNK_COLUMNS[0] : TRUNK_COLUMNS[1]], 8, axis=1).argmax(axis=1) + TRUNK_COLUMNS[0]
    centre = int(round(trunk.mean()))
    window = alpha[:, centre - LOOP_HALF_WIDTH : centre + LOOP_HALF_WIDTH]
    loop = np.concatenate([window, window[::-1]], axis=0)
    save_mask(loop.T, OUTPUT_DIR / "lightning_loop.png", MAX_LOOP_SIDE)


def flowers():
    for (title, scale, crops), name, fill, low, high in ((OSTEOSPERMUM, "osteospermum", True, 0.1, 0.5),):
        image = download(title)
        for number, crop in enumerate(crops, 1):
            save_mask(bright_on_dark(image, crop, scale, low, high, fill), OUTPUT_DIR / f"{name}_{number}.png", MAX_STAMP_SIDE)


def stones():
    image = download(STONES)
    brightness = np.asarray(image.convert("L").filter(ImageFilter.GaussianBlur(2)), dtype=np.float32) / 255.0
    solid = ndimage.binary_opening(brightness > STONE_LEVEL, iterations=4)
    distance = ndimage.gaussian_filter(ndimage.distance_transform_edt(solid), 6)
    peaks = peak_local_max(distance, min_distance=STONE_MIN_DISTANCE, labels=ndimage.label(solid)[0])
    markers = np.zeros(solid.shape, dtype=int)
    markers[tuple(peaks.T)] = np.arange(1, len(peaks) + 1)
    labels = watershed(-distance, markers, mask=solid)
    height, width = solid.shape
    kept = []
    for index, box in enumerate(ndimage.find_objects(labels), 1):
        if box is None:
            continue
        rows, columns = box
        if rows.start == 0 or columns.start == 0 or rows.stop == height or columns.stop == width:
            continue
        region = labels[box] == index
        area = int(region.sum())
        if area < STONE_MIN_AREA:
            continue
        kept.append((area / convex_hull_area(region), area, box, region))
    kept.sort(key=lambda item: -item[0])
    for number, rank in enumerate(STONE_PICKS, 1):
        ratio, area, box, region = kept[rank - 1]
        pad = 12
        rows, columns = box
        window = (slice(max(rows.start - pad, 0), rows.stop + pad), slice(max(columns.start - pad, 0), columns.stop + pad))
        mask = np.zeros(solid.shape, dtype=bool)
        mask[box] = region
        silhouette = np.asarray(Image.fromarray(mask[window].astype("uint8") * 255).filter(ImageFilter.GaussianBlur(2)), dtype=np.float32) / 255.0
        silhouette = smooth_step(0.35, 0.65, silhouette)
        light = brightness[window]
        low, high = np.percentile(light[mask[window]], (5, 95))
        shade = 1.0 - np.clip((light - low) / (high - low), 0.0, 1.0)
        save_mask(silhouette * (0.55 + 0.45 * shade), OUTPUT_DIR / f"stone_{number}.png", MAX_STONE_SIDE)


def convex_hull_area(region):
    rows, columns = np.nonzero(region)
    points = np.stack([columns, rows], axis=1).astype(np.float64)
    from scipy.spatial import ConvexHull

    return ConvexHull(points).volume


if __name__ == "__main__":
    steps = {"lightning": lightning, "flowers": flowers, "stones": stones}
    for name in sys.argv[1:] or steps:
        steps[name]()
