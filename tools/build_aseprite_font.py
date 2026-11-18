#!/usr/bin/env python3
"""Aseprite font sheet (PNG) -> TTF: every font pixel becomes a square of one em unit.

Usage: build_aseprite_font.py aseprite_font.png out.ttf
Sheet: a grid of 16 columns with an 11 pixel step, the first cell is space (U+0020), then Unicode code points in order;
black pixels are the glyph, transparent ones the cell background, white ones are past the glyph edge, red means no glyph.
A glyph fills its cell from the left, the cell width is the advance (the empty right column is the letter gap).
Aseprite font © David Capello, CC BY 4.0. Capital Ё, Й and small ё, й have no accents in the sheet
(Й looks like И), so the accents are drawn here above the cell, which is why the ascent is above the cap height.
"""
import sys

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from PIL import Image

GRID_ORIGIN, GRID_STEP, GRID_COLUMNS, FIRST_CODE = 1, 11, 16, 0x20
CELL_ROWS = 7
BASELINE_ROW = 5  # sheet row the letters stand on; descenders go below
ASCENT, DESCENT = 8, 1
WHITE, RED, BLACK = (255, 255, 255, 255), (255, 0, 0, 255), (0, 0, 0, 255)
LAST_CODE = 0x058F
# Accents missing from the sheet: (code, [(x, y_from_baseline)]).
ACCENTS = {
    0x0401: [(1, 7), (3, 7)],
    0x0419: [(0, 7), (3, 7), (1, 6), (2, 6)],
    0x0451: [(1, 6), (3, 6)],
    0x0439: [(0, 6), (3, 6), (1, 5), (2, 5)],
}
# Glyphs missing from the sheet entirely: (code, cell width, pixels (x, y_from_baseline)).
CHECKMARK = {(6, 4), (5, 3), (0, 2), (4, 2), (1, 1), (3, 1), (2, 0)}
EXTRA_GLYPHS = [(0x2713, 8, CHECKMARK), (0x2714, 8, CHECKMARK)]
# The sheet draws the й accent as one dot above the line; it is cleared before drawing the new one.
STALE_ROW = {0x0439: 0}


def read_cells(sheet):
    cells = {}
    rows = (sheet.height - GRID_ORIGIN) // GRID_STEP
    for row in range(rows):
        for column in range(GRID_COLUMNS):
            x0, y0 = GRID_ORIGIN + column * GRID_STEP, GRID_ORIGIN + row * GRID_STEP
            if sheet.getpixel((x0, y0)) == RED:
                continue
            width = 0
            while width < GRID_STEP - 1 and sheet.getpixel((x0 + width, y0)) not in (WHITE, RED):
                width += 1
            code = FIRST_CODE + row * GRID_COLUMNS + column
            pixels = {(x, BASELINE_ROW - y) for y in range(CELL_ROWS) for x in range(width) if sheet.getpixel((x0 + x, y0 + y)) == BLACK}
            cells[code] = (width, pixels)
    return cells


def with_extra_glyphs(cells):
    for code, width, pixels in EXTRA_GLYPHS:
        cells[code] = (width, set(pixels))
    return cells


def with_accents(cells):
    for code, marks in ACCENTS.items():
        width, pixels = cells[code]
        stale = BASELINE_ROW - STALE_ROW.get(code, -99)
        cells[code] = (width, {p for p in pixels if p[1] != stale} | set(marks))
    return cells


def outline(pixels):
    pen = TTGlyphPen(None)
    for x, y in sorted(pixels):
        pen.moveTo((x, y))
        pen.lineTo((x, y + 1))
        pen.lineTo((x + 1, y + 1))
        pen.lineTo((x + 1, y))
        pen.closePath()
    return pen.glyph()


def build(sheet_path, ttf_path):
    cells = with_extra_glyphs(with_accents(read_cells(Image.open(sheet_path).convert("RGBA"))))
    order, cmap, glyphs, metrics = [".notdef"], {}, {}, {}
    notdef_width, notdef_pixels = cells[ord("?")]
    glyphs[".notdef"], metrics[".notdef"] = outline(notdef_pixels), (notdef_width, 0)
    for code, (width, pixels) in sorted(cells.items()):
        if code > LAST_CODE and code not in {extra[0] for extra in EXTRA_GLYPHS}:
            continue
        name = f"u{code:04X}"
        order.append(name)
        cmap[code] = name
        glyphs[name], metrics[name] = outline(pixels), (width, 0)
    em = ASCENT + DESCENT
    builder = FontBuilder(em, isTTF=True)
    builder.setupGlyphOrder(order)
    builder.setupCharacterMap(cmap)
    builder.setupGlyf(glyphs)
    builder.setupHorizontalMetrics(metrics)
    builder.setupHorizontalHeader(ascent=ASCENT, descent=-DESCENT, lineGap=0)
    builder.setupNameTable({"familyName": "Aseprite", "styleName": "Regular"})
    builder.setupOS2(sTypoAscender=ASCENT, sTypoDescender=-DESCENT, sTypoLineGap=0, usWinAscent=ASCENT, usWinDescent=DESCENT)
    builder.setupPost(isFixedPitch=0)
    builder.save(ttf_path)
    print(f"{ttf_path}: {len(cmap)} glyphs, em={em}")


if __name__ == "__main__":
    build(*sys.argv[1:3])
