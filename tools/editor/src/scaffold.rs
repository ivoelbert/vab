//! Arcade cabinets: a small 3D model of an upright cabinet, dressed in a skin and rendered in
//! its four facings, so the views agree and each part lands on its own layer.
//!
//! The model is a few convex solids in cell units: a cell is 16 units on a side and its
//! diamond 32×16 pixels, so a unit is a pixel across and a pixel up. It faces +x (down and
//! right on screen) and turns a quarter at a time for the other views.
//!
//! A skin is a folder art/cabinets/<name>/ of small PNGs, one texel per unit, each as seen
//! from outside the cabinet. A missing one falls back to the plain skin's.
//! - side.png, 12×30: a side panel, back on the left. The other side shows it mirrored, as on
//!   real cabinets, and its border texels color the panels' edges (the T-molding).
//! - marquee.png, 8×4, and screen.png, 8×8: lit, so never shaded.
//! - panel.png, 10×2: the control panel's front.
//! - controls.png, 10×6: the control panel's top, far edge up. The joysticks stand on
//!   texels (2, 2) and (7, 2).
//! - front.png, 8×15: below the control panel, where the coin door goes.
//! - colors.png, 2×1: the body color, then the joystick balls'.

use std::error::Error;
use std::fs;
use std::path::Path;

use bevy::math::Vec3;

use crate::ART;
use crate::draw::{CLEAR, Rgba, read_png};

const CELL: f32 = 16.0;
pub const WIDTH: u32 = 32;
pub const HEIGHT: u32 = 48;

/// Views by the direction the screen faces on screen, each a quarter turn from the last.
const FACINGS: [&str; 4] = ["down_right", "down_left", "up_left", "up_right"];

// The model: depth along x from the back, width along y between the side panels, height z.
const BACK: f32 = 2.0;
const Y_MIN: f32 = 3.0;
const Y_MAX: f32 = 13.0;
const TOP: f32 = 30.0;
const SIDE_PANEL: f32 = 1.0;
const LOWER_FRONT: f32 = 11.0;
const LOWER_TOP: f32 = 15.0;
const DECK_BACK: f32 = 8.0;
const DECK_FRONT: f32 = 14.0;
const DECK_BOTTOM: f32 = 14.0;
const DECK_FRONT_TOP: f32 = 16.0;
const SCREEN_BOTTOM: f32 = 17.0;
const SCREEN_TOP: f32 = 25.0;
const MARQUEE_FRONT: f32 = 10.5;
const MARQUEE_BOTTOM: f32 = 26.0;
const STICK_X: f32 = 10.5;
const STICKS_Y: [f32; 2] = [10.5, 5.5];

const OUTLINE: Rgba = [20, 18, 28, 255];
const STICK: Rgba = [30, 30, 38, 255];
// The plain skin.
const BODY: Rgba = [43, 43, 54, 255];
const BALL: Rgba = [230, 60, 60, 255];
const PANEL: Rgba = [66, 66, 82, 255];
const TRIM: Rgba = [224, 176, 48, 255];
const MARQUEE: Rgba = [255, 214, 92, 255];
const BEZEL: Rgba = [24, 24, 30, 255];
const SCREEN: Rgba = [63, 182, 255, 255];
const GLARE: Rgba = [160, 222, 255, 255];
const BUTTONS: [Rgba; 4] = [
    [230, 60, 60, 255],
    [70, 130, 255, 255],
    [250, 210, 60, 255],
    [235, 235, 235, 255],
];
const COIN_LIGHT: Rgba = [255, 120, 40, 255];

/// Layers, bottom first.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Body,
    SideArt,
    Front,
    Controls,
    Screen,
    Marquee,
    Outline,
}

const PARTS: [(Part, &str); 7] = [
    (Part::Body, "body"),
    (Part::SideArt, "side art"),
    (Part::Front, "front"),
    (Part::Controls, "controls"),
    (Part::Screen, "screen"),
    (Part::Marquee, "marquee"),
    (Part::Outline, "outline"),
];

#[derive(Clone, Copy)]
enum Texture {
    Side,
    Marquee,
    Screen,
    Panel,
    Controls,
    Front,
    Colors,
}

/// Each texture's file name and size, in `Texture` order.
const TEXTURES: [(&str, usize, usize); 7] = [
    ("side", 12, 30),
    ("marquee", 8, 4),
    ("screen", 8, 8),
    ("panel", 10, 2),
    ("controls", 10, 6),
    ("front", 8, 15),
    ("colors", 2, 1),
];

/// One view: its tile name and its layers, bottom first.
pub struct Sketch {
    pub tile: String,
    pub layers: Vec<(&'static str, Vec<Rgba>)>,
}

/// The four views of a cabinet in `skin`, or in the plain skin, named
/// objects/cabinet_<skin>_<facing> or objects/cabinet_<facing>.
pub fn views(skin: Option<&str>) -> Result<Vec<Sketch>, Box<dyn Error>> {
    let dressing = match skin {
        Some(name) => Skin::load(name)?,
        None => Skin::plain(),
    };
    let solids = model();
    Ok(FACINGS
        .iter()
        .enumerate()
        .map(|(turns, facing)| Sketch {
            tile: match skin {
                Some(name) => format!("objects/cabinet_{name}_{facing}"),
                None => format!("objects/cabinet_{facing}"),
            },
            layers: render(&solids, &dressing, turns),
        })
        .collect())
}

/// The skins in art/cabinets.
pub fn skins() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(format!("{ART}/cabinets"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Draw mode's names for the skin textures on disk: cabinets/<skin>/<texture>.
pub fn texture_names() -> Vec<String> {
    skins()
        .iter()
        .flat_map(|skin| {
            TEXTURES.iter().filter_map(move |(file, _, _)| {
                let path = format!("{ART}/cabinets/{skin}/{file}.png");
                Path::new(&path)
                    .exists()
                    .then(|| format!("cabinets/{skin}/{file}"))
            })
        })
        .collect()
}

/// The plain skin's textures, to start a new skin from: file name, width, height, texels.
pub fn plain_textures() -> Vec<(&'static str, u32, u32, Vec<Rgba>)> {
    TEXTURES
        .iter()
        .zip(Skin::plain().textures)
        .map(|((file, width, height), texels)| {
            (*file, *width as u32, *height as u32, texels.pixels)
        })
        .collect()
}

struct Texels {
    width: usize,
    height: usize,
    pixels: Vec<Rgba>,
}

impl Texels {
    fn new(width: usize, height: usize, texel: impl Fn(usize, usize) -> Rgba) -> Self {
        let pixels = (0..width * height)
            .map(|i| texel(i % width, i / width))
            .collect();
        Self {
            width,
            height,
            pixels,
        }
    }

    /// The texel at (column, row), clamped to the edges; `None` where it is transparent.
    fn at(&self, column: f32, row: f32) -> Option<Rgba> {
        let column = (column.floor().max(0.0) as usize).min(self.width - 1);
        let row = (row.floor().max(0.0) as usize).min(self.height - 1);
        let texel = self.pixels[row * self.width + column];
        (texel[3] > 0).then_some(texel)
    }
}

struct Skin {
    /// In `Texture` order.
    textures: Vec<Texels>,
}

impl Skin {
    fn plain() -> Self {
        let textures = vec![
            // A stripe running down the side toward the player.
            Texels::new(12, 30, |column, row| {
                let x = BACK + column as f32 + 0.5;
                let z = TOP - row as f32 - 0.5;
                if (17.0..19.5).contains(&(z + 0.8 * x)) {
                    TRIM
                } else {
                    BODY
                }
            }),
            Texels::new(8, 4, |_, _| MARQUEE),
            Texels::new(8, 8, |column, row| {
                if column == 0 || row == 0 || column == 7 || row == 7 {
                    BEZEL
                } else if column + row == 5 && row < 4 {
                    GLARE
                } else {
                    SCREEN
                }
            }),
            Texels::new(10, 2, |_, _| PANEL),
            // Four buttons beside each joystick.
            Texels::new(10, 6, |column, row| {
                let button = (2..4).contains(&row) && matches!(column % 5, 3 | 4);
                if button {
                    BUTTONS[(row - 2) * 2 + column % 5 - 3]
                } else {
                    PANEL
                }
            }),
            // A coin door with two lit slots.
            Texels::new(8, 15, |column, row| {
                if row == 6 && (3..5).contains(&column) {
                    COIN_LIGHT
                } else if (5..12).contains(&row) && (2..6).contains(&column) {
                    PANEL
                } else {
                    BODY
                }
            }),
            Texels::new(2, 1, |column, _| [BODY, BALL][column]),
        ];
        Self { textures }
    }

    fn load(name: &str) -> Result<Self, Box<dyn Error>> {
        let mut skin = Self::plain();
        for ((file, width, height), texels) in TEXTURES.iter().zip(&mut skin.textures) {
            let path = format!("{ART}/cabinets/{name}/{file}.png");
            if !Path::new(&path).exists() {
                continue;
            }
            let (found_width, found_height, pixels) = read_png(&path)?;
            if (found_width as usize, found_height as usize) != (*width, *height) {
                return Err(format!(
                    "{path} is {found_width}×{found_height}, not {width}×{height}"
                )
                .into());
            }
            texels.pixels = pixels;
        }
        Ok(skin)
    }

    fn texture(&self, texture: Texture) -> &Texels {
        &self.textures[texture as usize]
    }

    fn body(&self) -> Rgba {
        self.texture(Texture::Colors).at(0.0, 0.0).unwrap_or(BODY)
    }

    fn ball(&self) -> Rgba {
        self.texture(Texture::Colors).at(1.0, 0.0).unwrap_or(BALL)
    }

    /// How the model looks at `point` on a face of `piece` facing `normal`.
    fn paint(&self, piece: Piece, point: Vec3, normal: Vec3) -> Paint {
        let body = Paint::new(Part::Body, self.body());
        match piece {
            Piece::Stick => return Paint::new(Part::Controls, STICK),
            Piece::Ball => return Paint::new(Part::Controls, self.ball()),
            _ => {}
        }
        // The side panels, and their edges wherever they show; the control panel spans them.
        let in_side_panel =
            piece != Piece::Deck && (point.y < Y_MIN + SIDE_PANEL || point.y > Y_MAX - SIDE_PANEL);
        if normal.y.abs() > 0.9 || in_side_panel {
            // Half a unit in, so edges take the panel's border texels.
            let inside = point - Vec3::new(normal.x, 0.0, normal.z) * 0.5;
            let side = self.texture(Texture::Side);
            return side
                .at(inside.x - BACK, TOP - inside.z)
                .map_or(body, |color| Paint::new(Part::SideArt, color));
        }
        // Across the front between the side panels, left to right as seen from the front.
        let across = Y_MAX - SIDE_PANEL - point.y;
        let (texture, column, row, part) = match piece {
            Piece::Lower if normal.x > 0.9 => {
                (Texture::Front, across, LOWER_TOP - point.z, Part::Front)
            }
            Piece::Deck if normal.x > 0.9 => (
                Texture::Panel,
                Y_MAX - point.y,
                DECK_FRONT_TOP - point.z,
                Part::Controls,
            ),
            Piece::Deck if normal.z > 0.9 => (
                Texture::Controls,
                Y_MAX - point.y,
                point.x - DECK_BACK,
                Part::Controls,
            ),
            Piece::Upper if normal.x > 0.5 => {
                (Texture::Screen, across, SCREEN_TOP - point.z, Part::Screen)
            }
            Piece::Marquee if normal.x > 0.9 => {
                (Texture::Marquee, across, TOP - point.z, Part::Marquee)
            }
            Piece::Upper if normal.x < -0.9 => {
                let vent = (5.0..11.0).contains(&point.y)
                    && (19.0..25.0).contains(&point.z)
                    && point.z as i32 % 2 == 0;
                let color = if vent {
                    scale(self.body(), 0.6)
                } else {
                    self.body()
                };
                return Paint::new(Part::Body, color);
            }
            _ => return body,
        };
        let lit = matches!(part, Part::Screen | Part::Marquee);
        match self.texture(texture).at(column, row) {
            Some(color) => Paint { part, color, lit },
            None => body,
        }
    }
}

/// How a surface looks: the layer it goes on, its color, and whether it glows (skips shading).
struct Paint {
    part: Part,
    color: Rgba,
    lit: bool,
}

impl Paint {
    fn new(part: Part, color: Rgba) -> Self {
        Self {
            part,
            color,
            lit: false,
        }
    }
}

/// The points with `normal · p <= offset`.
struct Plane {
    normal: Vec3,
    offset: f32,
}

impl Plane {
    fn through(normal: Vec3, point: Vec3) -> Self {
        let normal = normal.normalize();
        Self {
            normal,
            offset: normal.dot(point),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Piece {
    Lower,
    /// The control panel.
    Deck,
    /// The screen's housing.
    Upper,
    Marquee,
    Stick,
    Ball,
}

struct Solid {
    piece: Piece,
    /// Convex: the points inside all of them.
    planes: Vec<Plane>,
}

fn cuboid(min: Vec3, max: Vec3) -> Vec<Plane> {
    vec![
        Plane::through(Vec3::X, max),
        Plane::through(Vec3::Y, max),
        Plane::through(Vec3::Z, max),
        Plane::through(Vec3::NEG_X, min),
        Plane::through(Vec3::NEG_Y, min),
        Plane::through(Vec3::NEG_Z, min),
    ]
}

fn model() -> Vec<Solid> {
    let mut deck = cuboid(
        Vec3::new(DECK_BACK, Y_MIN, DECK_BOTTOM),
        Vec3::new(DECK_FRONT, Y_MAX, SCREEN_BOTTOM),
    );
    // Sloping down toward the player.
    deck.push(Plane::through(
        Vec3::new(1.0, 0.0, 6.0),
        Vec3::new(DECK_FRONT, 0.0, DECK_FRONT_TOP),
    ));
    let mut upper = cuboid(
        Vec3::new(BACK, Y_MIN, LOWER_TOP),
        Vec3::new(10.0, Y_MAX, MARQUEE_BOTTOM),
    );
    // The screen leans back.
    upper.push(Plane::through(
        Vec3::new(8.0, 0.0, 1.0),
        Vec3::new(9.5, 0.0, SCREEN_BOTTOM),
    ));
    let mut marquee = cuboid(
        Vec3::new(BACK, Y_MIN, 24.0),
        Vec3::new(MARQUEE_FRONT, Y_MAX, TOP),
    );
    // The speaker panel under the marquee faces down, meeting the top of the screen.
    marquee.push(Plane::through(
        Vec3::new(1.0, 0.0, -2.0),
        Vec3::new(MARQUEE_FRONT, 0.0, MARQUEE_BOTTOM),
    ));
    let mut solids = vec![
        Solid {
            piece: Piece::Lower,
            planes: cuboid(
                Vec3::new(BACK, Y_MIN, 0.0),
                Vec3::new(LOWER_FRONT, Y_MAX, LOWER_TOP),
            ),
        },
        Solid {
            piece: Piece::Deck,
            planes: deck,
        },
        Solid {
            piece: Piece::Upper,
            planes: upper,
        },
        Solid {
            piece: Piece::Marquee,
            planes: marquee,
        },
    ];
    for y in STICKS_Y {
        solids.push(Solid {
            piece: Piece::Stick,
            planes: cuboid(
                Vec3::new(STICK_X - 0.5, y - 0.5, LOWER_TOP),
                Vec3::new(STICK_X + 0.5, y + 0.5, 18.5),
            ),
        });
        solids.push(Solid {
            piece: Piece::Ball,
            planes: cuboid(
                Vec3::new(STICK_X - 0.75, y - 0.75, 18.5),
                Vec3::new(STICK_X + 0.75, y + 0.75, 20.0),
            ),
        });
    }
    solids
}

/// Turns `v` a quarter turn counterclockwise about the cell's center `turns` times, exactly.
fn turn(v: Vec3, turns: usize) -> Vec3 {
    let center = CELL / 2.0;
    let (mut x, mut y) = (v.x - center, v.y - center);
    for _ in 0..turns % 4 {
        (x, y) = (-y, x);
    }
    Vec3::new(x + center, y + center, v.z)
}

/// Turns a direction, which has no position to turn about.
fn turn_direction(v: Vec3, turns: usize) -> Vec3 {
    let center = Vec3::new(CELL / 2.0, CELL / 2.0, 0.0);
    turn(v + center, turns) - center
}

/// Where the ray from `origin` toward `toward` last leaves `solid`, which is where the viewer
/// sees it, as distance along the ray and the plane there.
fn hit(solid: &Solid, origin: Vec3, toward: Vec3) -> Option<(f32, &Plane)> {
    let (mut enter, mut leave, mut face) = (f32::NEG_INFINITY, f32::INFINITY, None);
    for plane in &solid.planes {
        let along = plane.normal.dot(toward);
        let room = plane.offset - plane.normal.dot(origin);
        if along > 0.0 {
            if room / along < leave {
                leave = room / along;
                face = Some(plane);
            }
        } else if along < 0.0 {
            enter = enter.max(room / along);
        } else if room < 0.0 {
            return None;
        }
    }
    (enter <= leave).then_some((leave, face?))
}

fn scale(color: Rgba, by: f32) -> Rgba {
    let channel = |c: u8| (c as f32 * by).round() as u8;
    [
        channel(color[0]),
        channel(color[1]),
        channel(color[2]),
        color[3],
    ]
}

/// Shades `color` like the other objects: tops brightest, faces toward +y (left on screen)
/// darker, faces toward +x darker still.
fn shade(color: Rgba, normal: Vec3) -> Rgba {
    let weights = normal.max(Vec3::ZERO);
    scale(
        color,
        weights.dot(Vec3::new(0.63, 0.81, 1.0)) / weights.element_sum(),
    )
}

fn render(solids: &[Solid], skin: &Skin, turns: usize) -> Vec<(&'static str, Vec<Rgba>)> {
    let (width, height) = (WIDTH as usize, HEIGHT as usize);
    let mut layers: Vec<Vec<Rgba>> = PARTS.iter().map(|_| vec![CLEAR; width * height]).collect();
    let mut covered = vec![false; width * height];
    // The model turned `turns` times, seen from the fixed camera, is the model seen from a
    // camera turned the other way.
    let back = 4 - turns % 4;
    let toward_viewer = turn_direction(Vec3::ONE, back);
    for row in 0..height {
        for column in 0..width {
            // Where this pixel's ray meets the floor: x - y across, (x + y) / 2 down.
            let across = column as f32 + 0.5 - WIDTH as f32 / 2.0;
            let down = row as f32 + 0.5 - (HEIGHT as f32 - CELL);
            let floor = Vec3::new(down + across / 2.0, down - across / 2.0, 0.0);
            let origin = turn(floor, back);
            let nearest = solids
                .iter()
                .filter_map(|solid| hit(solid, origin, toward_viewer).map(|hit| (solid, hit)))
                .max_by(|(_, (a, _)), (_, (b, _))| a.total_cmp(b));
            let Some((solid, (distance, plane))) = nearest else {
                continue;
            };
            let point = origin + toward_viewer * distance;
            let paint = skin.paint(solid.piece, point, plane.normal);
            let color = if paint.lit {
                paint.color
            } else {
                shade(paint.color, turn_direction(plane.normal, turns))
            };
            let layer = PARTS
                .iter()
                .position(|(part, _)| *part == paint.part)
                .unwrap();
            layers[layer][row * width + column] = color;
            covered[row * width + column] = true;
        }
    }
    // Outline where the silhouette meets empty space above or below, like the other objects.
    let outline = PARTS
        .iter()
        .position(|(part, _)| *part == Part::Outline)
        .unwrap();
    for row in 0..height {
        for column in 0..width {
            let empty = |row: Option<usize>| {
                row.filter(|row| *row < height)
                    .is_none_or(|row| !covered[row * width + column])
            };
            if covered[row * width + column] && (empty(row.checked_sub(1)) || empty(Some(row + 1)))
            {
                layers[outline][row * width + column] = OUTLINE;
            }
        }
    }
    PARTS
        .iter()
        .zip(layers)
        .map(|((_, name), pixels)| (*name, pixels))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_front_views_show_the_screen() {
        let shows_screen: Vec<bool> = views(None)
            .unwrap()
            .iter()
            .map(|sketch| {
                let (_, screen) = sketch.layers.iter().find(|(n, _)| *n == "screen").unwrap();
                screen.iter().any(|p| p[3] > 0)
            })
            .collect();
        assert_eq!(shows_screen, [true, true, false, false]);
    }

    #[test]
    fn plain_textures_have_their_sizes() {
        for ((file, width, height), (_, w, h, pixels)) in TEXTURES.iter().zip(plain_textures()) {
            assert_eq!((w as usize, h as usize), (*width, *height), "{file}");
            assert_eq!(pixels.len(), width * height, "{file}");
        }
    }
}
