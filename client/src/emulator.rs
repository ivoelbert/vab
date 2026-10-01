//! Plays a cabinet's game with the page's emulator worker (web/emulator/worker.js). The page
//! sits the player at the cabinet and plays alone or online with whoever takes the other seat,
//! or has them watch the game the players there are playing. Frames come in through
//! `push_frame` and fill the screen, a status line (who you play with, the connection) through
//! `game_status`, the player's buttons go out through `emulatorInput`, and Esc stops the game
//! and returns to the bar.

use std::cell::RefCell;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use wasm_bindgen::prelude::*;
use world::Game;

use crate::Mode;
use crate::chat::{Chat, chat_closed};

/// Keys and the RetroPad button ids (libretro.h) they press. FBNeo maps MK's panel to
/// A S D = high punch, high kick, block and Z X C = low punch, low kick, block.
pub const KEYS: [(KeyCode, u16); 12] = [
    (KeyCode::ArrowUp, 4),
    (KeyCode::ArrowDown, 5),
    (KeyCode::ArrowLeft, 6),
    (KeyCode::ArrowRight, 7),
    (KeyCode::Digit5, 2), // coin (SELECT)
    (KeyCode::Digit1, 3), // start
    (KeyCode::KeyA, 1),   // Y
    (KeyCode::KeyS, 9),   // X
    (KeyCode::KeyD, 10),  // L
    (KeyCode::KeyZ, 0),   // B
    (KeyCode::KeyX, 8),   // A
    (KeyCode::KeyC, 11),  // R
];

thread_local! {
    static LATEST_FRAME: RefCell<Option<(UVec2, Vec<u8>)>> = const { RefCell::new(None) };
    static LATEST_STATUS: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Called by index.html with each RGBA frame the emulator worker posts.
#[wasm_bindgen]
pub fn push_frame(rgba: Vec<u8>, width: u32, height: u32) {
    LATEST_FRAME.set(Some((UVec2::new(width, height), rgba)));
}

/// Called by index.html with a line about the game: who you play with, how the connection is.
#[wasm_bindgen]
pub fn game_status(text: String) {
    LATEST_STATUS.set(Some(text));
}

// Defined in index.html.
#[wasm_bindgen]
extern "C" {
    /// Sits the player at `cabinet` ("x,y") and starts its game, for up to `players` at once.
    #[wasm_bindgen(js_name = emulatorPlay)]
    fn emulator_play(
        core: &str,
        rom: &str,
        bios: Option<String>,
        cabinet: &str,
        turns: bool,
        players: u32,
    );
    /// Watches the game at `cabinet` ("x,y"), streamed from one of its players.
    #[wasm_bindgen(js_name = emulatorWatch)]
    fn emulator_watch(core: &str, rom: &str, bios: Option<String>, cabinet: &str, turns: bool);
    #[wasm_bindgen(js_name = emulatorStop)]
    fn emulator_stop();
    /// Sends the player's RetroPad mask to the worker, for their seat's controller.
    #[wasm_bindgen(js_name = emulatorInput)]
    fn emulator_input(mask: u16);
}

/// The game being played or watched, while `Mode::Playing`.
#[derive(Resource)]
pub struct PlayingGame {
    pub title: String,
    pub watching: bool,
}

/// Starts the game at the cabinet in `cell`; switch to `Mode::Playing` to show it.
pub fn play(cell: IVec2, game: &Game) {
    LATEST_FRAME.set(None);
    LATEST_STATUS.set(None);
    let cabinet = cabinet_id(cell);
    let bios = game.bios.clone();
    emulator_play(
        &game.core,
        &game.rom,
        bios,
        &cabinet,
        game.turns,
        game.players,
    );
}

/// Watches the game being played at the cabinet in `cell`; switch to `Mode::Playing` to show it.
pub fn watch(cell: IVec2, game: &Game) {
    LATEST_FRAME.set(None);
    LATEST_STATUS.set(None);
    let cabinet = cabinet_id(cell);
    let bios = game.bios.clone();
    emulator_watch(&game.core, &game.rom, bios, &cabinet, game.turns);
}

/// How the page and the room name a cabinet: its cell, "x,y".
pub fn cabinet_id(cell: IVec2) -> String {
    format!("{},{}", cell.x, cell.y)
}

pub struct EmulatorPlugin;

impl Plugin for EmulatorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Mode::Playing), show_screen)
            .add_systems(
                Update,
                (
                    show_latest_frame,
                    show_status,
                    send_input,
                    leave.run_if(chat_closed),
                )
                    .run_if(in_state(Mode::Playing)),
            )
            .add_systems(OnExit(Mode::Playing), stop);
    }
}

/// The black overlay covering the bar while a game plays.
#[derive(Component)]
struct Overlay;

/// The image node that shows the game.
#[derive(Component)]
struct Screen;

/// The line above the game about who you play with.
#[derive(Component)]
struct Status;

fn show_screen(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn((
        Overlay,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::BLACK),
        GlobalZIndex(1),
        children![
            (
                Screen,
                ImageNode::new(images.add(screen_image(4, 3))),
                Node {
                    height: Val::Percent(100.0),
                    max_width: Val::Percent(100.0),
                    aspect_ratio: Some(4.0 / 3.0),
                    ..default()
                },
            ),
            (
                Status,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(Color::WHITE),
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(8.0),
                    left: Val::Px(8.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
                // Over the game, which may run under it.
                ZIndex(1),
            ),
        ],
    ));
}

/// A black image the size of a game frame, for the screen to start with.
fn screen_image(width: u32, height: u32) -> Image {
    Image::new_fill(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
}

fn show_latest_frame(
    mut images: ResMut<Assets<Image>>,
    mut screens: Query<(&ImageNode, &mut Node), With<Screen>>,
) {
    let Some((size, rgba)) = LATEST_FRAME.take() else {
        return;
    };
    for (image_node, mut node) in &mut screens {
        let Some(mut image) = images.get_mut(&image_node.image) else {
            continue;
        };
        if image.size() != size {
            *image = screen_image(size.x, size.y);
            // Arcade monitors were 4:3, or 3:4 when mounted vertically (Pac-Man).
            node.aspect_ratio = Some(if size.x >= size.y {
                4.0 / 3.0
            } else {
                3.0 / 4.0
            });
        }
        image.data = Some(rgba.clone());
    }
}

fn show_status(mut status: Single<&mut Text, With<Status>>) {
    if let Some(text) = LATEST_STATUS.take() {
        status.0 = text;
    }
}

fn send_input(keys: Res<ButtonInput<KeyCode>>, chat: Res<Chat>, mut sent: Local<u16>) {
    // While typing in the chat, the player's hands are off the controls.
    let mask = KEYS
        .iter()
        .filter(|(key, _)| !chat.is_open() && keys.pressed(*key))
        .fold(0, |mask, (_, id)| mask | 1 << id);
    if mask != *sent {
        emulator_input(mask);
        *sent = mask;
    }
}

fn leave(keys: Res<ButtonInput<KeyCode>>, mut mode: ResMut<NextState<Mode>>) {
    if keys.just_pressed(KeyCode::Escape) {
        mode.set(Mode::Walking);
    }
}

fn stop(mut commands: Commands, overlays: Query<Entity, With<Overlay>>) {
    emulator_stop();
    LATEST_FRAME.set(None);
    for overlay in &overlays {
        commands.entity(overlay).despawn();
    }
}
