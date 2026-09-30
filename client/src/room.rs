//! Everyone else in the bar room, as the page hears about them (web/room.js): each walks to
//! where the room last said they are. The player's own position goes out through `roomMove`.

use std::cell::RefCell;
use std::collections::HashMap;

use bevy::prelude::*;
use bevy::sprite::Anchor;
use wasm_bindgen::prelude::*;

use crate::player;

/// A little faster than walking, so others catch up with where they said they are.
const SPEED: f32 = 80.0;
/// Further than this and they jump there (they sat down, or the connection dropped for a bit).
const SNAP: f32 = 48.0;

enum Update {
    Moved { id: u32, feet: Vec2, flip: bool },
    Left(u32),
    Reset,
}

thread_local! {
    static UPDATES: RefCell<Vec<Update>> = const { RefCell::new(Vec::new()) };
}

/// Called by index.html when a player shows up or moves; `x`, `y` are their feet in world pixels.
#[wasm_bindgen]
pub fn player_moved(id: u32, x: f32, y: f32, flip: bool) {
    let feet = Vec2::new(x, y);
    UPDATES.with_borrow_mut(|updates| updates.push(Update::Moved { id, feet, flip }));
}

#[wasm_bindgen]
pub fn player_left(id: u32) {
    UPDATES.with_borrow_mut(|updates| updates.push(Update::Left(id)));
}

/// Called on (re)joining the room, before everyone in it is sent again.
#[wasm_bindgen]
pub fn players_reset() {
    UPDATES.with_borrow_mut(|updates| updates.push(Update::Reset));
}

// Defined in index.html.
#[wasm_bindgen]
extern "C" {
    /// Tells the room where the player's feet are.
    #[wasm_bindgen(js_name = roomMove)]
    pub fn room_move(x: f32, y: f32, flip: bool);
}

pub struct RoomPlugin;

impl Plugin for RoomPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (apply_updates, walk_others).chain());
    }
}

/// Another player: where they are drawn and where the room says they are.
#[derive(Component)]
struct Other {
    feet: Vec2,
    target: Vec2,
}

fn apply_updates(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut others: Local<HashMap<u32, Entity>>,
    mut query: Query<(&mut Other, &mut Sprite)>,
) {
    for update in UPDATES.take() {
        match update {
            Update::Moved { id, feet, flip } => {
                if let Some(&entity) = others.get(&id) {
                    if let Ok((mut other, mut sprite)) = query.get_mut(entity) {
                        other.target = feet;
                        sprite.flip_x = flip;
                        continue;
                    }
                    // Spawned earlier in this same batch: replace what it started with.
                    commands.entity(entity).insert(Other { feet, target: feet });
                    continue;
                }
                let mut sprite = Sprite::from_image(asset_server.load("characters/player.png"));
                sprite.color = tint(id);
                sprite.flip_x = flip;
                let entity = commands
                    .spawn((
                        Other { feet, target: feet },
                        sprite,
                        Anchor::BOTTOM_CENTER,
                        Transform::from_translation(player::translation(feet)),
                    ))
                    .id();
                others.insert(id, entity);
            }
            Update::Left(id) => {
                if let Some(entity) = others.remove(&id) {
                    commands.entity(entity).despawn();
                }
            }
            Update::Reset => {
                for (_, entity) in others.drain() {
                    commands.entity(entity).despawn();
                }
            }
        }
    }
}

fn walk_others(time: Res<Time>, mut others: Query<(&mut Other, &mut Transform)>) {
    for (mut other, mut transform) in &mut others {
        let to_go = other.target - other.feet;
        if to_go.length() > SNAP {
            other.feet = other.target;
        } else {
            let step = SPEED * time.delta_secs();
            other.feet = if to_go.length() <= step {
                other.target
            } else {
                other.feet + to_go.normalize() * step
            };
        }
        transform.translation = player::translation(other.feet);
    }
}

/// A color per player, so they can tell each other apart until there are real characters.
fn tint(id: u32) -> Color {
    Color::hsl((id % 360) as f32, 0.7, 0.75)
}
