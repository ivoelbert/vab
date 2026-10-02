//! Telling players the controls. A welcome panel with them shows on a first visit (the page
//! remembers it in a cookie) and again with /help in the chat; any key closes it. When a game
//! starts, a card lists its controls for a while, named the way the game names its buttons (the
//! core says, e.g. "Z  Low Punch"); /help while playing shows it again.

use std::cell::RefCell;

use bevy::input::keyboard::KeyboardInput;
use bevy::input::{ButtonState, InputSystems};
use bevy::prelude::*;
use wasm_bindgen::prelude::*;

use crate::Mode;
use crate::chat::type_in_chat;
use crate::emulator::{KEYS, PlayingGame};

/// How long a game's card stays up, in seconds.
const CARD_FOR: f32 = 12.0;
const KEY_COLOR: Color = Color::srgb(1.0, 0.82, 0.3);
const DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.6);

/// What to press, and what it does. Short enough not to wrap: keys are padded into a column
/// (the font is monospaced).
const IN_THE_BAR: [(&str, &str); 6] = [
    ("Arrows / W A S D", "Walk"),
    ("E", "Play the cabinet you're next to"),
    ("F", "Watch the game being played there"),
    ("Y", "Chat (Enter sends)"),
    ("/name Mauri", "Set your name, in the chat"),
    ("/help", "These controls, in the chat"),
];
const AT_A_CABINET: [(&str, &str); 7] = [
    ("5", "Insert a coin"),
    ("1", "Start"),
    ("Arrows", "Move"),
    ("Z X C  A S D", "Buttons, listed when a game starts"),
    ("M", "Your microphone, playing with others"),
    ("Shift+1-4", "Mute player 1-4 (or click them)"),
    ("Esc", "Stand up"),
];
const KEY_COLUMN: usize = 18;
/// The RetroPad's Start (libretro.h).
const START: u16 = 3;

thread_local! {
    static WELCOME: RefCell<bool> = const { RefCell::new(false) };
    static BUTTONS: RefCell<Option<Vec<(u16, String)>>> = const { RefCell::new(None) };
}

/// Called by index.html on a first visit.
#[wasm_bindgen]
pub fn help_welcome() {
    WELCOME.set(true);
}

/// Called by index.html with what the game being played calls its buttons, as lines of
/// "<RetroPad id>\t<name>" (from the core, web/emulator/libretro.js).
#[wasm_bindgen]
pub fn game_buttons(buttons: String) {
    let buttons = buttons
        .lines()
        .filter_map(|line| {
            let (id, name) = line.split_once('\t')?;
            Some((id.parse().ok()?, name.to_string()))
        })
        .collect();
    BUTTONS.set(Some(buttons));
}

/// Shows the controls: the welcome panel in the bar, the game's card while playing.
#[derive(Message)]
pub struct ShowHelp;

#[derive(Resource, Default)]
pub struct Help {
    open: bool,
    card_until: f32,
}

impl Help {
    /// The welcome panel is up, and keys are for closing it.
    pub fn is_open(&self) -> bool {
        self.open
    }
}

/// Run condition: the welcome panel isn't up.
pub fn help_closed(help: Res<Help>) -> bool {
    !help.open
}

pub struct HelpPlugin;

impl Plugin for HelpPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Help>()
            .add_message::<ShowHelp>()
            .add_systems(Startup, spawn_help)
            // After the chat has seen (and ignored) the key, so it doesn't open the chat too.
            .add_systems(
                PreUpdate,
                close_panel.after(InputSystems).after(type_in_chat),
            )
            .add_systems(Update, (show_help, list_buttons, show_card).chain())
            .add_systems(OnExit(Mode::Playing), |mut help: ResMut<Help>| {
                help.card_until = 0.0
            });
    }
}

#[derive(Component)]
struct Panel;

#[derive(Component)]
struct Card;

fn spawn_help(mut commands: Commands) {
    commands
        .spawn((
            Panel,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            // Over everything, the chat too.
            GlobalZIndex(3),
            Visibility::Hidden,
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(14.0),
                        padding: UiRect::all(Val::Px(20.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.07, 0.07, 0.1)),
                    BorderColor::all(KEY_COLOR),
                ))
                .with_children(|panel| {
                    panel.spawn(text("Welcome to the Arcade Bar", 20.0, KEY_COLOR));
                    // One block of text, so the columns line up.
                    panel
                        .spawn(text("", 14.0, Color::WHITE))
                        .with_children(|body| {
                            let sections: [(&str, &[(&str, &str)]); 2] =
                                [("In the bar", &IN_THE_BAR), ("At a cabinet", &AT_A_CABINET)];
                            for (i, (heading, rows)) in sections.into_iter().enumerate() {
                                let gap = if i == 0 { "" } else { "\n" };
                                body.spawn(span(&format!("{gap}{heading}\n"), DIM));
                                for (key, what) in rows {
                                    body.spawn(span(&format!("{key:<KEY_COLUMN$}"), KEY_COLOR));
                                    body.spawn(span(&format!("{what}\n"), Color::WHITE));
                                }
                            }
                            body.spawn(span(
                                "\nFriends who sit at your cabinet play with you.",
                                DIM,
                            ));
                        });
                    panel.spawn(text("Press any key to play", 14.0, KEY_COLOR));
                });
        });

    commands.spawn((
        Card,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            right: Val::Px(8.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(3.0),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
        // Over the game.
        GlobalZIndex(2),
        Visibility::Hidden,
    ));
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

/// A piece of a 14 px text block, in its own color.
fn span(content: &str, color: Color) -> impl Bundle {
    (
        TextSpan::new(content),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        TextColor(color),
    )
}

/// A key and what it does, the keys lined up in a column `key_width` wide.
fn row(parent: &mut ChildSpawnerCommands, key: &str, what: &str, key_width: f32, size: f32) {
    parent.spawn(Node::default()).with_children(|row| {
        row.spawn((
            text(key, size, KEY_COLOR),
            Node {
                width: Val::Px(key_width),
                flex_shrink: 0.0,
                ..default()
            },
        ));
        row.spawn(text(what, size, Color::WHITE));
    });
}

/// Any key closes the welcome panel, and does nothing else.
fn close_panel(
    mut keyboard: MessageReader<KeyboardInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut help: ResMut<Help>,
) {
    for key in keyboard.read() {
        if help.open && key.state == ButtonState::Pressed {
            help.open = false;
            keys.clear_just_pressed(key.key_code);
        }
    }
}

fn show_help(
    mut asked: MessageReader<ShowHelp>,
    mut help: ResMut<Help>,
    mode: Res<State<Mode>>,
    game: Option<Res<PlayingGame>>,
    time: Res<Time>,
    mut panel: Single<&mut Visibility, With<Panel>>,
) {
    let welcome = WELCOME.take();
    // Watching, there are no buttons to list: the panel instead.
    let playing = *mode.get() == Mode::Playing && game.is_some_and(|game| !game.watching);
    for _ in asked.read() {
        if playing {
            help.card_until = time.elapsed_secs() + CARD_FOR;
        } else {
            help.open = true;
        }
    }
    help.open |= welcome;
    **panel = if help.open {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
}

/// Fills the card with the game's controls once the page says what its buttons are, and
/// shows it for a while.
fn list_buttons(
    mut commands: Commands,
    mut help: ResMut<Help>,
    game: Option<Res<PlayingGame>>,
    time: Res<Time>,
    card: Single<Entity, With<Card>>,
) {
    let Some(buttons) = BUTTONS.take() else {
        return;
    };
    let named = |id: u16| {
        buttons
            .iter()
            .find(|(button, _)| *button == id)
            .map(|(_, name)| name.as_str())
    };
    let title = game.map(|game| game.title.clone()).unwrap_or_default();
    let arrow = |key: KeyCode| {
        matches!(
            key,
            KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::ArrowLeft | KeyCode::ArrowRight
        )
    };
    let coin_or_start = |key: KeyCode| matches!(key, KeyCode::Digit5 | KeyCode::Digit1);
    commands
        .entity(*card)
        .despawn_children()
        .with_children(|card| {
            card.spawn(text(&title, 14.0, KEY_COLOR));
            // Coin and Start first, then the stick, then the buttons.
            for (key, id) in KEYS.into_iter().filter(|(key, _)| coin_or_start(*key)) {
                if let Some(name) = named(id) {
                    row(card, key_name(key), name, 70.0, 13.0);
                }
            }
            // Sunset Riders has no Start: a coin, then a button, joins.
            let first_button = KEYS
                .into_iter()
                .find(|(key, id)| !arrow(*key) && !coin_or_start(*key) && named(*id).is_some());
            if let (None, Some((key, _))) = (named(START), first_button) {
                let join = format!("No Start: coin, then {}", key_name(key));
                row(card, "", &join, 70.0, 13.0);
            }
            if KEYS
                .into_iter()
                .any(|(key, id)| arrow(key) && named(id).is_some())
            {
                row(card, "Arrows", "Move", 70.0, 13.0);
            }
            for (key, id) in KEYS
                .into_iter()
                .filter(|(key, _)| !arrow(*key) && !coin_or_start(*key))
            {
                if let Some(name) = named(id) {
                    row(card, key_name(key), name, 70.0, 13.0);
                }
            }
            row(card, "Esc", "Stand up", 70.0, 13.0);
            row(card, "Y", "Chat", 70.0, 13.0);
        });
    help.card_until = time.elapsed_secs() + CARD_FOR;
}

fn show_card(
    help: Res<Help>,
    mode: Res<State<Mode>>,
    time: Res<Time>,
    mut card: Single<&mut Visibility, With<Card>>,
) {
    let shown = *mode.get() == Mode::Playing && time.elapsed_secs() < help.card_until;
    **card = if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
}

/// The label on a key the game uses.
fn key_name(key: KeyCode) -> &'static str {
    match key {
        KeyCode::Digit1 => "1",
        KeyCode::Digit5 => "5",
        KeyCode::KeyA => "A",
        KeyCode::KeyS => "S",
        KeyCode::KeyD => "D",
        KeyCode::KeyZ => "Z",
        KeyCode::KeyX => "X",
        KeyCode::KeyC => "C",
        _ => "?",
    }
}
