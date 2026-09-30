//! Bar layout editor: paint floor tiles and place objects on the isometric grid, then save
//! to assets/maps/bar.ron. Desktop-only dev tool (`make editor`), not part of the web build.
//!
//! The palette lists every PNG in assets/tiles/floor and assets/tiles/objects, and images
//! reload when their files change, so art edited in draw mode or a pixel-art app shows up
//! right away.

mod draw;
mod history;
mod scaffold;

use std::fs;

use bevy::input::gestures::PinchGesture;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use draw::Studio;
use history::History;
use world::{
    Game, Map, MapSprite, Placed, TILE_HEIGHT, TILE_WIDTH, cell_to_world, games_from_ron,
    map_sprite, world_to_cell,
};

/// The repo's assets/ folder (this crate lives in tools/editor).
const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
/// The repo's art/ folder: layer sources and cabinet skins, kept out of the web build.
const ART: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../art");
const MAP_FILE: &str = "maps/bar.ron";
const GAMES_FILE: &str = "games.ron";
/// Grid lines drawn around the origin, in cells.
const GRID_RADIUS: i32 = 16;
/// Largest brush, in cells per side.
const MAX_BRUSH_SIZE: i32 = 9;
const MIN_ZOOM: f32 = 1.0;
const MAX_ZOOM: f32 = 8.0;
/// Trackpad pinch (in magnification) that changes the zoom by one step.
const PINCH_PER_ZOOM_STEP: f32 = 0.2;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: ASSETS.into(),
                    watch_for_changes_override: Some(true),
                    ..default()
                })
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Bar editor".into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(EguiPlugin::default())
        .insert_resource(ClearColor(Color::srgb(0.08, 0.08, 0.1)))
        .init_resource::<Editor>()
        .init_resource::<Studio>()
        .init_resource::<Mode>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                (move_camera, resize_brush, paint, draw_grid, shortcuts)
                    .run_if(resource_equals(Mode::Map)),
                redraw_map,
            ),
        )
        .add_systems(EguiPrimaryContextPass, panel)
        .run();
}

/// Whether the editor shows the map or draws an asset.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default)]
enum Mode {
    #[default]
    Map,
    Draw,
}

#[derive(Resource)]
struct Editor {
    map: Map,
    floor_tiles: Vec<String>,
    object_tiles: Vec<String>,
    /// The tile left click paints.
    brush: String,
    /// Cells per side of the square the brush paints and erases.
    brush_size: i32,
    /// The games cabinets can run (assets/games.ron).
    games: Vec<Game>,
    /// ROM set given to cabinets as they are painted; empty for none.
    game: String,
    hovered: Option<IVec2>,
    zoom: f32,
    /// Pinch not yet turned into a whole zoom step.
    pinch: f32,
    map_changed: bool,
    pointer_over_panel: bool,
    /// The map before the current paint or erase stroke; `None` when no stroke is under way
    /// or the press began on the panel.
    stroke_start: Option<Map>,
    history: History<Map>,
    typing_in_panel: bool,
    status: String,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            map: Map::default(),
            floor_tiles: tiles_in("floor"),
            object_tiles: tiles_in("objects"),
            brush: String::new(),
            brush_size: 1,
            games: Vec::new(),
            game: String::new(),
            hovered: None,
            zoom: 3.0,
            pinch: 0.0,
            map_changed: true,
            pointer_over_panel: false,
            stroke_start: None,
            history: History::default(),
            typing_in_panel: false,
            status: String::new(),
        }
    }
}

/// Tile names ("floor/wood") for the PNGs in assets/tiles/<folder>.
fn tiles_in(folder: &str) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(format!("{ASSETS}/tiles/{folder}"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_stem()?.to_string_lossy().into_owned();
            (path.extension()? == "png").then(|| format!("{folder}/{name}"))
        })
        .collect();
    names.sort();
    names
}

fn setup(mut commands: Commands, mut editor: ResMut<Editor>) {
    commands.spawn(Camera2d);
    editor.brush = editor.floor_tiles.first().cloned().unwrap_or_default();
    let games = fs::read_to_string(format!("{ASSETS}/{GAMES_FILE}"))
        .map_err(|error| error.to_string())
        .and_then(|text| games_from_ron(&text).map_err(|error| error.to_string()));
    let games_status = match games {
        Ok(games) => {
            editor.games = games;
            String::new()
        }
        Err(error) => format!("\nCould not read {GAMES_FILE}: {error}"),
    };
    editor.status = match fs::read_to_string(format!("{ASSETS}/{MAP_FILE}")) {
        Ok(text) => match Map::from_ron(&text) {
            Ok(map) => {
                editor.map = map;
                format!("Loaded {MAP_FILE}")
            }
            Err(error) => format!("Could not read {MAP_FILE}: {error}"),
        },
        Err(_) => format!("New map; saves to {MAP_FILE}"),
    } + &games_status;
}

/// Arrows/WASD or scrolling pan; + and - or a trackpad pinch zoom in whole steps so pixels
/// stay square.
fn move_camera(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    mut pinches: MessageReader<PinchGesture>,
    time: Res<Time>,
    mut editor: ResMut<Editor>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    let (mut transform, mut projection) = camera.into_inner();
    let mut pan = Vec2::ZERO;
    if !editor.typing_in_panel {
        for (key, direction) in [
            (KeyCode::ArrowLeft, Vec2::NEG_X),
            (KeyCode::KeyA, Vec2::NEG_X),
            (KeyCode::ArrowRight, Vec2::X),
            (KeyCode::KeyD, Vec2::X),
            (KeyCode::ArrowUp, Vec2::Y),
            (KeyCode::KeyW, Vec2::Y),
            (KeyCode::ArrowDown, Vec2::NEG_Y),
            (KeyCode::KeyS, Vec2::NEG_Y),
        ] {
            if keys.pressed(key) {
                pan += direction * 600.0 * time.delta_secs();
            }
        }
        if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
            editor.zoom = (editor.zoom + 1.0).min(MAX_ZOOM);
        }
        if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
            editor.zoom = (editor.zoom - 1.0).max(MIN_ZOOM);
        }
    }
    if !editor.pointer_over_panel {
        let lines_to_pixels = match scroll.unit {
            MouseScrollUnit::Line => 40.0,
            MouseScrollUnit::Pixel => 1.0,
        };
        pan += Vec2::new(-scroll.delta.x, scroll.delta.y) * lines_to_pixels;
    }
    let pinched: f32 = pinches.read().map(|pinch| pinch.0).sum();
    if !editor.pointer_over_panel {
        editor.pinch += pinched;
        let steps = (editor.pinch / PINCH_PER_ZOOM_STEP).trunc();
        editor.pinch -= steps * PINCH_PER_ZOOM_STEP;
        editor.zoom = (editor.zoom + steps).clamp(MIN_ZOOM, MAX_ZOOM);
    }
    transform.translation += (pan / editor.zoom).extend(0.0);
    if let Projection::Orthographic(ortho) = &mut *projection {
        ortho.scale = 1.0 / editor.zoom;
    }
}

/// [ and ] shrink and grow the brush.
fn resize_brush(keys: Res<ButtonInput<KeyCode>>, mut editor: ResMut<Editor>) {
    if editor.typing_in_panel {
        return;
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        editor.brush_size = (editor.brush_size - 1).max(1);
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        editor.brush_size = (editor.brush_size + 1).min(MAX_BRUSH_SIZE);
    }
}

/// The cells a brush of `size` covers when centered on `center`.
fn brush_cells(center: IVec2, size: i32) -> impl Iterator<Item = IVec2> {
    let start = center - IVec2::splat((size - 1) / 2);
    (0..size).flat_map(move |x| (0..size).map(move |y| start + IVec2::new(x, y)))
}

/// Left click paints the brush on the hovered cells, right click erases them (objects first).
fn paint(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform)>,
    mut editor: ResMut<Editor>,
) {
    let (camera, camera_transform) = *camera;
    editor.hovered = window
        .cursor_position()
        .and_then(|cursor| camera.viewport_to_world_2d(camera_transform, cursor).ok())
        .map(world_to_cell);
    let editor = &mut *editor;
    let mouse = [MouseButton::Left, MouseButton::Right];
    // A stroke is one undo step.
    if !buttons.any_pressed(mouse)
        && let Some(before) = editor.stroke_start.take()
    {
        record_undo(editor, before);
    }
    if buttons.any_just_pressed(mouse) && editor.stroke_start.is_none() {
        editor.stroke_start = (!editor.pointer_over_panel).then(|| editor.map.clone());
    }
    let Some(center) = editor.hovered else { return };
    if editor.pointer_over_panel || editor.stroke_start.is_none() {
        return;
    }

    if buttons.pressed(MouseButton::Left) && !editor.brush.is_empty() {
        let is_object = editor.brush.starts_with("objects/");
        let game = (is_object && editor.brush.contains("cabinet") && !editor.game.is_empty())
            .then(|| editor.game.clone());
        let layer = if is_object {
            &mut editor.map.objects
        } else {
            &mut editor.map.floor
        };
        for cell in brush_cells(center, editor.brush_size) {
            let placed = Placed {
                x: cell.x,
                y: cell.y,
                tile: editor.brush.clone(),
                game: game.clone(),
            };
            if !layer.contains(&placed) {
                layer.retain(|p| (p.x, p.y) != (cell.x, cell.y));
                layer.push(placed);
                editor.map_changed = true;
            }
        }
    }

    if buttons.pressed(MouseButton::Right) {
        for cell in brush_cells(center, editor.brush_size) {
            let at_cell = |p: &Placed| (p.x, p.y) == (cell.x, cell.y);
            for layer in [&mut editor.map.objects, &mut editor.map.floor] {
                if layer.iter().any(at_cell) {
                    layer.retain(|p| !at_cell(p));
                    editor.map_changed = true;
                    break;
                }
            }
        }
    }
}

/// Redraws every map sprite after an edit; maps are small, so this stays simple.
fn redraw_map(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut editor: ResMut<Editor>,
    drawn: Query<Entity, With<MapSprite>>,
) {
    if !editor.map_changed {
        return;
    }
    editor.map_changed = false;
    for entity in &drawn {
        commands.entity(entity).despawn();
    }
    for placed in &editor.map.floor {
        commands.spawn(map_sprite(&asset_server, placed, false));
    }
    for placed in &editor.map.objects {
        commands.spawn(map_sprite(&asset_server, placed, true));
    }
}

fn draw_grid(mut gizmos: Gizmos, editor: Res<Editor>) {
    let diamond = |x: i32, y: i32| {
        let c = cell_to_world(x, y);
        let (w, h) = (TILE_WIDTH / 2.0, TILE_HEIGHT / 2.0);
        [
            c + Vec2::new(0.0, h),
            c + Vec2::new(w, 0.0),
            c + Vec2::new(0.0, -h),
            c + Vec2::new(-w, 0.0),
            c + Vec2::new(0.0, h),
        ]
    };
    let faint = Color::srgba(1.0, 1.0, 1.0, 0.06);
    for x in -GRID_RADIUS..=GRID_RADIUS {
        for y in -GRID_RADIUS..=GRID_RADIUS {
            gizmos.linestrip_2d(diamond(x, y), faint);
        }
    }
    if let Some(center) = editor.hovered {
        for cell in brush_cells(center, editor.brush_size) {
            gizmos.linestrip_2d(diamond(cell.x, cell.y), Color::srgb(1.0, 0.85, 0.2));
        }
    }
    // A dot above each cabinet that has a game.
    for placed in editor.map.objects.iter().filter(|p| p.game.is_some()) {
        let above = cell_to_world(placed.x, placed.y) + Vec2::Y * 44.0;
        gizmos.circle_2d(above, 2.5, Color::srgb(0.3, 1.0, 0.4));
    }
}

/// Cmd/Ctrl+S saves, Cmd/Ctrl+Z undoes, Cmd/Ctrl+Shift+Z or Cmd/Ctrl+Y redoes.
fn shortcuts(keys: Res<ButtonInput<KeyCode>>, mut editor: ResMut<Editor>) {
    let modifier = [
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ];
    if !keys.any_pressed(modifier) {
        return;
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if keys.just_pressed(KeyCode::KeyS) {
        save(&mut editor);
    }
    if keys.just_pressed(KeyCode::KeyZ) && !shift {
        undo(&mut editor);
    }
    if (keys.just_pressed(KeyCode::KeyZ) && shift) || keys.just_pressed(KeyCode::KeyY) {
        redo(&mut editor);
    }
}

/// Clears a deleted tile from the map, as one undo step, and from the palette.
fn forget_tile(editor: &mut Editor, tile: &str) {
    let before = editor.map.clone();
    editor.map.floor.retain(|placed| placed.tile != tile);
    editor.map.objects.retain(|placed| placed.tile != tile);
    if before != editor.map {
        editor.map_changed = true;
        editor.status = format!("Cleared {tile} from the map; save to keep that");
    }
    record_undo(editor, before);
    editor.floor_tiles = tiles_in("floor");
    editor.object_tiles = tiles_in("objects");
    if editor.brush == tile {
        editor.brush = editor.floor_tiles.first().cloned().unwrap_or_default();
    }
}

/// Keeps `before` for undo if the map has changed since.
fn record_undo(editor: &mut Editor, before: Map) {
    editor.history.record(before, &editor.map);
}

fn undo(editor: &mut Editor) {
    if editor.history.undo(&mut editor.map) {
        editor.map_changed = true;
    }
}

fn redo(editor: &mut Editor) {
    if editor.history.redo(&mut editor.map) {
        editor.map_changed = true;
    }
}

fn save(editor: &mut Editor) {
    let path = format!("{ASSETS}/{MAP_FILE}");
    let result = fs::create_dir_all(format!("{ASSETS}/maps"))
        .and_then(|()| fs::write(&path, editor.map.to_ron()));
    editor.status = match result {
        Ok(()) => format!("Saved {MAP_FILE}"),
        Err(error) => format!("Could not save: {error}"),
    };
}

fn panel(
    mut contexts: EguiContexts,
    mut editor: ResMut<Editor>,
    mut studio: ResMut<Studio>,
    mut mode: ResMut<Mode>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let editor = &mut *editor;
    // egui 0.36 panels are shown inside a Ui; this one covers the window (see bevy_egui's
    // side_panel example).
    let mut window_ui = egui::Ui::new(
        ctx.clone(),
        "window".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let palette = egui::Panel::left("palette")
        .resizable(false)
        .default_size(190.0)
        .show(&mut window_ui, |ui| {
            ui.horizontal(|ui| {
                if ui.selectable_label(*mode == Mode::Map, "Map").clicked() && *mode != Mode::Map {
                    *mode = Mode::Map;
                    // Pick up assets made in draw mode.
                    editor.floor_tiles = tiles_in("floor");
                    editor.object_tiles = tiles_in("objects");
                }
                if ui.selectable_label(*mode == Mode::Draw, "Draw").clicked() && *mode != Mode::Draw
                {
                    *mode = Mode::Draw;
                    studio.enter(&editor.brush);
                }
            });
            ui.separator();
            match *mode {
                Mode::Map => map_panel(ui, editor),
                Mode::Draw => {
                    if let Some(tile) = studio.side_panel(ui, &editor.map) {
                        forget_tile(editor, &tile);
                    }
                }
            }
        })
        .response
        .rect;
    if *mode == Mode::Draw {
        studio.canvas(&mut window_ui);
        studio.shortcuts(ctx);
    }
    // Panels in a background Ui aren't areas egui hit-tests, so is_pointer_over_egui() misses
    // the palette; check its rect too.
    let over_palette = ctx
        .pointer_hover_pos()
        .is_some_and(|pos| palette.contains(pos));
    editor.pointer_over_panel =
        over_palette || ctx.is_pointer_over_egui() || ctx.egui_wants_pointer_input();
    editor.typing_in_panel = ctx.egui_wants_keyboard_input();
    Ok(())
}

fn map_panel(ui: &mut egui::Ui, editor: &mut Editor) {
    for (heading, tiles) in [
        ("Floor", &editor.floor_tiles),
        ("Objects", &editor.object_tiles),
    ] {
        ui.heading(heading);
        for tile in tiles {
            let name = tile.split_once('/').map_or(tile.as_str(), |(_, name)| name);
            if ui.selectable_label(editor.brush == *tile, name).clicked() {
                editor.brush = tile.clone();
            }
        }
        ui.add_space(8.0);
    }
    ui.add(egui::Slider::new(&mut editor.brush_size, 1..=MAX_BRUSH_SIZE).text("Brush size"));
    ui.separator();
    ui.label("Cabinet game");
    let selected = editor
        .games
        .iter()
        .find(|game| game.rom == editor.game)
        .map_or("(none)", |game| game.title.as_str());
    egui::ComboBox::from_id_salt("game")
        .selected_text(selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut editor.game, String::new(), "(none)");
            for game in &editor.games {
                ui.selectable_value(&mut editor.game, game.rom.clone(), &game.title);
            }
        });
    ui.small("Paint a cabinet to place or reassign it.");
    ui.separator();
    if let Some(cell) = editor.hovered {
        let object = editor
            .map
            .objects
            .iter()
            .find(|p| (p.x, p.y) == (cell.x, cell.y));
        let text = match object {
            Some(Placed {
                tile,
                game: Some(rom),
                ..
            }) => {
                let title = editor.games.iter().find(|game| &game.rom == rom);
                format!(
                    "{tile} [{}]",
                    title.map_or(rom.as_str(), |game| &game.title)
                )
            }
            Some(placed) => placed.tile.clone(),
            None => "empty".into(),
        };
        ui.label(format!("Cell ({}, {}): {text}", cell.x, cell.y));
    }
    ui.horizontal(|ui| {
        if ui.button("Save (Cmd+S)").clicked() {
            save(editor);
        }
        if ui.button("Clear").clicked() {
            let before = std::mem::take(&mut editor.map);
            record_undo(editor, before);
            editor.map_changed = true;
            editor.status = format!("Cleared; {MAP_FILE} is unchanged until you save");
        }
    });
    ui.horizontal(|ui| {
        let undo_button = egui::Button::new("Undo (Cmd+Z)");
        if ui
            .add_enabled(editor.history.can_undo(), undo_button)
            .clicked()
        {
            undo(editor);
        }
        let redo_button = egui::Button::new("Redo (Cmd+Shift+Z)");
        if ui
            .add_enabled(editor.history.can_redo(), redo_button)
            .clicked()
        {
            redo(editor);
        }
    });
    ui.label(&editor.status);
    ui.separator();
    ui.small(
                "Left click: paint\nRight click: erase\n[ / ]: brush size\nScroll, arrows, WASD: pan\n+ / -, pinch: zoom",
            );
}
