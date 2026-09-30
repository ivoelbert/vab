#!/usr/bin/env python3
"""Sunset Riders artwork on an enlarged white-trim four-player cabinet.

Run: python3 tools/art/build_sunsetriders.py (requires Pillow).
Uses the supplied decal sheet, not Snow Bros. artwork or gameplay. The display
is an explicitly stylized western placeholder, not a captured emulator frame.
"""
from PIL import Image, ImageDraw
import build_mk2 as base
import build_snowbros as shell

ROOT = base.ROOT
REF = ROOT / "art/references/sunsetriders"
WIDTH, HEIGHT = 48, 56
# Four character-colored stations: Steve, Billy, Bob, and Cormano.
PLAYERS = ((15.5, (249, 204, 55)), (10.5, (53, 142, 239)),
           (5.5, (68, 190, 98)), (.5, (235, 87, 180)))


def crop(sheet, box):
    # Landmarks were measured on a 2000px-wide preview; preserve original pixels.
    scale = sheet.width / 2000
    return sheet.crop(tuple(round(c * scale) for c in box))


def side(sheet, box, polygon):
    image = crop(sheet, box)
    scale = sheet.width / 2000
    mask = Image.new("L", image.size)
    points = [(round((x - box[0]) * scale), round((y - box[1]) * scale)) for x, y in polygon]
    ImageDraw.Draw(mask).polygon(points, fill=255)
    # Continue the sky/land ramp into the cut-out area of the decal. This prevents
    # the sheet's white page from leaking through the model's different outline.
    stops = ((0, (95, 153, 188)), (.18, (170, 143, 171)), (.32, (229, 151, 139)),
             (.43, (250, 194, 113)), (.49, (244, 201, 132)), (.50, (52, 72, 48)),
             (.67, (62, 79, 48)), (.72, (211, 170, 98)), (1, (181, 114, 70)))
    background = Image.new("RGB", image.size)
    draw = ImageDraw.Draw(background)
    for row in range(image.height):
        t = row / max(1, image.height - 1)
        for (start, a), (end, b) in zip(stops, stops[1:]):
            if start <= t <= end:
                f = (t - start) / (end - start)
                color = tuple(round(x + (y - x) * f) for x, y in zip(a, b))
                draw.line((0, row, image.width - 1, row), fill=color)
                break
    background.paste(image, mask=mask)
    return background


def western_screen():
    """Small attract-style western illustration; intentionally not real gameplay."""
    image = Image.new("RGB", (64, 48), (230, 141, 69))
    draw = ImageDraw.Draw(image)
    draw.rectangle((0, 0, 63, 3), fill=(17, 22, 31))
    draw.rectangle((5, 1, 24, 2), fill=(245, 210, 97))
    draw.rectangle((40, 1, 58, 2), fill=(235, 216, 150))
    draw.ellipse((36, 6, 49, 19), fill=(255, 222, 132))
    draw.polygon(((0, 26), (12, 16), (23, 26), (33, 19), (44, 26), (56, 16), (63, 25), (63, 39), (0, 39)), fill=(153, 80, 66))
    for x, width, top in ((1, 17, 20), (23, 18, 22), (47, 15, 19)):
        draw.rectangle((x, top, x + width, 37), fill=(113, 64, 43))
        draw.rectangle((x - 1, top, x + width + 1, top + 2), fill=(76, 45, 35))
        for window in (x + 3, x + width - 5):
            draw.rectangle((window, top + 5, window + 2, top + 8), fill=(251, 195, 96))
    draw.rectangle((0, 38, 63, 44), fill=(200, 135, 68))
    draw.rectangle((0, 45, 63, 47), fill=(64, 43, 35))
    for x, shirt in ((17, (76, 168, 210)), (46, (221, 73, 58))):
        draw.rectangle((x - 3, 29, x + 3, 30), fill=(36, 29, 31))
        draw.rectangle((x - 1, 27, x + 1, 29), fill=(55, 35, 29))
        draw.rectangle((x - 1, 31, x + 1, 32), fill=(247, 188, 132))
        draw.rectangle((x - 2, 33, x + 2, 37), fill=shirt)
        draw.line((x - 1, 38, x - 2, 42), fill=(36, 42, 61), width=2)
        draw.line((x + 1, 38, x + 2, 42), fill=(36, 42, 61), width=2)
        draw.line((x + 2, 34, x + 5, 34), fill=(40, 31, 30), width=2)
    return image


def textures():
    sheet = Image.open(REF / "decals.png").convert("RGB")
    left = side(sheet, (8, 193, 694, 1525),
                ((8, 423), (40, 345), (100, 263), (230, 205), (345, 193),
                 (390, 208), (404, 248), (370, 416), (383, 608), (415, 723),
                 (477, 808), (693, 808), (693, 925), (620, 1013), (620, 1525), (8, 1525)))
    right = side(sheet, (1320, 193, 1998, 1525),
                 ((1998, 423), (1960, 345), (1900, 263), (1770, 205), (1655, 193),
                  (1610, 208), (1596, 248), (1630, 416), (1617, 608), (1585, 723),
                  (1523, 808), (1320, 808), (1320, 925), (1387, 1013), (1387, 1525), (1998, 1525)))
    right = right.transpose(Image.Transpose.FLIP_LEFT_RIGHT)
    images = {
        "left": left, "right": right,
        "marquee": crop(sheet, (755, 1, 1245, 138)),
        "bezel": crop(sheet, (755, 160, 1245, 649)),
        "controls": crop(sheet, (737, 684, 1264, 973)),
        "panel": crop(sheet, (755, 711, 1245, 837)),
        "front": crop(sheet, (755, 1010, 1245, 1686)),
        "screen": western_screen(),
    }
    sizes = {"left": (18, 54), "right": (18, 54), "marquee": (24, 6),
             "bezel": (24, 16), "controls": (30, 10), "panel": (30, 4),
             "front": (21, 26), "screen": (20, 10)}
    return {name: image.resize(sizes[name], Image.Resampling.BOX) for name, image in images.items()}


def model():
    # Body is 16 units wide (the small cabinets are 10); the projecting
    # four-player deck is 20 units wide, twice the two-player deck's width.
    solids = [
        base.Solid("lower", 2, 11, 0, 16, 0, 17),
        base.Solid("upper", 2, 10, 1, 15, 17, 31, (((8, 0, 1), 99),)),
        base.Solid("hood", 2, 10.5, 1, 15, 28, 36, (((1, 0, -2), -49.5),)),
        base.Solid("deck", 8, 15, -2, 18, 16.5, 20.5, (((1, 0, 7), 148),)),
    ]
    for ymin, ymax in ((0, 1), (15, 16)):
        solids += [
            base.Solid("side", 2, 11, ymin, ymax, 0, 17),
            base.Solid("side", 2, 10, ymin, ymax, 17, 30, (((8, 0, 1), 102),)),
            base.Solid("side", 2, 10.5, ymin, ymax, 29, 36, (((1, 0, -2), -49.5),)),
        ]
    for y, color in PLAYERS:
        solids.append(base.Solid("stick", 10.6, 11, y - .2, y + .2, 19.4, 22))
        solids.append(base.Ball((10.8, y, 22.25), .75, color))
        for x, dy in ((12.2, -1.2), (13.1, -2.0)):
            z = (148 - x) / 7
            solids.append(base.Solid("button", x - .4, x + .4, y + dy - .4,
                                     y + dy + .4, z, z + .3, color=color))
    return solids


def side_front(z):
    if z < 17:
        return 11
    if z < 29:
        return min(10, (102 - z) / 8)
    return min(10.5, 2 * z - 49.5)


def paint(solid, p, n, tex):
    x, y, z = p
    nx, ny, nz = n
    part = solid.part
    if part == "side":
        if abs(ny) < .9 or x > side_front(z) - .65 or z > 35.35:
            return "trim", shell.TRIM, False
        return "side art", base.sample(tex["left" if ny > 0 else "right"],
                                       (x - 2) / 9, (36 - z) / 36), False
    if part == "deck":
        if nz > .7:
            return "controls", base.sample(tex["controls"], (18 - y) / 20, (x - 8) / 7), False
        if nx > .9:
            if z > 18.65 or z < 16.8 or y < -1.5 or y > 17.5:
                return "trim", shell.TRIM, False
            return "controls", base.sample(tex["panel"], (18 - y) / 20, (19 - z) / 2.5), False
        return "controls", shell.BLACK, False
    if part == "hood" and nx > .9:
        if z > 35.4 or z < 32.6:
            return "trim", (45, 49, 50), False
        return "marquee", base.sample(tex["marquee"], (15 - y) / 14, (36 - z) / 4), True
    if part == "upper" and nx > .5:
        if 2 < y < 14 and 21.4 < z < 28.4:
            return "screen", base.sample(tex["screen"], (14 - y) / 12, (28.4 - z) / 7), True
        return "screen", base.sample(tex["bezel"], (15 - y) / 14, (29.5 - z) / 9), False
    if part == "lower" and nx > .9:
        if 3.5 < y < 12.5 and 5.5 < z < 12:
            if y < 3.9 or y > 12.1 or z < 5.9 or z > 11.6:
                return "front", (58, 61, 61), False
            if 10 < z < 11 and any(abs(y - slot) < .4 for slot in (4.8, 6.9, 9.1, 11.2)):
                return "front", (234, 220, 172), False
            return "front", (15, 18, 20), False
        return "front", base.sample(tex["front"], (15 - y) / 14, (17 - z) / 17), False
    return base.paint(solid, p, n, tex)


def main():
    tex, solids, views = textures(), model(), []
    for turns, facing in enumerate(base.FACINGS):
        flat, layers = base.render(solids, tex, turns, painter=paint, width=WIDTH, height=HEIGHT)
        base.save_view(facing, flat, layers, prefix="cabinet_sunsetriders")
        views.append(flat)
    shell.preview(views, title="SUNSET RIDERS / REFERENCE PASS",
                  subtitle="48 x 56 px | four players / double-width deck / sunset decals / western placeholder screen",
                  filename="sunsetriders.png")


if __name__ == "__main__":
    main()
