//! Everyone else in the bar room, as the page hears about them (web/room.js): each walks to
//! where the room last said they are. The player's own position goes out through `roomMove`.
//! Everyone, the player too, has their name above their head.

use std::cell::RefCell;
use std::collections::HashMap;

use bevy::prelude::*;
use bevy::sprite::Anchor;
use wasm_bindgen::prelude::*;

use crate::player::{self, Player, Zoom};

/// A little faster than walking, so others catch up with where they said they are.
const SPEED: f32 = 80.0;
/// Further than this and they jump there (they sat down, or the connection dropped for a bit).
const SNAP: f32 = 48.0;
/// Where a name sits: a little above a character's head, in world pixels from their feet.
const NAME_HEIGHT: f32 = 28.0;

enum Update {
    Moved {
        id: u32,
        feet: Vec2,
        flip: bool,
        name: String,
    },
    Left(u32),
    Reset,
}

thread_local! {
    static UPDATES: RefCell<Vec<Update>> = const { RefCell::new(Vec::new()) };
    static OWN_NAME: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Called by index.html when a player shows up, moves or changes name; `x`, `y` are their feet
/// in world pixels.
#[wasm_bindgen]
pub fn player_moved(id: u32, x: f32, y: f32, flip: bool, name: String) {
    let feet = Vec2::new(x, y);
    UPDATES.with_borrow_mut(|updates| {
        updates.push(Update::Moved {
            id,
            feet,
            flip,
            name,
        })
    });
}

/// Called by index.html with the player's own name, when they join and when they change it.
#[wasm_bindgen]
pub fn own_name(name: String) {
    OWN_NAME.set(Some(name));
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
        app.add_systems(
            Update,
            (apply_updates, walk_others, name_player, place_names).chain(),
        );
    }
}

/// Another player: where they are drawn and where the room says they are.
#[derive(Component)]
struct Other {
    feet: Vec2,
    target: Vec2,
}

/// The UI text with a character's name, kept above their head (`place_names`).
#[derive(Component)]
struct NameTag(Entity);

#[derive(Component)]
struct NameText;

fn spawn_name(commands: &mut Commands, name: String) -> Entity {
    commands
        .spawn((
            NameText,
            Text::new(name),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(Color::WHITE),
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::axes(Val::Px(3.0), Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
        ))
        .id()
}

fn apply_updates(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    zoom: Res<Zoom>,
    mut others: Local<HashMap<u32, Entity>>,
    mut query: Query<(&mut Other, &mut Sprite, &NameTag)>,
    mut names: Query<&mut Text, With<NameText>>,
) {
    for update in UPDATES.take() {
        match update {
            Update::Moved {
                id,
                feet,
                flip,
                name,
            } => {
                if let Some(&entity) = others.get(&id) {
                    if let Ok((mut other, mut sprite, tag)) = query.get_mut(entity) {
                        other.target = feet;
                        sprite.flip_x = flip;
                        if let Ok(mut text) = names.get_mut(tag.0)
                            && text.0 != name
                        {
                            text.0 = name;
                        }
                        continue;
                    }
                    // Spawned earlier in this same batch: replace what it started with.
                    commands.entity(entity).insert(Other { feet, target: feet });
                    continue;
                }
                let mut sprite = Sprite::from_image(asset_server.load("characters/player.png"));
                sprite.color = tint(id);
                sprite.flip_x = flip;
                let tag = NameTag(spawn_name(&mut commands, name));
                let entity = commands
                    .spawn((
                        Other { feet, target: feet },
                        sprite,
                        Anchor::BOTTOM_CENTER,
                        Transform::from_translation(player::translation(feet, zoom.0)),
                        tag,
                    ))
                    .id();
                others.insert(id, entity);
            }
            Update::Left(id) => {
                if let Some(entity) = others.remove(&id) {
                    despawn_with_name(&mut commands, entity, &query);
                }
            }
            Update::Reset => {
                for (_, entity) in others.drain() {
                    despawn_with_name(&mut commands, entity, &query);
                }
            }
        }
    }
}

fn despawn_with_name(
    commands: &mut Commands,
    entity: Entity,
    query: &Query<(&mut Other, &mut Sprite, &NameTag)>,
) {
    if let Ok((_, _, tag)) = query.get(entity) {
        commands.entity(tag.0).despawn();
    }
    commands.entity(entity).despawn();
}

/// Puts the player's own name above their head once the page knows it, and when it changes.
fn name_player(
    mut commands: Commands,
    player: Single<(Entity, Option<&NameTag>), With<Player>>,
    mut names: Query<&mut Text, With<NameText>>,
) {
    let Some(name) = OWN_NAME.take() else {
        return;
    };
    let (entity, tag) = *player;
    match tag.and_then(|tag| names.get_mut(tag.0).ok()) {
        Some(mut text) => text.0 = name,
        None => {
            let tag = NameTag(spawn_name(&mut commands, name));
            commands.entity(entity).insert(tag);
        }
    }
}

/// Keeps each name centered above its character's head.
fn place_names(
    camera: Single<(&Camera, &GlobalTransform)>,
    tagged: Query<(&Transform, &NameTag)>,
    mut names: Query<(&mut Node, &ComputedNode), With<NameText>>,
) {
    let (camera, camera_transform) = *camera;
    for (transform, tag) in &tagged {
        let Ok((mut node, computed)) = names.get_mut(tag.0) else {
            continue;
        };
        let head = transform.translation.truncate() + Vec2::Y * NAME_HEIGHT;
        let Ok(on_screen) = camera.world_to_viewport(camera_transform, head.extend(0.0)) else {
            continue;
        };
        let size = computed.size() * computed.inverse_scale_factor();
        node.left = Val::Px((on_screen.x - size.x / 2.0).round());
        node.top = Val::Px((on_screen.y - size.y).round());
    }
}

fn walk_others(time: Res<Time>, zoom: Res<Zoom>, mut others: Query<(&mut Other, &mut Transform)>) {
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
        transform.translation = player::translation(other.feet, zoom.0);
    }
}

/// A color per player, so they can tell each other apart until there are real characters.
fn tint(id: u32) -> Color {
    Color::hsl((id % 360) as f32, 0.7, 0.75)
}
