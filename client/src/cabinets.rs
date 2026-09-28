//! Cabinets with a game (assigned in the editor): next to one, a hint shows its title and E
//! starts it.

use bevy::prelude::*;
use world::{Game, Map, cell_to_world, games_from_ron, world_to_cell};

use crate::Mode;
use crate::emulator;
use crate::player::Player;

/// The games cabinets can run, built into the client like the map.
const GAMES: &str = include_str!("../../assets/games.ron");
/// Where the hint sits: a little above a cabinet's top, in world pixels from its cell.
const HINT_HEIGHT: f32 = 44.0;

pub struct CabinetsPlugin;

impl Plugin for CabinetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hint)
            .add_systems(Update, (show_hint, play).run_if(in_state(Mode::Walking)))
            .add_systems(OnEnter(Mode::Playing), hide_hint);
    }
}

/// Cabinets that have a game, with their cells.
#[derive(Resource)]
pub struct Cabinets(Vec<(IVec2, Game)>);

impl Cabinets {
    /// Cabinets whose game isn't in assets/games.ron are left out.
    pub fn from_map(map: &Map) -> Self {
        let games = games_from_ron(GAMES).expect("assets/games.ron is a valid game list");
        Self(
            map.objects
                .iter()
                .filter_map(|placed| {
                    let rom = placed.game.as_ref()?;
                    let game = games.iter().find(|game| &game.rom == rom)?;
                    Some((IVec2::new(placed.x, placed.y), game.clone()))
                })
                .collect(),
        )
    }

    /// The nearest cabinet in one of the 8 cells around the player's feet.
    fn next_to(&self, feet: Vec2) -> Option<&(IVec2, Game)> {
        let cell = world_to_cell(feet);
        self.0
            .iter()
            .filter(|(cabinet, _)| (*cabinet - cell).abs().max_element() == 1)
            .min_by(|(a, _), (b, _)| {
                let da = cell_to_world(a.x, a.y).distance_squared(feet);
                let db = cell_to_world(b.x, b.y).distance_squared(feet);
                da.total_cmp(&db)
            })
    }
}

#[derive(Component)]
struct Hint;

fn spawn_hint(mut commands: Commands) {
    commands.spawn((
        Hint,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
        Visibility::Hidden,
    ));
}

/// Shows the hint above the cabinet next to the player, centered on it.
fn show_hint(
    cabinets: Res<Cabinets>,
    player: Single<&Player>,
    camera: Single<(&Camera, &GlobalTransform)>,
    hint: Single<(&mut Text, &mut Node, &mut Visibility, &ComputedNode), With<Hint>>,
) {
    let (mut text, mut node, mut visibility, computed) = hint.into_inner();
    let (camera, camera_transform) = *camera;
    let Some((cell, game)) = cabinets.next_to(player.feet) else {
        *visibility = Visibility::Hidden;
        return;
    };
    let above = cell_to_world(cell.x, cell.y) + Vec2::Y * HINT_HEIGHT;
    let Ok(on_screen) = camera.world_to_viewport(camera_transform, above.extend(0.0)) else {
        return;
    };
    let label = format!("E  {}", game.title);
    if text.0 != label {
        text.0 = label;
    }
    let size = computed.size() * computed.inverse_scale_factor();
    node.left = Val::Px((on_screen.x - size.x / 2.0).round());
    node.top = Val::Px((on_screen.y - size.y).round());
    *visibility = Visibility::Visible;
}

fn play(
    keys: Res<ButtonInput<KeyCode>>,
    cabinets: Res<Cabinets>,
    player: Single<&Player>,
    mut mode: ResMut<NextState<Mode>>,
) {
    if !keys.just_pressed(KeyCode::KeyE) {
        return;
    }
    if let Some((_, game)) = cabinets.next_to(player.feet) {
        emulator::play(game);
        mode.set(Mode::Playing);
    }
}

fn hide_hint(mut hint: Single<&mut Visibility, With<Hint>>) {
    **hint = Visibility::Hidden;
}
