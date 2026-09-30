//! Draw mode: pixel art for floor tiles, objects and cabinet skins, in layers.
//!
//! A drawing keeps its layers in art/<name>/: layers.ron names them, bottom first, and
//! <index>.png holds each one. Saving also writes the flattened image: a tile's to
//! assets/tiles/<name>.png, which the game loads and the map view reloads, and a cabinet skin
//! texture's (cabinets/<skin>/<texture>) to art/cabinets/<skin>/<texture>.png. art/ sits
//! outside assets/ so none of it ships with the web build.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;

use bevy::prelude::{IVec2, Resource};
use bevy_egui::egui::{
    self, Color32, DragValue, Key, PointerButton, Rect, RichText, Sense, Stroke, StrokeKind,
    TextEdit,
};
use serde::{Deserialize, Serialize};
use world::{Map, TILE_HEIGHT, TILE_WIDTH};

use crate::history::History;
use crate::{ART, ASSETS, scaffold, tiles_in};
/// Screen points per image pixel, at most.
const MAX_ZOOM: f32 = 64.0;
/// Zoom from which lines between pixels are drawn.
const GRID_ZOOM: f32 = 8.0;
/// Colors listed under the color picker, at most.
const PALETTE_SIZE: usize = 48;
const TOOLS: [(Tool, &str, Key); 4] = [
    (Tool::Pencil, "Pencil (B)", Key::B),
    (Tool::Eraser, "Eraser (E)", Key::E),
    (Tool::Fill, "Fill (G)", Key::G),
    (Tool::Picker, "Picker (I)", Key::I),
];

/// Unpremultiplied sRGB and alpha.
pub type Rgba = [u8; 4];
pub const CLEAR: Rgba = [0; 4];

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Pencil,
    Eraser,
    Fill,
    Picker,
}

#[derive(Resource)]
pub struct Studio {
    /// Tiles with a PNG in assets/tiles, and cabinet skin textures.
    tiles: Vec<String>,
    /// Drawings opened this session by tile name, kept so switching between them loses nothing.
    drawings: BTreeMap<String, Drawing>,
    current: Option<String>,
    tool: Tool,
    color: Rgba,
    /// Screen points per image pixel, rounded when drawn; `None` fits the drawing next frame.
    zoom: Option<f32>,
    pan: egui::Vec2,
    /// The active layer's name as typed; applied when the field loses focus.
    layer_name: String,
    new_folder: String,
    new_name: String,
    new_size: [u32; 2],
    /// Whether the delete button is waiting for confirmation.
    confirm_delete: bool,
    /// Cabinet skins in art/cabinets.
    skins: Vec<String>,
    /// The skin Make views renders; empty for the plain one.
    skin: String,
    new_skin: String,
    /// Whether Make views is waiting for confirmation to replace views.
    confirm_views: bool,
    status: String,
}

impl Default for Studio {
    fn default() -> Self {
        Self {
            tiles: Vec::new(),
            drawings: BTreeMap::new(),
            current: None,
            tool: Tool::Pencil,
            color: [255, 255, 255, 255],
            zoom: None,
            pan: egui::Vec2::ZERO,
            layer_name: String::new(),
            new_folder: "objects".into(),
            new_name: String::new(),
            new_size: default_size("objects"),
            confirm_delete: false,
            skins: Vec::new(),
            skin: String::new(),
            new_skin: String::new(),
            confirm_views: false,
            status: String::new(),
        }
    }
}

/// The size of a new drawing in a tiles folder; a floor tile fills one cell.
fn default_size(folder: &str) -> [u32; 2] {
    if folder == "floor" {
        [TILE_WIDTH as u32, TILE_HEIGHT as u32]
    } else {
        [32, 48]
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Where a drawing's flattened image goes.
fn flat_path(name: &str) -> String {
    if name.starts_with("cabinets/") {
        format!("{ART}/{name}.png")
    } else {
        format!("{ASSETS}/tiles/{name}.png")
    }
}

impl Studio {
    /// Called on switching to draw mode; opens `tile` if no drawing is open yet.
    pub fn enter(&mut self, tile: &str) {
        self.refresh();
        if self.current.is_none() && !tile.is_empty() {
            self.open(tile);
        }
    }

    fn open(&mut self, tile: &str) {
        if !self.drawings.contains_key(tile) {
            match Drawing::load(tile) {
                Ok(drawing) => {
                    self.drawings.insert(tile.to_owned(), drawing);
                }
                Err(error) => {
                    self.status = format!("Could not open {tile}: {error}");
                    return;
                }
            }
        }
        self.current = Some(tile.to_owned());
        self.zoom = None;
        self.pan = egui::Vec2::ZERO;
        self.confirm_delete = false;
        self.status.clear();
    }

    /// Rereads which tiles, skins and skin textures are on disk.
    fn refresh(&mut self) {
        self.tiles = tiles_in("floor");
        self.tiles.extend(tiles_in("objects"));
        self.tiles.extend(scaffold::texture_names());
        self.skins = scaffold::skins();
    }

    /// Removes `tile`'s PNG and layers from disk; returns whether it did.
    fn delete(&mut self, tile: &str) -> bool {
        self.confirm_delete = false;
        let removed = ignore_missing(fs::remove_file(flat_path(tile)))
            .and_then(|()| ignore_missing(fs::remove_dir_all(format!("{ART}/{tile}"))));
        if let Err(error) = removed {
            self.status = format!("Could not delete {tile}: {error}");
            return false;
        }
        self.drawings.remove(tile);
        self.current = None;
        self.refresh();
        self.status = format!("Deleted {tile}");
        true
    }

    fn create(&mut self) {
        let name = self.new_name.trim();
        if !valid_name(name) {
            self.status = "Name it with letters, digits, - or _".into();
            return;
        }
        let tile = format!("{}/{name}", self.new_folder);
        if self.tiles.contains(&tile) || self.drawings.contains_key(&tile) {
            self.status = format!("{tile} already exists");
            return;
        }
        let [width, height] = self.new_size;
        let mut drawing = Drawing::new(width, height);
        drawing.unsaved = true;
        self.drawings.insert(tile.clone(), drawing);
        self.open(&tile);
        self.new_name.clear();
    }

    fn drawing(&mut self) -> Option<&mut Drawing> {
        self.drawings.get_mut(self.current.as_deref()?)
    }

    fn save(&mut self) {
        let Some(tile) = self.current.clone() else {
            return;
        };
        let drawing = self
            .drawings
            .get_mut(&tile)
            .expect("the current drawing is open");
        self.status = match drawing.save(&tile) {
            Ok(()) => {
                drawing.unsaved = false;
                self.refresh();
                format!("Saved {tile}")
            }
            Err(error) => format!("Could not save {tile}: {error}"),
        };
    }

    /// Tool and zoom keys, and Cmd/Ctrl+S, Cmd/Ctrl+Z, Cmd/Ctrl+Shift+Z or Cmd/Ctrl+Y.
    pub fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (command, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        let pressed = |key: Key| ctx.input(|i| i.key_pressed(key));
        if command {
            if pressed(Key::S) {
                self.save();
            }
            if let Some(drawing) = self.drawing() {
                if pressed(Key::Z) && !shift {
                    drawing.undo();
                }
                if (pressed(Key::Z) && shift) || pressed(Key::Y) {
                    drawing.redo();
                }
            }
            return;
        }
        for (tool, _, key) in TOOLS {
            if pressed(key) {
                self.tool = tool;
            }
        }
        if let Some(zoom) = &mut self.zoom {
            if pressed(Key::Plus) || pressed(Key::Equals) {
                *zoom = (zoom.round() * 2.0).min(MAX_ZOOM);
            }
            if pressed(Key::Minus) {
                *zoom = (zoom.round() / 2.0).max(1.0);
            }
        }
    }

    /// The side panel in draw mode. `map` tells how often each tile is used; returns the tile
    /// deleted this frame, if any.
    pub fn side_panel(&mut self, ui: &mut egui::Ui, map: &Map) -> Option<String> {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let deleted = self.assets_ui(ui, map);
            if self.current.is_some() {
                ui.separator();
                self.tools_ui(ui);
                ui.separator();
                self.layers_ui(ui);
                ui.separator();
                self.preview_ui(ui);
            }
            ui.label(&self.status);
            ui.separator();
            ui.small(
                "Left click: draw\nRight click: erase\nAlt + click: pick color\nScroll: pan\n+ / -, pinch: zoom",
            );
            deleted
        })
        .inner
    }

    fn assets_ui(&mut self, ui: &mut egui::Ui, map: &Map) -> Option<String> {
        ui.heading("Assets");
        let tiles: BTreeSet<String> = self
            .tiles
            .iter()
            .chain(self.drawings.keys())
            .cloned()
            .collect();
        let mut open = None;
        for tile in tiles {
            let unsaved = self.drawings.get(&tile).is_some_and(|d| d.unsaved);
            let label = if unsaved {
                format!("{tile} •")
            } else {
                tile.clone()
            };
            if ui
                .selectable_label(self.current.as_ref() == Some(&tile), label)
                .clicked()
            {
                open = Some(tile);
            }
        }
        if let Some(tile) = open {
            self.open(&tile);
        }
        let deleted = self.delete_ui(ui, map);
        ui.add_space(4.0);
        ui.label("New asset");
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("new folder")
                .width(70.0)
                .selected_text(&self.new_folder)
                .show_ui(ui, |ui| {
                    for folder in ["floor", "objects"] {
                        let choice =
                            ui.selectable_value(&mut self.new_folder, folder.into(), folder);
                        if choice.changed() {
                            self.new_size = default_size(folder);
                        }
                    }
                });
            ui.add(
                TextEdit::singleline(&mut self.new_name)
                    .hint_text("name")
                    .desired_width(80.0),
            );
        });
        ui.horizontal(|ui| {
            ui.add(DragValue::new(&mut self.new_size[0]).range(1..=256));
            ui.label("×");
            ui.add(DragValue::new(&mut self.new_size[1]).range(1..=256));
            if ui.button("Create").clicked() {
                self.create();
            }
        });
        self.cabinets_ui(ui);
        deleted
    }

    fn cabinets_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.label("Cabinet views");
        ui.horizontal(|ui| {
            let selected = if self.skin.is_empty() {
                "plain"
            } else {
                &self.skin
            };
            egui::ComboBox::from_id_salt("skin")
                .width(90.0)
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.skin, String::new(), "plain");
                    for skin in &self.skins {
                        ui.selectable_value(&mut self.skin, skin.clone(), skin);
                    }
                });
            let make = ui.button("Make views").on_hover_text(
                "Renders the cabinet in this skin, from its saved textures, facing four ways",
            );
            if make.clicked() {
                self.make_views(false);
            }
        });
        if self.confirm_views {
            ui.label("Replace this skin's views? Changes made to them are lost.");
            ui.horizontal(|ui| {
                if ui.button("Replace").clicked() {
                    self.make_views(true);
                }
                if ui.button("Cancel").clicked() {
                    self.confirm_views = false;
                }
            });
        }
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.new_skin)
                    .hint_text("skin name")
                    .desired_width(80.0),
            );
            if ui.button("New skin").clicked() {
                self.create_skin();
            }
        });
    }

    /// Renders the selected skin's cabinet views and opens the first; asks before replacing
    /// views that exist unless `replace`.
    fn make_views(&mut self, replace: bool) {
        let skin = (!self.skin.is_empty()).then_some(self.skin.as_str());
        let sketches = match scaffold::views(skin) {
            Ok(sketches) => sketches,
            Err(error) => {
                self.status = format!("Could not read the skin: {error}");
                return;
            }
        };
        let exists = |tile: &String| self.tiles.contains(tile) || self.drawings.contains_key(tile);
        if !replace && sketches.iter().any(|sketch| exists(&sketch.tile)) {
            self.confirm_views = true;
            return;
        }
        self.confirm_views = false;
        let mut made = Vec::new();
        let mut failed = None;
        for sketch in sketches {
            let layers = sketch
                .layers
                .into_iter()
                .map(|(name, pixels)| Layer {
                    name: name.into(),
                    visible: true,
                    pixels,
                })
                .collect();
            let drawing = Drawing::from_layers(scaffold::WIDTH, scaffold::HEIGHT, layers);
            if let Err(error) = drawing.save(&sketch.tile) {
                failed = Some(format!("Could not save {}: {error}", sketch.tile));
                break;
            }
            self.drawings.insert(sketch.tile.clone(), drawing);
            made.push(sketch.tile);
        }
        self.refresh();
        if let Some(first) = made.first() {
            self.open(first);
        }
        self.status = failed.unwrap_or_else(|| format!("Made {}", made.join(", ")));
    }

    /// Starts a skin from the plain one's textures and opens its side panel.
    fn create_skin(&mut self) {
        let name = self.new_skin.trim().to_owned();
        if !valid_name(&name) {
            self.status = "Name the skin with letters, digits, - or _".into();
            return;
        }
        if self.skins.contains(&name) {
            self.status = format!("Skin {name} already exists");
            return;
        }
        for (file, width, height, pixels) in scaffold::plain_textures() {
            let key = format!("cabinets/{name}/{file}");
            let base = Layer {
                name: "base".into(),
                visible: true,
                pixels,
            };
            let drawing = Drawing::from_layers(width, height, vec![base]);
            if let Err(error) = drawing.save(&key) {
                self.status = format!("Could not save {key}: {error}");
                return;
            }
            self.drawings.insert(key, drawing);
        }
        self.refresh();
        self.skin.clone_from(&name);
        self.new_skin.clear();
        self.open(&format!("cabinets/{name}/side"));
        self.status = format!("Draw {name}'s textures and save them, then Make views");
    }

    /// Deletes the open asset after a second, confirming click.
    fn delete_ui(&mut self, ui: &mut egui::Ui, map: &Map) -> Option<String> {
        let tile = self.current.clone()?;
        let name = tile.split_once('/').map_or(tile.as_str(), |(_, name)| name);
        if !self.confirm_delete {
            if ui.button(format!("Delete {name}…")).clicked() {
                self.confirm_delete = true;
            }
            return None;
        }
        let uses = map
            .floor
            .iter()
            .chain(&map.objects)
            .filter(|placed| placed.tile == tile)
            .count();
        let mut warning = format!("Delete {tile}? Its PNG and layers are removed from disk.");
        if uses > 0 {
            warning += &format!(" The {uses} map cells using it are cleared.");
        }
        ui.label(warning);
        let mut deleted = None;
        ui.horizontal(|ui| {
            let delete = egui::Button::new(RichText::new("Delete").color(Color32::WHITE))
                .fill(Color32::from_rgb(170, 45, 45));
            if ui.add(delete).clicked() && self.delete(&tile) {
                deleted = Some(tile.clone());
            }
            if ui.button("Cancel").clicked() {
                self.confirm_delete = false;
            }
        });
        deleted
    }

    fn tools_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Tools");
        ui.horizontal_wrapped(|ui| {
            for (tool, label, _) in TOOLS {
                ui.selectable_value(&mut self.tool, tool, label);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Color");
            ui.color_edit_button_srgba_unmultiplied(&mut self.color);
        });
        // The drawing's own colors, to reuse.
        let colors = self.drawing().map(|d| d.colors()).unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
            for color in colors {
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::click());
                ui.painter().rect_filled(rect, 2.0, color32(color));
                if color == self.color {
                    let stroke = Stroke::new(1.5, Color32::WHITE);
                    ui.painter()
                        .rect_stroke(rect, 2.0, stroke, StrokeKind::Outside);
                }
                if response.clicked() {
                    self.color = color;
                }
            }
        });
    }

    fn layers_ui(&mut self, ui: &mut egui::Ui) {
        let Studio {
            drawings,
            current,
            layer_name,
            ..
        } = self;
        let Some(drawing) = current.as_deref().and_then(|tile| drawings.get_mut(tile)) else {
            return;
        };
        let count = drawing.layers.len();
        let active = drawing.active;
        let mut actions = Vec::new();
        ui.heading("Layers");
        ui.horizontal_wrapped(|ui| {
            for (label, enabled, action) in [
                ("Add", true, LayerAction::Add),
                ("Copy", true, LayerAction::Duplicate),
                ("Delete", count > 1, LayerAction::Delete),
                ("Up", active + 1 < count, LayerAction::Raise),
                ("Down", active > 0, LayerAction::Lower),
            ] {
                if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                    actions.push(action);
                }
            }
        });
        // Top layer first, as it stacks.
        for (i, layer) in drawing.layers.iter().enumerate().rev() {
            ui.horizontal(|ui| {
                let mut visible = layer.visible;
                if ui
                    .checkbox(&mut visible, "")
                    .on_hover_text("Visible")
                    .changed()
                {
                    actions.push(LayerAction::ToggleVisible(i));
                }
                if ui.selectable_label(i == active, &layer.name).clicked() {
                    actions.push(LayerAction::Select(i));
                }
            });
        }
        let field = ui.add(TextEdit::singleline(layer_name).hint_text("layer name"));
        if field.lost_focus() {
            let name = layer_name.trim();
            if !name.is_empty() && name != drawing.layers[active].name {
                actions.push(LayerAction::Rename(active, name.to_owned()));
            }
        } else if !field.has_focus() {
            layer_name.clone_from(&drawing.layers[active].name);
        }
        for action in actions {
            drawing.apply(action);
        }
    }

    fn preview_ui(&mut self, ui: &mut egui::Ui) {
        let Some(drawing) = self.drawing() else {
            return;
        };
        let (can_undo, can_redo) = (drawing.history.can_undo(), drawing.history.can_redo());
        let size = egui::vec2(drawing.width as f32, drawing.height as f32);
        let scale = (ui.available_width() / size.x).min(2.0);
        let scale = if scale >= 1.0 { scale.floor() } else { scale };
        let (rect, _) = ui.allocate_exact_size(size * scale, Sense::hover());
        paint_image(
            ui.painter(),
            rect.min,
            scale,
            drawing.width,
            &drawing.flattened(),
            false,
        );
        ui.horizontal_wrapped(|ui| {
            if ui.button("Save (Cmd+S)").clicked() {
                self.save();
            }
            if ui
                .add_enabled(can_undo, egui::Button::new("Undo"))
                .clicked()
                && let Some(drawing) = self.drawing()
            {
                drawing.undo();
            }
            if ui
                .add_enabled(can_redo, egui::Button::new("Redo"))
                .clicked()
                && let Some(drawing) = self.drawing()
            {
                drawing.redo();
            }
        });
    }

    /// The drawing, filling the rest of the window.
    pub fn canvas(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| self.canvas_contents(ui));
    }

    fn canvas_contents(&mut self, ui: &mut egui::Ui) {
        let Studio {
            drawings,
            current,
            tool,
            color,
            zoom,
            pan,
            ..
        } = self;
        let Some(drawing) = current.as_deref().and_then(|tile| drawings.get_mut(tile)) else {
            ui.centered_and_justified(|ui| ui.label("Pick an asset or create one"));
            return;
        };
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let size = egui::vec2(drawing.width as f32, drawing.height as f32);
        let zoom = zoom.get_or_insert_with(|| {
            let fit = (response.rect.size() / size).min_elem() * 0.8;
            fit.floor().clamp(1.0, MAX_ZOOM)
        });
        if response.hovered() {
            let (zoom_delta, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
            *zoom = (*zoom * zoom_delta).clamp(1.0, MAX_ZOOM);
            *pan += scroll;
        }
        let scale = zoom.round();
        let origin = (response.rect.center() + *pan - size * scale / 2.0).round();
        let flat = drawing.flattened();
        paint_image(&painter, origin, scale, drawing.width, &flat, true);
        let image = Rect::from_min_size(origin, size * scale);
        if scale >= GRID_ZOOM {
            let line = Stroke::new(1.0, Color32::from_white_alpha(18));
            for x in 0..=drawing.width {
                painter.vline(origin.x + x as f32 * scale, image.y_range(), line);
            }
            for y in 0..=drawing.height {
                painter.hline(image.x_range(), origin.y + y as f32 * scale, line);
            }
        }
        let border = Stroke::new(1.0, Color32::from_gray(110));
        painter.rect_stroke(image, 0.0, border, StrokeKind::Outside);

        let down = response.is_pointer_button_down_on();
        let pixel = ui.input(|i| i.pointer.interact_pos()).map(|pos| {
            let pixel = ((pos - origin) / scale).floor();
            IVec2::new(pixel.x as i32, pixel.y as i32)
        });
        if let Some(pixel) = pixel
            && (response.hovered() || down)
            && drawing.index(pixel).is_some()
        {
            let corner = origin + egui::vec2(pixel.x as f32, pixel.y as f32) * scale;
            let outline = Stroke::new(1.0, Color32::from_rgb(255, 217, 51));
            let rect = Rect::from_min_size(corner, egui::Vec2::splat(scale));
            painter.rect_stroke(rect, 0.0, outline, StrokeKind::Inside);
        }

        // A stroke, from press to release, is one undo step.
        if down {
            if let Some(pixel) = pixel {
                let (erase, pick) = ui.input(|i| {
                    (
                        i.pointer.button_down(PointerButton::Secondary),
                        i.modifiers.alt,
                    )
                });
                drawing.stroke(pixel, *tool, color, erase, pick, &flat);
            }
        } else if let Some(before) = drawing.stroke_start.take()
            && drawing.history.record(before, &drawing.layers)
        {
            drawing.unsaved = true;
        }
    }
}

#[derive(Clone, PartialEq)]
struct Layer {
    name: String,
    visible: bool,
    /// Rows from the top.
    pixels: Vec<Rgba>,
}

enum LayerAction {
    Select(usize),
    ToggleVisible(usize),
    Rename(usize, String),
    Add,
    Duplicate,
    Delete,
    Raise,
    Lower,
}

/// An image being drawn.
struct Drawing {
    width: u32,
    height: u32,
    /// Bottom first.
    layers: Vec<Layer>,
    /// The layer tools draw on.
    active: usize,
    history: History<Vec<Layer>>,
    /// The layers before the current stroke, while one is under way.
    stroke_start: Option<Vec<Layer>>,
    /// Where the stroke was last frame, so fast drags draw unbroken lines.
    last_pixel: Option<IVec2>,
    unsaved: bool,
}

/// art/<tile>/layers.ron
#[derive(Serialize, Deserialize)]
struct Manifest {
    width: u32,
    height: u32,
    /// Bottom first; layer i is in <i>.png.
    layers: Vec<LayerInfo>,
}

#[derive(Serialize, Deserialize)]
struct LayerInfo {
    name: String,
    visible: bool,
}

impl Drawing {
    fn new(width: u32, height: u32) -> Self {
        let base = Layer {
            name: "base".into(),
            visible: true,
            pixels: vec![CLEAR; (width * height) as usize],
        };
        Self::from_layers(width, height, vec![base])
    }

    fn from_layers(width: u32, height: u32, layers: Vec<Layer>) -> Self {
        Self {
            width,
            height,
            active: layers.len() - 1,
            layers,
            history: History::default(),
            stroke_start: None,
            last_pixel: None,
            unsaved: false,
        }
    }

    /// Opens a tile from its layers in art/, or from its flat PNG if it has none yet.
    fn load(tile: &str) -> Result<Self, Box<dyn Error>> {
        let dir = format!("{ART}/{tile}");
        let Ok(text) = fs::read_to_string(format!("{dir}/layers.ron")) else {
            let (width, height, pixels) = read_png(&flat_path(tile))?;
            let base = Layer {
                name: "base".into(),
                visible: true,
                pixels,
            };
            return Ok(Self::from_layers(width, height, vec![base]));
        };
        let manifest: Manifest = ron::from_str(&text)?;
        if manifest.layers.is_empty() {
            return Err(format!("{dir}/layers.ron lists no layers").into());
        }
        let mut layers = Vec::new();
        for (i, info) in manifest.layers.into_iter().enumerate() {
            let (width, height, pixels) = read_png(&format!("{dir}/{i}.png"))?;
            if (width, height) != (manifest.width, manifest.height) {
                return Err(format!(
                    "{dir}/{i}.png is {width}×{height}, not {}×{}",
                    manifest.width, manifest.height
                )
                .into());
            }
            layers.push(Layer {
                name: info.name,
                visible: info.visible,
                pixels,
            });
        }
        Ok(Self::from_layers(manifest.width, manifest.height, layers))
    }

    fn save(&self, tile: &str) -> Result<(), Box<dyn Error>> {
        let dir = format!("{ART}/{tile}");
        fs::create_dir_all(&dir)?;
        for (i, layer) in self.layers.iter().enumerate() {
            write_png(
                &format!("{dir}/{i}.png"),
                self.width,
                self.height,
                &layer.pixels,
            )?;
        }
        // Layers deleted since the last save.
        for i in self.layers.len().. {
            if fs::remove_file(format!("{dir}/{i}.png")).is_err() {
                break;
            }
        }
        let manifest = Manifest {
            width: self.width,
            height: self.height,
            layers: self
                .layers
                .iter()
                .map(|layer| LayerInfo {
                    name: layer.name.clone(),
                    visible: layer.visible,
                })
                .collect(),
        };
        let pretty = ron::ser::PrettyConfig::default();
        fs::write(
            format!("{dir}/layers.ron"),
            ron::ser::to_string_pretty(&manifest, pretty)?,
        )?;
        let flat = flat_path(tile);
        if let Some(folder) = Path::new(&flat).parent() {
            fs::create_dir_all(folder)?;
        }
        write_png(&flat, self.width, self.height, &self.flattened())
    }

    fn index(&self, pixel: IVec2) -> Option<usize> {
        let inside = pixel.x >= 0
            && pixel.y >= 0
            && pixel.x < self.width as i32
            && pixel.y < self.height as i32;
        inside.then(|| (pixel.y * self.width as i32 + pixel.x) as usize)
    }

    /// The visible layers blended bottom to top.
    fn flattened(&self) -> Vec<Rgba> {
        let mut flat = vec![CLEAR; (self.width * self.height) as usize];
        for layer in self.layers.iter().filter(|layer| layer.visible) {
            for (below, &above) in flat.iter_mut().zip(&layer.pixels) {
                *below = over(*below, above);
            }
        }
        flat
    }

    /// The colors in the drawing's layers.
    fn colors(&self) -> Vec<Rgba> {
        let colors: BTreeSet<Rgba> = self
            .layers
            .iter()
            .flat_map(|layer| &layer.pixels)
            .copied()
            .filter(|color| color[3] > 0)
            .collect();
        colors.into_iter().take(PALETTE_SIZE).collect()
    }

    /// Applies `tool` at `pixel` for one frame of a stroke. `flat` is the image as shown.
    fn stroke(
        &mut self,
        pixel: IVec2,
        tool: Tool,
        color: &mut Rgba,
        erase: bool,
        pick: bool,
        flat: &[Rgba],
    ) {
        let first = self.stroke_start.is_none();
        if first {
            self.stroke_start = Some(self.layers.clone());
        }
        let from = if first {
            pixel
        } else {
            self.last_pixel.unwrap_or(pixel)
        };
        self.last_pixel = Some(pixel);
        if pick || tool == Tool::Picker {
            if let Some(i) = self.index(pixel)
                && flat[i][3] > 0
            {
                *color = flat[i];
            }
            return;
        }
        let paint = if erase || tool == Tool::Eraser {
            CLEAR
        } else {
            *color
        };
        if tool == Tool::Fill {
            if first {
                self.fill(pixel, paint);
            }
            return;
        }
        for point in line(from, pixel) {
            if let Some(i) = self.index(point) {
                self.layers[self.active].pixels[i] = paint;
            }
        }
    }

    /// Fills the active layer's area of one color around `start`.
    fn fill(&mut self, start: IVec2, color: Rgba) {
        let Some(i) = self.index(start) else {
            return;
        };
        let target = self.layers[self.active].pixels[i];
        if target == color {
            return;
        }
        let mut stack = vec![start];
        while let Some(pixel) = stack.pop() {
            let Some(i) = self.index(pixel) else {
                continue;
            };
            let pixels = &mut self.layers[self.active].pixels;
            if pixels[i] == target {
                pixels[i] = color;
                stack.extend([IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y].map(|d| pixel + d));
            }
        }
    }

    /// Applies a layer change as one undo step.
    fn apply(&mut self, action: LayerAction) {
        if let LayerAction::Select(i) = action {
            self.active = i;
            return;
        }
        let before = self.layers.clone();
        let active = self.active;
        match action {
            LayerAction::Select(_) => unreachable!("handled above"),
            LayerAction::ToggleVisible(i) => self.layers[i].visible ^= true,
            LayerAction::Rename(i, name) => self.layers[i].name = name,
            LayerAction::Add => {
                let layer = Layer {
                    name: format!("layer {}", self.layers.len() + 1),
                    visible: true,
                    pixels: vec![CLEAR; (self.width * self.height) as usize],
                };
                self.layers.insert(active + 1, layer);
                self.active += 1;
            }
            LayerAction::Duplicate => {
                let mut copy = self.layers[active].clone();
                copy.name += " copy";
                self.layers.insert(active + 1, copy);
                self.active += 1;
            }
            LayerAction::Delete => {
                if self.layers.len() > 1 {
                    self.layers.remove(active);
                    self.active = active.saturating_sub(1);
                }
            }
            LayerAction::Raise => {
                if active + 1 < self.layers.len() {
                    self.layers.swap(active, active + 1);
                    self.active += 1;
                }
            }
            LayerAction::Lower => {
                if active > 0 {
                    self.layers.swap(active, active - 1);
                    self.active -= 1;
                }
            }
        }
        if self.history.record(before, &self.layers) {
            self.unsaved = true;
        }
    }

    fn undo(&mut self) {
        if self.history.undo(&mut self.layers) {
            self.after_history();
        }
    }

    fn redo(&mut self) {
        if self.history.redo(&mut self.layers) {
            self.after_history();
        }
    }

    fn after_history(&mut self) {
        self.active = self.active.min(self.layers.len() - 1);
        self.unsaved = true;
    }
}

/// `above` drawn over `below`.
fn over(below: Rgba, above: Rgba) -> Rgba {
    match above[3] {
        255 => return above,
        0 => return below,
        _ => {}
    }
    let a = above[3] as f32 / 255.0;
    let b = below[3] as f32 / 255.0 * (1.0 - a);
    let alpha = a + b;
    let channel = |i: usize| ((above[i] as f32 * a + below[i] as f32 * b) / alpha).round() as u8;
    [
        channel(0),
        channel(1),
        channel(2),
        (alpha * 255.0).round() as u8,
    ]
}

/// The pixels on a line from `from` to `to`, both included.
fn line(from: IVec2, to: IVec2) -> impl Iterator<Item = IVec2> {
    let steps = (to - from).abs().max_element().max(1);
    (0..=steps).map(move |step| {
        let t = step as f32 / steps as f32;
        from + ((to - from).as_vec2() * t).round().as_ivec2()
    })
}

fn color32([r, g, b, a]: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Paints `pixels`, rows of `width`, each `scale` points square, over a checkerboard when
/// `checker` is set so transparency shows. One mesh, so neighboring pixels meet without
/// anti-aliased seams.
fn paint_image(
    painter: &egui::Painter,
    origin: egui::Pos2,
    scale: f32,
    width: u32,
    pixels: &[Rgba],
    checker: bool,
) {
    let mut mesh = egui::Mesh::default();
    for (i, &color) in pixels.iter().enumerate() {
        let (x, y) = (i as u32 % width, i as u32 / width);
        let corner = origin + egui::vec2(x as f32, y as f32) * scale;
        let rect = Rect::from_min_size(corner, egui::Vec2::splat(scale));
        if checker {
            let gray = if (x + y) % 2 == 0 { 70 } else { 90 };
            mesh.add_colored_rect(rect, Color32::from_gray(gray));
        }
        if color[3] > 0 {
            mesh.add_colored_rect(rect, color32(color));
        }
    }
    painter.add(mesh);
}

/// Treats a file that is already gone as removed.
fn ignore_missing(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

pub fn read_png(path: &str) -> Result<(u32, u32, Vec<Rgba>), Box<dyn Error>> {
    let image = image::open(path)?.into_rgba8();
    let (width, height) = image.dimensions();
    Ok((width, height, image.pixels().map(|pixel| pixel.0).collect()))
}

fn write_png(path: &str, width: u32, height: u32, pixels: &[Rgba]) -> Result<(), Box<dyn Error>> {
    let image = image::RgbaImage::from_raw(width, height, pixels.concat())
        .expect("a drawing has width × height pixels");
    image.save(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba = [255, 0, 0, 255];
    const BLUE: Rgba = [0, 0, 255, 255];

    #[test]
    fn lines_have_no_gaps() {
        let points: Vec<IVec2> = line(IVec2::new(0, 0), IVec2::new(5, -2)).collect();
        assert_eq!(points.first(), Some(&IVec2::new(0, 0)));
        assert_eq!(points.last(), Some(&IVec2::new(5, -2)));
        for pair in points.windows(2) {
            assert_eq!((pair[1] - pair[0]).abs().max_element(), 1);
        }
    }

    #[test]
    fn fill_stops_at_other_colors() {
        let mut drawing = Drawing::new(3, 3);
        for y in 0..3 {
            let i = drawing.index(IVec2::new(1, y)).unwrap();
            drawing.layers[0].pixels[i] = RED;
        }
        drawing.fill(IVec2::ZERO, BLUE);
        let row: Vec<Rgba> = drawing.layers[0].pixels[3..6].to_vec();
        assert_eq!(row, [BLUE, RED, CLEAR]);
    }

    #[test]
    fn flattening_skips_hidden_layers_and_blends() {
        let mut drawing = Drawing::new(1, 1);
        drawing.layers[0].pixels[0] = [0, 0, 0, 255];
        drawing.apply(LayerAction::Add);
        drawing.layers[1].pixels[0] = [255, 255, 255, 128];
        assert_eq!(drawing.flattened(), [[128, 128, 128, 255]]);
        drawing.apply(LayerAction::ToggleVisible(1));
        assert_eq!(drawing.flattened(), [[0, 0, 0, 255]]);
        drawing.undo();
        assert_eq!(drawing.flattened(), [[128, 128, 128, 255]]);
    }

    #[test]
    fn pngs_round_trip() {
        let (width, height, pixels) =
            read_png(&format!("{ASSETS}/tiles/objects/cabinet.png")).unwrap();
        let path = std::env::temp_dir().join("editor-draw-round-trip.png");
        let path = path.to_str().unwrap();
        write_png(path, width, height, &pixels).unwrap();
        assert_eq!(read_png(path).unwrap(), (width, height, pixels));
    }
}
