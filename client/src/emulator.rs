//! Bridge to the FBNeo emulator worker (web/emulator/worker.js). The page passes each frame
//! in through `push_frame`, and player 1's buttons go back out through `emulatorInput`.

use std::cell::RefCell;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use wasm_bindgen::prelude::*;

/// Keys and the RetroPad button ids (libretro.h) they press. FBNeo maps MK's panel to
/// A S D = high punch, high kick, block and Z X C = low punch, low kick, block.
const KEYS: [(KeyCode, u16); 12] = [
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
}

/// Called by index.html with each RGBA frame the emulator worker posts.
#[wasm_bindgen]
pub fn push_frame(rgba: Vec<u8>, width: u32, height: u32) {
    LATEST_FRAME.set(Some((UVec2::new(width, height), rgba)));
}

#[wasm_bindgen]
extern "C" {
    /// Defined in index.html: sends player 1's RetroPad mask to the worker.
    #[wasm_bindgen(js_name = emulatorInput)]
    fn emulator_input(mask: u16);
}

pub struct EmulatorPlugin;

impl Plugin for EmulatorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (show_latest_frame, send_input));
    }
}

/// Marks the sprite that shows the game.
#[derive(Component)]
pub struct Screen;

/// A black image the size of a game frame, for the screen sprite to start with.
pub fn screen_image(width: u32, height: u32) -> Image {
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
    mut screens: Query<&mut Sprite, With<Screen>>,
) {
    let Some((size, rgba)) = LATEST_FRAME.take() else {
        return;
    };
    for mut sprite in &mut screens {
        let Some(mut image) = images.get_mut(&sprite.image) else {
            continue;
        };
        if image.size() != size {
            *image = screen_image(size.x, size.y);
            // Arcade monitors were 4:3, or 3:4 when mounted vertically (Pac-Man). The camera
            // fits a 4 x 3 area, so a vertical screen is 2.25 x 3.
            sprite.custom_size = Some(if size.x >= size.y {
                Vec2::new(4.0, 3.0)
            } else {
                Vec2::new(2.25, 3.0)
            });
        }
        image.data = Some(rgba.clone());
    }
}

fn send_input(keys: Res<ButtonInput<KeyCode>>, mut sent: Local<u16>) {
    let mask = KEYS
        .iter()
        .filter(|(key, _)| keys.pressed(*key))
        .fold(0, |mask, (_, id)| mask | 1 << id);
    if mask != *sent {
        emulator_input(mask);
        *sent = mask;
    }
}
