//! The room's chat: Y opens a line to type in, Enter sends it to everyone in the bar room and
//! Esc closes it. The latest messages show in the bottom-left corner for a while (all of them
//! while typing). `/name <name>` sets the name shown above your head and next to what you say;
//! the page keeps it in a cookie (web/index.html).

use std::cell::RefCell;
use std::collections::VecDeque;

use bevy::input::keyboard::KeyboardInput;
use bevy::input::{ButtonState, InputSystems};
use bevy::prelude::*;
use wasm_bindgen::prelude::*;

/// Lines kept and shown.
const LINES: usize = 8;
/// How long a line stays up when not typing, in seconds.
const SHOWN_FOR: f32 = 20.0;
/// The longest message and name; the room cuts them there too.
const MAX_MESSAGE: usize = 200;
const MAX_NAME: usize = 20;

thread_local! {
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Called by index.html with each message said in the room, ours included once the room has it.
#[wasm_bindgen]
pub fn chat_said(name: String, text: String) {
    SAID.with_borrow_mut(|said| said.push(format!("{name}: {text}")));
}

// Defined in index.html.
#[wasm_bindgen]
extern "C" {
    /// False when the page isn't connected to the room.
    #[wasm_bindgen(js_name = roomSay)]
    fn room_say(text: &str) -> bool;
    /// Sets the player's name in the room and remembers it for next time.
    #[wasm_bindgen(js_name = roomName)]
    fn room_name(name: &str);
}

pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Chat>()
            .add_systems(Startup, spawn_chat)
            // Before anything reads the keys this frame, so typing neither walks nor plays.
            .add_systems(PreUpdate, type_in_chat.after(InputSystems))
            .add_systems(Update, show_chat);
    }
}

#[derive(Resource, Default)]
pub struct Chat {
    open: bool,
    typing: String,
    /// Oldest first, with when each arrived (seconds since start).
    lines: VecDeque<(String, f32)>,
}

/// Run condition: the keyboard isn't typing in the chat.
pub fn chat_closed(chat: Res<Chat>) -> bool {
    !chat.open
}

impl Chat {
    pub fn is_open(&self) -> bool {
        self.open
    }

    fn add(&mut self, line: String, now: f32) {
        self.lines.push_back((line, now));
        if self.lines.len() > LINES {
            self.lines.pop_front();
        }
    }

    /// Sends what was typed: a message, or a command.
    fn send(&mut self, now: f32) {
        let typed = std::mem::take(&mut self.typing);
        let line = typed.trim();
        let command = line
            .split_whitespace()
            .next()
            .filter(|word| word.starts_with('/'));
        match command {
            None if !line.is_empty() => {
                if !room_say(line) {
                    self.add(
                        "* Not connected to the room, try again in a moment.".into(),
                        now,
                    );
                }
            }
            None => {}
            Some("/name") => {
                let name = line["/name".len()..]
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let name: String = name.chars().take(MAX_NAME).collect();
                if name.is_empty() {
                    self.add("* To pick a name: /name Mauri".into(), now);
                } else {
                    room_name(&name);
                    self.add(format!("* You're {name} now."), now);
                }
            }
            Some(_) => self.add("* The only command is /name, e.g. /name Mauri".into(), now),
        }
    }
}

fn type_in_chat(
    mut keyboard: MessageReader<KeyboardInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut chat: ResMut<Chat>,
    time: Res<Time>,
) {
    for key in keyboard.read() {
        if key.state != ButtonState::Pressed {
            continue;
        }
        if !chat.open {
            chat.open = key.key_code == KeyCode::KeyY;
            continue;
        }
        match key.key_code {
            KeyCode::Enter | KeyCode::NumpadEnter => {
                chat.send(time.elapsed_secs());
                chat.open = false;
            }
            KeyCode::Escape => {
                chat.typing.clear();
                chat.open = false;
                // It closed the chat; it doesn't also leave the game.
                keys.clear_just_pressed(KeyCode::Escape);
            }
            KeyCode::Backspace => {
                chat.typing.pop();
            }
            _ => {
                for c in key.text.iter().flat_map(|text| text.chars()) {
                    if !c.is_control() && chat.typing.chars().count() < MAX_MESSAGE {
                        chat.typing.push(c);
                    }
                }
            }
        }
    }
}

#[derive(Component)]
struct ChatLog;

/// The messages, apart from the line being typed.
type Log<'w, 's> = Single<
    'w,
    's,
    (&'static mut Text, &'static mut Visibility),
    (With<ChatLog>, Without<ChatLine>),
>;

#[derive(Component)]
struct ChatLine;

fn spawn_chat(mut commands: Commands) {
    let font = TextFont {
        font_size: FontSize::Px(14.0),
        ..default()
    };
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(8.0),
            bottom: Val::Px(8.0),
            max_width: Val::Percent(60.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Start,
            row_gap: Val::Px(4.0),
            ..default()
        },
        // Over the game too.
        GlobalZIndex(2),
        children![
            (
                ChatLog,
                Text::new(""),
                font.clone(),
                TextColor(Color::WHITE),
                Node {
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                Visibility::Hidden,
            ),
            (
                ChatLine,
                Text::new(""),
                font,
                TextColor(Color::WHITE),
                Node {
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            ),
        ],
    ));
}

fn show_chat(
    mut chat: ResMut<Chat>,
    time: Res<Time>,
    mut log: Log,
    mut line: Single<(&mut Text, &mut TextColor), With<ChatLine>>,
) {
    let now = time.elapsed_secs();
    for said in SAID.take() {
        chat.add(said, now);
    }
    let shown: Vec<&str> = chat
        .lines
        .iter()
        .filter(|(_, at)| chat.open || now - at < SHOWN_FOR)
        .map(|(text, _)| text.as_str())
        .collect();
    let text = shown.join("\n");
    let (log_text, log_visibility) = &mut *log;
    if log_text.0 != text {
        log_text.0 = text;
    }
    **log_visibility = if shown.is_empty() {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };

    let (typed, typed_color) = if !chat.open {
        ("Y  chat".to_string(), 0.5)
    } else if chat.typing.is_empty() {
        ("> Say something, or /name <your name>".to_string(), 0.6)
    } else {
        (format!("> {}_", chat.typing), 1.0)
    };
    let (line_text, line_color) = &mut *line;
    if line_text.0 != typed {
        line_text.0 = typed;
        line_color.0 = Color::srgba(1.0, 1.0, 1.0, typed_color);
    }
}
