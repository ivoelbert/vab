//! A placeholder player that walks the bar with the arrow keys or WASD. It can stand on
//! floor tiles without an object, and slides along whatever blocks it.

use std::collections::HashSet;

use bevy::prelude::*;
use bevy::sprite::Anchor;
use world::{Map, TILE_HEIGHT, cell_to_world, world_to_cell};

use crate::Mode;
use crate::room;

/// Walking speed in world pixels per second.
const SPEED: f32 = 64.0;
/// Roughly how many world pixels tall the view is; the zoom is the whole number closest to it.
const VIEW_HEIGHT: f32 = 270.0;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (walk.run_if(in_state(Mode::Walking)), follow).chain(),
        );
    }
}

/// Cells the player can stand on: floor without an object on it.
#[derive(Resource)]
pub struct Walkable(HashSet<IVec2>);

impl Walkable {
    pub fn from_map(map: &Map) -> Self {
        let blocked: HashSet<IVec2> = map.objects.iter().map(|p| IVec2::new(p.x, p.y)).collect();
        Self(
            map.floor
                .iter()
                .map(|p| IVec2::new(p.x, p.y))
                .filter(|cell| !blocked.contains(cell))
                .collect(),
        )
    }

    fn allows(&self, feet: Vec2) -> bool {
        self.0.contains(&world_to_cell(feet))
    }

    /// The free cell nearest the middle of the bar.
    fn start(&self) -> Option<IVec2> {
        let middle = self.0.iter().map(|cell| cell.as_vec2()).sum::<Vec2>() / self.0.len() as f32;
        self.0.iter().copied().min_by(|a, b| {
            let da = a.as_vec2().distance_squared(middle);
            let db = b.as_vec2().distance_squared(middle);
            da.total_cmp(&db).then(a.x.cmp(&b.x)).then(a.y.cmp(&b.y))
        })
    }
}

#[derive(Component)]
pub struct Player {
    /// Where the player's feet are, in world pixels.
    pub feet: Vec2,
}

pub fn spawn_player(commands: &mut Commands, asset_server: &AssetServer, walkable: &Walkable) {
    let Some(cell) = walkable.start() else {
        return;
    };
    let feet = cell_to_world(cell.x, cell.y);
    room::room_move(feet.x, feet.y, false);
    commands.spawn((
        Player { feet },
        Sprite::from_image(asset_server.load("characters/player.png")),
        Anchor::BOTTOM_CENTER,
        Transform::from_translation(translation(feet)),
    ));
}

fn walk(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    walkable: Res<Walkable>,
    mut players: Query<(&mut Player, &mut Transform, &mut Sprite)>,
) {
    let mut direction = Vec2::ZERO;
    for (key_a, key_b, towards) in [
        (KeyCode::ArrowUp, KeyCode::KeyW, Vec2::Y),
        (KeyCode::ArrowDown, KeyCode::KeyS, Vec2::NEG_Y),
        (KeyCode::ArrowLeft, KeyCode::KeyA, Vec2::NEG_X),
        (KeyCode::ArrowRight, KeyCode::KeyD, Vec2::X),
    ] {
        if keys.any_pressed([key_a, key_b]) {
            direction += towards;
        }
    }
    if direction == Vec2::ZERO {
        return;
    }
    // Up and down at half speed, so walking looks even on the 2:1 isometric floor.
    let step = direction.normalize() * Vec2::new(1.0, 0.5) * SPEED * time.delta_secs();

    for (mut player, mut transform, mut sprite) in &mut players {
        // The whole step if it's free, otherwise slide along whichever axis is.
        for attempt in [step, Vec2::new(step.x, 0.0), Vec2::new(0.0, step.y)] {
            if attempt != Vec2::ZERO && walkable.allows(player.feet + attempt) {
                player.feet += attempt;
                break;
            }
        }
        if step.x != 0.0 {
            sprite.flip_x = step.x < 0.0;
        }
        transform.translation = translation(player.feet);
        room::room_move(player.feet.x, player.feet.y, sprite.flip_x);
    }
}

/// Snapped to whole pixels, with the same depth rule as map objects (lower on screen draws
/// on top), nudged ahead of objects on the same row.
pub fn translation(feet: Vec2) -> Vec3 {
    let row = -feet.y / (TILE_HEIGHT / 2.0);
    feet.round().extend(1.0 + row * 0.001 + 0.0005)
}

/// Keeps the player in the middle of the view, at a whole-number zoom so pixels stay square.
fn follow(
    window: Single<&Window>,
    player: Single<&Player>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    let (mut transform, mut projection) = camera.into_inner();
    // Aim at the player's middle rather than their feet.
    let target = (player.feet + Vec2::Y * TILE_HEIGHT).round();
    transform.translation = target.extend(transform.translation.z);
    if let Projection::Orthographic(ortho) = &mut *projection {
        ortho.scale = 1.0 / (window.height() / VIEW_HEIGHT).round().max(1.0);
    }
}
