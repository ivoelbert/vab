//! Talking with the others at your cabinet. The page runs the microphones over the players'
//! WebRTC links (web/index.html); here a panel under the status line lists everyone at the
//! cabinet while there's someone to talk to, like a voice channel: a dot in their color, ringed
//! green while they talk, and whether they're muted or have their microphone off. Clicking
//! someone mutes or unmutes them (yourself: your microphone), as does Shift with their player
//! number; M turns your microphone off or on. Muting is only on your side: the others still
//! hear each other.

use std::cell::RefCell;

use bevy::prelude::*;
use wasm_bindgen::prelude::*;

use crate::Mode;
use crate::chat::chat_closed;
use crate::room::tint;

const TALKING: Color = Color::srgb(0.25, 0.85, 0.4);
const MUTED: Color = Color::srgb(0.95, 0.4, 0.4);
const DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.6);
const HOVERED: Color = Color::srgba(1.0, 1.0, 1.0, 0.12);
/// Shift with one of these mutes player 1-4.
const SEAT_KEYS: [KeyCode; 4] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
];

/// Someone at the cabinet, as the page says.
struct Person {
    id: u32,
    seat: u32,
    name: String,
    flags: String,
}

impl Person {
    fn has(&self, flag: char) -> bool {
        self.flags.contains(flag)
    }
}

thread_local! {
    static PEOPLE: RefCell<Option<Vec<Person>>> = const { RefCell::new(None) };
}

/// Called by index.html when who's at the cabinet, who's talking or who's muted changes: a line
/// per person, "id\tseat\tname\tflags" in seat order. Flags: y you, t talking, m muted by you,
/// o microphone off, b microphone blocked, n no voice (no direct connection to them). Empty
/// while there's nobody to talk to.
#[wasm_bindgen]
pub fn voice_people(lines: String) {
    let people = lines
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            Some(Person {
                id: fields.next()?.parse().ok()?,
                seat: fields.next()?.parse().ok()?,
                name: fields.next()?.to_string(),
                flags: fields.next().unwrap_or_default().to_string(),
            })
        })
        .collect();
    PEOPLE.set(Some(people));
}

// Defined in index.html.
#[wasm_bindgen]
extern "C" {
    /// Mutes or unmutes another player; with your own id, turns your microphone off or on.
    #[wasm_bindgen(js_name = voiceToggle)]
    fn voice_toggle(id: u32);
    /// The same for whoever plays at `seat` (0 for player 1).
    #[wasm_bindgen(js_name = voiceToggleSeat)]
    fn voice_toggle_seat(seat: u32);
    #[wasm_bindgen(js_name = voiceToggleMic)]
    fn voice_toggle_mic();
}

pub struct VoicePlugin;

impl Plugin for VoicePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Mode::Playing), spawn_panel)
            .add_systems(
                Update,
                (show_people, click, press_keys.run_if(chat_closed))
                    .run_if(in_state(Mode::Playing)),
            )
            .add_systems(OnExit(Mode::Playing), despawn_panel);
    }
}

#[derive(Component)]
struct Panel;

/// A person's line in the panel, which toggles muting them when clicked.
#[derive(Component)]
struct Row(u32);

fn spawn_panel(mut commands: Commands) {
    commands.spawn((
        Panel,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(36.0),
            left: Val::Px(8.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            padding: UiRect::all(Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
        // Over the game screen.
        GlobalZIndex(2),
        Visibility::Hidden,
    ));
}

fn despawn_panel(mut commands: Commands, panel: Single<Entity, With<Panel>>) {
    commands.entity(*panel).despawn();
}

/// Lists everyone again whenever the page says something changed.
fn show_people(mut commands: Commands, panel: Single<(Entity, &mut Visibility), With<Panel>>) {
    let Some(people) = PEOPLE.take() else {
        return;
    };
    let (panel, mut visibility) = panel.into_inner();
    *visibility = if people.is_empty() {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    commands
        .entity(panel)
        .despawn_children()
        .with_children(|panel| {
            for person in &people {
                spawn_row(panel, person);
            }
            panel.spawn(text("Click or Shift+1-4 mutes, M your mic", 11.0, DIM));
        });
}

fn spawn_row(panel: &mut ChildSpawnerCommands, person: &Person) {
    let you = person.has('y');
    let name = if you {
        format!("{} (you)", person.name)
    } else {
        person.name.clone()
    };
    let tag = if you {
        match (person.has('b'), person.has('o')) {
            (true, _) => Some(("mic blocked", MUTED)),
            (_, true) => Some(("mic off", MUTED)),
            _ => None,
        }
    } else if person.has('m') {
        Some(("muted", MUTED))
    } else if person.has('n') {
        Some(("no voice", DIM))
    } else if person.has('o') {
        Some(("mic off", DIM))
    } else {
        None
    };
    let quiet = !you && person.has('m');
    panel
        .spawn((
            Button,
            Row(person.id),
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|row| {
            // Their color, ringed while they talk.
            row.spawn((
                Node {
                    width: Val::Px(14.0),
                    height: Val::Px(14.0),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(tint(person.id)),
                BorderColor::all(if person.has('t') {
                    TALKING
                } else {
                    Color::NONE
                }),
            ));
            row.spawn(text(&format!("P{}", person.seat + 1), 13.0, DIM));
            row.spawn(text(&name, 13.0, if quiet { DIM } else { Color::WHITE }));
            if let Some((tag, color)) = tag {
                row.spawn(text(tag, 13.0, color));
            }
        });
}

fn text(content: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

fn click(mut rows: Query<(&Interaction, &Row, &mut BackgroundColor), Changed<Interaction>>) {
    for (interaction, row, mut background) in &mut rows {
        match interaction {
            Interaction::Pressed => voice_toggle(row.0),
            Interaction::Hovered => background.0 = HOVERED,
            Interaction::None => background.0 = Color::NONE,
        }
    }
}

fn press_keys(keys: Res<ButtonInput<KeyCode>>) {
    if keys.just_pressed(KeyCode::KeyM) {
        voice_toggle_mic();
    }
    if !keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        return;
    }
    for (seat, key) in SEAT_KEYS.into_iter().enumerate() {
        if keys.just_pressed(key) {
            voice_toggle_seat(seat as u32);
        }
    }
}
