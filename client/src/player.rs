//! A placeholder player that walks the bar with the arrow keys or WASD, or a thumb dragged on
//! a touch screen (touch.rs). It can stand on floor tiles without an object, and slides along
//! whatever blocks it.

use std::collections::HashSet;

use bevy::prelude::*;
use bevy::sprite::Anchor;
use world::{Map, TILE_HEIGHT, cell_to_world, world_to_cell};

use crate::Mode;
use crate::chat::chat_closed;
use crate::help::help_closed;
use crate::room;
use crate::touch::Touch;

/// Walking speed in world pixels per second.
const SPEED: f32 = 64.0;
/// Roughly how many world pixels tall the view is; the zoom is the whole number closest to it.
const VIEW_HEIGHT: f32 = 270.0;
/// In a window much taller than wide (a phone held upright), how many it is wide instead.
const VIEW_WIDTH: f32 = 240.0;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Zoom(1.0)).add_systems(
            Update,
            (
                walk.run_if(
                    in_state(Mode::Walking)
                        .and_then(chat_closed)
                        .and_then(help_closed),
                ),
                follow,
            )
                .chain(),
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

/// How many of the screen's own pixels a world pixel takes up: a whole number, set from the
/// window's size.
#[derive(Resource)]
pub struct Zoom(pub f32);

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
        Transform::from_translation(translation(feet, 1.0)),
    ));
}

fn walk(
    keys: Res<ButtonInput<KeyCode>>,
    touch: Res<Touch>,
    time: Res<Time>,
    walkable: Res<Walkable>,
    mut players: Query<(&mut Player, &mut Sprite)>,
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
    // A thumb walks the way it points on the screen, which the halving below would flatten.
    direction += touch.stick() * Vec2::new(1.0, 2.0);
    if direction == Vec2::ZERO {
        return;
    }
    // Up and down at half speed, so walking looks even on the 2:1 isometric floor.
    let step = direction.normalize() * Vec2::new(1.0, 0.5) * SPEED * time.delta_secs();

    for (mut player, mut sprite) in &mut players {
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
        room::room_move(player.feet.x, player.feet.y, sprite.flip_x);
    }
}

/// Snapped to whole screen pixels, with the same depth rule as map objects (lower on screen
/// draws on top), nudged ahead of objects on the same row. Snapping to screen pixels rather
/// than world pixels keeps diagonal walking smooth: x and y cross world pixels on different
/// frames, which shakes the view by a whole world pixel at a time.
pub fn translation(feet: Vec2, zoom: f32) -> Vec3 {
    let row = -feet.y / (TILE_HEIGHT / 2.0);
    snap(feet, zoom).extend(1.0 + row * 0.001 + 0.0005)
}

fn snap(position: Vec2, zoom: f32) -> Vec2 {
    (position * zoom).round() / zoom
}

/// Keeps the player in the middle of the view, at a whole number of the screen's own pixels to
/// each of the world's so they stay square (a phone has three or so to a logical one).
fn follow(
    window: Single<&Window>,
    mut zoom: ResMut<Zoom>,
    player: Single<(&Player, &mut Transform), Without<Camera2d>>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    let tall = window.physical_height() as f32 / VIEW_HEIGHT;
    let wide = window.physical_width() as f32 / VIEW_WIDTH;
    let scale = tall.min(wide).round().max(1.0);
    if zoom.0 != scale {
        zoom.0 = scale;
    }
    let (player, mut player_transform) = player.into_inner();
    player_transform.translation = translation(player.feet, scale);
    let (mut transform, mut projection) = camera.into_inner();
    // Aim at the player's middle rather than their feet, snapped the same way so they stay put.
    let target = snap(player.feet + Vec2::Y * TILE_HEIGHT, scale);
    transform.translation = target.extend(transform.translation.z);
    if let Projection::Orthographic(ortho) = &mut *projection {
        ortho.scale = window.scale_factor() / scale;
    }
}
