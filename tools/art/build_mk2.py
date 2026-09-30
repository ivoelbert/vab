#!/usr/bin/env python3
"""Build a photo-referenced MKII cabinet, without replacing the original assets.

Run from anywhere: python3 tools/art/build_mk2.py (requires Pillow).
One model, four orthographic 2:1 views, native 32x48 pixels. Source photographs
stay in art/references/mk2; editable render layers use the editor's RON format.
"""
from pathlib import Path
import math
from PIL import Image, ImageDraw, ImageEnhance

ROOT = Path(__file__).resolve().parents[2]
REF = ROOT / "art/references/mk2"
WIDTH, HEIGHT = 32, 48
FACINGS = ("down_right", "down_left", "up_left", "up_right")
LAYERS = ("body", "side art", "front", "controls", "screen", "marquee", "trim")
BLACK = (17, 19, 27)
RED = (232, 36, 48)


def rectify(name, corners, size):
    """Corners clockwise from top left, sampled into an outside-facing texture."""
    image = Image.open(REF / f"{name}.png").convert("RGB")
    a, b, c, d = corners
    image = image.transform(size, Image.Transform.QUAD, (*a, *d, *c, *b), Image.Resampling.BICUBIC)
    image = ImageEnhance.Color(image).enhance(1.25)
    return ImageEnhance.Contrast(image).enhance(1.12)


def textures():
    # Separate side photos: no mirrored lettering on the opposite side.
    left = rectify("left", ((104, 91), (288, 0), (399, 1405), (123, 1272)), (96, 256))
    right = rectify("right", ((706, 139), (598, 36), (506, 1439), (694, 1330)), (96, 256))
    # The right-hand photo is dimmer; normalize its print, not the cabinet lighting.
    right = ImageEnhance.Brightness(right).enhance(1.15)
    images = {
        "left": left,
        "right": right,
        "marquee": rectify("front", ((62, 3), (540, 3), (537, 124), (63, 125)), (96, 32)),
        "screen": rectify("left", ((287, 258), (494, 251), (535, 462), (328, 480)), (64, 64)),
        "controls": rectify("front", ((115, 531), (499, 531), (532, 662), (43, 659)), (96, 48)),
        "panel": rectify("front", ((52, 681), (535, 691), (532, 746), (55, 741)), (96, 24)),
        "front": rectify("front", ((87, 750), (507, 755), (481, 1186), (145, 1186)), (64, 128)),
    }
    # Area filtering before point sampling avoids isolated photographic noise.
    sizes = {"left": (18, 48), "right": (18, 48), "marquee": (16, 6),
             "screen": (10, 10), "controls": (16, 9), "panel": (16, 4), "front": (12, 24)}
    return {name: image.resize(sizes[name], Image.Resampling.BOX) for name, image in images.items()}


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def add(a, b):
    return tuple(x + y for x, y in zip(a, b))


def mul(v, s):
    return tuple(x * s for x in v)


def unit(v):
    return mul(v, 1 / math.sqrt(dot(v, v)))


def rotate(v, turns, point=False):
    x, y, z = v
    if point:
        x, y = x - 8, y - 8
    for _ in range(turns % 4):
        x, y = -y, x
    return (x + 8, y + 8, z) if point else (x, y, z)


class Solid:
    def __init__(self, part, xmin, xmax, ymin, ymax, zmin, zmax, extra=(), color=None):
        self.part, self.color = part, color
        self.planes = [((1, 0, 0), xmax), ((-1, 0, 0), -xmin),
                       ((0, 1, 0), ymax), ((0, -1, 0), -ymin),
                       ((0, 0, 1), zmax), ((0, 0, -1), -zmin)] + list(extra)

    def hit(self, origin, ray):
        enter, leave, normal = -math.inf, math.inf, None
        for n, offset in self.planes:
            along, room = dot(n, ray), offset - dot(n, origin)
            if along > 1e-9:
                t = room / along
                if t < leave:
                    leave, normal = t, n
            elif along < -1e-9:
                enter = max(enter, room / along)
            elif room < 0:
                return None
        if enter <= leave and normal is not None:
            return leave, unit(normal)
        return None


class Ball:
    def __init__(self, center, radius, color):
        self.center, self.radius, self.color = center, radius, color
        self.part = "button"

    def hit(self, origin, ray):
        offset = add(origin, mul(self.center, -1))
        a, b = dot(ray, ray), 2 * dot(offset, ray)
        c = dot(offset, offset) - self.radius ** 2
        disc = b * b - 4 * a * c
        if disc < 0:
            return None
        t = (-b + math.sqrt(disc)) / (2 * a)
        return t, unit(add(add(origin, mul(ray, t)), mul(self.center, -1)))


def model():
    # Recessed monitor between continuous shaped side panels. The deck projects
    # beyond the lower body; the speaker hood projects beyond the screen.
    solids = [
        Solid("lower", 2, 11, 3, 13, 0, 16),
        Solid("upper", 2, 9, 4, 12, 16, 28, (((8, 0, 1), 88),)),
        Solid("hood", 2, 10.5, 4, 12, 26, 32, (((1, 0, -2), -45.5),)),
        Solid("deck", 8, 14, 3, 13, 15.5, 18.5, (((1, 0, 6), 122),)),
    ]
    for ymin, ymax in ((3, 4), (12, 13)):
        solids += [
            Solid("side", 2, 11, ymin, ymax, 0, 16),
            Solid("side", 2, 9.5, ymin, ymax, 16, 27, (((8, 0, 1), 95),)),
            Solid("side", 2, 10.5, ymin, ymax, 27, 32, (((1, 0, -2), -45.5),)),
        ]
    for y in (10.5, 5.5):
        solids.append(Solid("stick", 10.4, 10.8, y - .2, y + .2, 17, 19.1))
        solids.append(Ball((10.6, y, 19.4), .7, RED))
        # Six buttons per player, like the reference: red / white / blue,
        # with the yellow run button toward the near edge.
        for x, dy, color in ((11, -1.4, RED), (11, -2.3, (58, 118, 230)),
                             (12, -1.0, RED), (12, -1.9, (225, 222, 205)),
                             (12, -2.8, (58, 118, 230)), (13, -.8, (255, 212, 48))):
            z = (122 - x) / 6
            solids.append(Solid("button", x - .35, x + .35, y + dy - .35,
                                y + dy + .35, z, z + .25, color=color))
    return solids


def sample(image, u, v):
    x = max(0, min(image.width - 1, int(u * image.width)))
    y = max(0, min(image.height - 1, int(v * image.height)))
    return image.getpixel((x, y))


def side_front(z):
    if z < 16:
        return 11
    if z < 27:
        return min(9.5, (95 - z) / 8)
    return min(10.5, 2 * z - 45.5)


def paint(solid, p, n, tex):
    x, y, z = p
    nx, ny, nz = n
    part = solid.part
    if part == "button":
        return "controls", solid.color, False
    if part == "stick":
        return "controls", (57, 57, 64), False
    if part == "side":
        # T-molding follows the actual stepped outline, not a rectangular UV border.
        if abs(ny) < .9 or x > side_front(z) - .65 or z > 31.35:
            return "trim", RED, False
        image = tex["left" if ny > 0 else "right"]
        return "side art", sample(image, (x - 2) / 9, (32 - z) / 32), False
    if part == "deck":
        if nz > .7:
            return "controls", sample(tex["controls"], (13 - y) / 10, (x - 8) / 6), False
        if nx > .9:
            if z > 17.55 or z < 15.9 or y < 3.6 or y > 12.4:
                return "trim", RED, False
            return "controls", sample(tex["panel"], (13 - y) / 10, (18 - z) / 2.5), False
        return "controls", BLACK, False
    if part == "hood" and nx > .9:
        if z > 31.4 or z < 28.6:
            return "trim", RED, False
        return "marquee", sample(tex["marquee"], (12 - y) / 8, (32 - z) / 4), True
    if part == "upper" and nx > .5:
        if 4.65 < y < 11.35 and 19.4 < z < 26.35:
            return "screen", sample(tex["screen"], (11.35 - y) / 6.7, (26.35 - z) / 6.95), True
        return "screen", (12, 14, 20), False
    if part == "lower" and nx > .9:
        # Keep the print subdued; pick out a readable coin door at native resolution.
        if 6 < y < 10 and 5.2 < z < 10.8:
            border = y < 6.4 or y > 9.6 or z < 5.6 or z > 10.4
            if border:
                return "front", (48, 47, 54), False
            if 8.8 < z < 9.8 and (6.6 < y < 7.4 or 8.6 < y < 9.4):
                return "front", (174, 43, 36), False
            return "front", (23, 24, 30), False
        color = sample(tex["front"], (12 - y) / 8, (16 - z) / 16)
        # Reflections in the photo should not look like a brown wooden cabinet.
        gray = min(42, int(sum(color) / 3))
        if z < 1.5:
            return "front", (gray, gray, gray + 3), False
        return "front", (max(18, gray // 2), max(20, gray // 2), max(26, gray // 2 + 5)), False
    if nx < -.9:
        # Rear service hatch and paired ventilation bands, no invented side artwork.
        if 5 < y < 11 and 22 < z < 28 and int(z) % 2 == 0:
            return "body", (9, 11, 16), False
        if 5 < y < 11 and 4 < z < 15:
            edge = y < 5.35 or y > 10.65 or z < 4.35 or z > 14.65
            return "body", (42, 44, 53) if edge else (24, 27, 35), False
    return "body", (30, 33, 43), False


def render(solids, tex, turns, painter=paint, *, width=WIDTH, height=HEIGHT):
    layers = {name: Image.new("RGBA", (width, height)) for name in LAYERS}
    ray = rotate((1, 1, 1), -turns)
    for row in range(height):
        for col in range(width):
            across, down = col + .5 - width / 2, row + .5 - (height - 16)
            origin = rotate((down + across / 2, down - across / 2, 0), -turns, True)
            nearest = None
            for solid in solids:
                hit = solid.hit(origin, ray)
                if hit and (nearest is None or hit[0] >= nearest[0]):
                    nearest = hit[0], hit[1], solid
            if nearest is None:
                continue
            distance, normal, solid = nearest
            p = add(origin, mul(ray, distance))
            layer, color, lit = painter(solid, p, normal, tex)
            if not lit:
                normal = rotate(normal, turns)
                # Fixed upper-left lighting shared by every orientation.
                weights = [max(0, c) for c in normal]
                light = dot(weights, (.72, .88, 1.0)) / max(.001, sum(weights))
                color = tuple(round(c * light) for c in color)
            # A small color ramp prevents photographic speckle at sprite scale.
            color = tuple(min(255, round(c / 8) * 8) for c in color)
            layers[layer].putpixel((col, row), (*color, 255))
    flat = Image.new("RGBA", (width, height))
    for layer in layers.values():
        flat.alpha_composite(layer)
    return flat, layers


def save_view(facing, flat, layers, prefix="cabinet_mk2_v2"):
    name = f"{prefix}_{facing}"
    output = ROOT / "assets/tiles/objects" / f"{name}.png"
    output.parent.mkdir(parents=True, exist_ok=True)
    flat.save(output)
    source = ROOT / "art/objects" / name
    source.mkdir(parents=True, exist_ok=True)
    for i, layer in enumerate(layers.values()):
        layer.save(source / f"{i}.png")
    entries = "".join(f'        (name: "{n}", visible: true),\n' for n in LAYERS)
    (source / "layers.ron").write_text(
        f"(\n    width: {flat.width},\n    height: {flat.height},\n    layers: [\n{entries}    ],\n)\n")


def preview(views):
    # Honest nearest-neighbor zoom, plus a native-size row and same-scale comparison.
    sheet = Image.new("RGB", (896, 810), (22, 26, 37))
    draw = ImageDraw.Draw(sheet)
    draw.text((24, 18), "MORTAL KOMBAT II / REFERENCE PASS", fill=(233, 237, 247))
    draw.text((24, 40), "32 x 48 px | four rotations of one model | red trim / Raiden print", fill=(151, 164, 184))
    for i, (facing, image) in enumerate(zip(FACINGS, views)):
        x = 24 + i * 218
        draw.text((x, 82), facing.replace("_", " ").upper(), fill=(207, 216, 233))
        enlarged = image.resize((192, 288), Image.Resampling.NEAREST)
        sheet.paste(enlarged, (x, 111), enlarged)
        sheet.paste(image, (x + 80, 422), image)
        old = Image.open(REF / f"previous_{facing}.png").convert("RGBA")
        old = old.resize((192, 288), Image.Resampling.NEAREST)
        sheet.paste(old, (x, 492), old)
    draw.text((24, 448), "NATIVE", fill=(151, 164, 184))
    draw.text((24, 478), "EXISTING / SAME 6x SCALE", fill=(151, 164, 184))
    output = ROOT / "art/previews/mk2-v2.png"
    output.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(output)
    print(output.relative_to(ROOT))


def main():
    tex, solids = textures(), model()
    output = []
    for turns, facing in enumerate(FACINGS):
        flat, layers = render(solids, tex, turns)
        save_view(facing, flat, layers)
        output.append(flat)
    preview(output)


if __name__ == "__main__":
    main()
