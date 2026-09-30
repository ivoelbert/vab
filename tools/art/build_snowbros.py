#!/usr/bin/env python3
"""Render four Snow Bros. cabinets from the decal sheet and assembled reference.

Run: python3 tools/art/build_snowbros.py (requires Pillow).
The shared MKII raycaster provides projection, lighting, and editor layer export;
this cabinet has its own artwork, white molding, sloping monitor, and controls.
"""
from PIL import Image, ImageDraw
import build_mk2 as base

ROOT = base.ROOT
REF = ROOT / "art/references/snowbros"
CYAN = (8, 179, 213)
TRIM = (232, 241, 239)
BLACK = (20, 24, 28)


def side(sheet, box, polygon):
    # The sheet includes the cut-out silhouette. Extend its cyan background
    # outside the decal, so UV sampling never imports white page margins.
    mask = Image.new("L", sheet.size)
    ImageDraw.Draw(mask).polygon(polygon, fill=255)
    image = Image.new("RGB", sheet.size, CYAN)
    image.paste(sheet, mask=mask)
    return image.crop(box)


def textures():
    sheet = Image.open(REF / "decals.png").convert("RGB")
    cabinet = Image.open(REF / "cabinet.png").convert("RGB")
    left = side(sheet, (28, 132, 343, 1012),
                ((28, 203), (215, 132), (335, 132), (335, 256),
                 (205, 325), (234, 510), (343, 562), (298, 615), (298, 1012), (28, 1012)))
    right = side(sheet, (710, 132, 1018, 1012),
                 ((710, 132), (822, 132), (1018, 203), (1018, 1012),
                  (736, 1012), (736, 619), (710, 562), (815, 511), (835, 325), (710, 256)))
    # The right decal's rear is on the right in the sheet. Reverse its depth UV,
    # not the opposite snowman's color: Nick stays blue, Tom stays red.
    right = right.transpose(Image.Transpose.FLIP_LEFT_RIGHT)
    # Monitor pixels come from the assembled machine; no invented gameplay.
    screen = cabinet.transform((64, 64), Image.Transform.QUAD,
                               (282, 539, 322, 713, 530, 676, 467, 540),
                               Image.Resampling.BICUBIC)
    images = {
        "left": left, "right": right,
        "marquee": sheet.crop((345, 25, 701, 143)),
        "bezel": sheet.crop((374, 158, 680, 412)),
        "controls": sheet.crop((360, 422, 692, 555)),
        "panel": sheet.crop((374, 566, 675, 613)),
        "screen": screen,
    }
    sizes = {"left": (18, 48), "right": (18, 48), "marquee": (16, 6),
             "bezel": (16, 16), "controls": (16, 9), "panel": (16, 4), "screen": (12, 10)}
    return {name: image.resize(sizes[name], Image.Resampling.BOX) for name, image in images.items()}


def model():
    solids = [solid for solid in base.model() if solid.part not in ("button", "stick", "upper")]
    # The photo has a low, strongly reclined screen under a deep black hood.
    solids.insert(1, base.Solid("upper", 2, 9, 4, 12, 16, 28, (((1, 0, .65), 20.75),)))
    for y in (10.5, 5.5):
        solids.append(base.Solid("stick", 10.4, 10.8, y - .2, y + .2, 17, 19.1))
        solids.append(base.Ball((10.6, y, 19.4), .7, (36, 205, 237)))
        # Snow Bros. uses two buttons per player: throw and jump, not MKII's six.
        for x, dy in ((11.3, -1.6), (12.1, -2.1)):
            z = (122 - x) / 6
            solids.append(base.Solid("button", x - .4, x + .4, y + dy - .4,
                                     y + dy + .4, z, z + .25, color=(44, 127, 232)))
    return solids


def paint(solid, p, n, tex):
    x, y, z = p
    nx, ny, nz = n
    part = solid.part
    if part == "side":
        if abs(ny) < .9 or x > base.side_front(z) - .65 or z > 31.35:
            return "trim", TRIM, False
        return "side art", base.sample(tex["left" if ny > 0 else "right"],
                                       (x - 2) / 9, (32 - z) / 32), False
    if part == "deck":
        if nz > .7:
            return "controls", base.sample(tex["controls"], (13 - y) / 10, (x - 8) / 6), False
        if nx > .9:
            if z > 17.65 or z < 15.8 or y < 3.5 or y > 12.5:
                return "trim", TRIM, False
            return "controls", base.sample(tex["panel"], (13 - y) / 10, (18 - z) / 2.5), False
        return "controls", BLACK, False
    if part == "hood" and nx > .9:
        if z > 31.4 or z < 28.6:
            return "trim", (45, 49, 50), False
        return "marquee", base.sample(tex["marquee"], (12 - y) / 8, (32 - z) / 4), True
    if part == "upper" and nx > .5:
        if 4.8 < y < 11.2 and 19.2 < z < 24.2:
            return "screen", base.sample(tex["screen"], (11.2 - y) / 6.4, (24.2 - z) / 5), True
        if z < 25.2:
            return "screen", base.sample(tex["bezel"], (12 - y) / 8, (25.2 - z) / 7), False
        return "screen", BLACK, False
    if part == "lower" and nx > .9:
        if 6 < y < 10 and 5.2 < z < 10.8:
            if y < 6.4 or y > 9.6 or z < 5.6 or z > 10.4:
                return "front", (58, 61, 61), False
            if 8.8 < z < 9.8 and (6.6 < y < 7.4 or 8.6 < y < 9.4):
                return "front", (234, 220, 172), False
            return "front", (15, 18, 20), False
        return "front", BLACK, False
    # Shared back panel/vents and physical control materials; no MKII colors.
    return base.paint(solid, p, n, tex)


def preview(views, *, title="SNOW BROS. / REFERENCE PASS",
            subtitle="32 x 48 px | cyan sides / white molding / blue and red snowmen",
            filename="snowbros.png"):
    width, height = views[0].size
    column = width * 6 + 26
    native_y = 111 + height * 6 + 28
    sheet = Image.new("RGB", (24 + column * 4, native_y + height + 26), (22, 26, 37))
    draw = ImageDraw.Draw(sheet)
    draw.text((24, 18), title, fill=(233, 237, 247))
    draw.text((24, 40), subtitle, fill=(151, 164, 184))
    for i, (facing, image) in enumerate(zip(base.FACINGS, views)):
        x = 24 + i * column
        draw.text((x, 82), facing.replace("_", " ").upper(), fill=(207, 216, 233))
        enlarged = image.resize((width * 6, height * 6), Image.Resampling.NEAREST)
        sheet.paste(enlarged, (x, 111), enlarged)
        sheet.paste(image, (x + (width * 6 - width) // 2, native_y), image)
    draw.text((24, native_y + 20), "NATIVE", fill=(151, 164, 184))
    path = ROOT / "art/previews" / filename
    path.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(path)
    print(path.relative_to(ROOT))


def main():
    tex, solids, views = textures(), model(), []
    for turns, facing in enumerate(base.FACINGS):
        flat, layers = base.render(solids, tex, turns, painter=paint)
        base.save_view(facing, flat, layers, prefix="cabinet_snowbros")
        views.append(flat)
    preview(views)


if __name__ == "__main__":
    main()
