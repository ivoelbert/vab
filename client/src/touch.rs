//! Touch controls, for phones and tablets: the page says when the screen is one
//! (web/index.html). A thumb dragged anywhere off the buttons is the stick: it walks the bar,
//! and moves in a game. Buttons are UI nodes with a [`TouchButton`], spawned by whoever knows
//! what they do, who then asks [`Touch`] about them: Play and Watch by a cabinet (cabinets.rs)
//! and Leave at one (emulator.rs). The game's own buttons are here: the pad, under the right
//! thumb, laid out like the keys and named as the game names them. The chat is the page's, as
//! a canvas can't bring up a phone's keyboard.

use bevy::input::InputSystems;
use bevy::prelude::*;
use wasm_bindgen::prelude::*;

use crate::Mode;
use crate::emulator::PlayingGame;
use crate::help::{GameButtons, Help};

/// How far the stick goes from where the thumb landed, and how far before it counts, in
/// logical pixels.
const REACH: f32 = 40.0;
const DEAD: f32 = 10.0;
const KNOB: f32 = 44.0;
/// The pad's buttons across, and the space between them.
const BUTTON: f32 = 60.0;
const GAP: f32 = 10.0;
/// The RetroPad buttons (libretro.h) under the right thumb, in the rows the keys are in
/// (emulator.rs): A S D over Z X C. A game shows the ones it names.
const PAD_ROWS: [[u16; 3]; 2] = [[1, 9, 10], [0, 8, 11]];
/// The RetroPad's Select, a coin on a cabinet, and its Start.
const COIN: u16 = 2;
const START: u16 = 3;
const UP: u16 = 4;
const DOWN: u16 = 5;
const LEFT: u16 = 6;
const RIGHT: u16 = 7;

const FILL: Color = Color::srgba(1.0, 1.0, 1.0, 0.04);
const PRESSED: Color = Color::srgba(1.0, 1.0, 1.0, 0.3);
const LINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.5);

// Defined in index.html.
#[wasm_bindgen]
extern "C" {
    /// The screen is one for fingers, without a keyboard to count on.
    #[wasm_bindgen(js_name = touchScreen)]
    fn touch_screen() -> bool;
}

/// A button for a finger, and what it's for.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TouchButton {
    Play,
    Watch,
    Leave,
    /// One of the game's, by RetroPad id.
    Pad(u16),
}

/// What the fingers on the screen are doing.
#[derive(Resource, Default)]
pub struct Touch {
    on: bool,
    thumb: Option<Thumb>,
    /// Buttons with a finger on them, and those a finger landed on this frame.
    held: Vec<TouchButton>,
    tapped: Vec<TouchButton>,
}

/// The finger on the stick: where the stick's middle is, and where the finger is.
struct Thumb {
    id: u64,
    from: Vec2,
    at: Vec2,
}

impl Touch {
    /// The screen is a touch screen, and shows these controls instead of naming keys.
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// A finger just landed on `button`.
    pub fn tapped(&self, button: TouchButton) -> bool {
        self.tapped.contains(&button)
    }

    fn pressed(&self, button: TouchButton) -> bool {
        self.held.contains(&button) || self.tapped(button)
    }

    /// Where the stick is pushed, as on the screen with up positive: up to 1 long, and zero
    /// while nearly centered.
    pub fn stick(&self) -> Vec2 {
        let Some(thumb) = &self.thumb else {
            return Vec2::ZERO;
        };
        let pull = (thumb.at - thumb.from) * Vec2::new(1.0, -1.0);
        if pull.length() < DEAD {
            Vec2::ZERO
        } else {
            pull / REACH
        }
    }

    /// The RetroPad mask of the stick, as a cabinet's 8-way one, and the pad's buttons.
    pub fn pad(&self) -> u16 {
        let stick = self.stick().normalize_or_zero();
        // Each way takes the 135 degrees around it, so two share the 45 of a diagonal.
        let lean = 22.5_f32.to_radians().sin();
        let ways = [
            (UP, stick.y > lean),
            (DOWN, stick.y < -lean),
            (LEFT, stick.x < -lean),
            (RIGHT, stick.x > lean),
        ];
        let buttons = self.held.iter().chain(&self.tapped).filter_map(|button| {
            let TouchButton::Pad(id) = button else {
                return None;
            };
            Some(*id)
        });
        ways.into_iter()
            .filter_map(|(id, pushed)| pushed.then_some(id))
            .chain(buttons)
            .fold(0, |mask, id| mask | 1 << id)
    }
}

pub struct TouchPlugin;

impl Plugin for TouchPlugin {
    fn build(&self, app: &mut App) {
        let on = touch_screen();
        app.insert_resource(Touch { on, ..default() });
        if !on {
            return;
        }
        app.add_systems(Startup, spawn_stick)
            // Before anything asks about the fingers this frame.
            .add_systems(PreUpdate, read_touches.after(InputSystems))
            .add_systems(
                Update,
                (
                    show_stick,
                    light_buttons,
                    lay_out_pad
                        .run_if(in_state(Mode::Playing).and_then(resource_changed::<GameButtons>)),
                ),
            )
            .add_systems(OnExit(Mode::Playing), remove_pad);
    }
}

pub fn read_touches(
    touches: Res<Touches>,
    window: Single<&Window>,
    help: Res<Help>,
    buttons: Query<(
        &TouchButton,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
    )>,
    mut touch: ResMut<Touch>,
) {
    touch.held.clear();
    touch.tapped.clear();
    // A tap closes the controls panel (help.rs), and does nothing else.
    if help.is_open() {
        touch.thumb = None;
        return;
    }
    let button_at = |position: Vec2| {
        let point = position * window.scale_factor();
        buttons
            .iter()
            .find(|(_, node, transform, visible)| {
                visible.get() && node.contains_point(**transform, point)
            })
            .map(|(button, ..)| *button)
    };
    for finger in touches.iter_just_pressed() {
        match button_at(finger.position()) {
            Some(button) => touch.tapped.push(button),
            // The first finger down off the buttons takes the stick.
            None if touch.thumb.is_none() => {
                let at = finger.position();
                touch.thumb = Some(Thumb {
                    id: finger.id(),
                    from: at,
                    at,
                });
            }
            None => {}
        }
    }
    touch.thumb = touch.thumb.take().and_then(|mut thumb| {
        thumb.at = touches.get_pressed(thumb.id)?.position();
        // Pulled past its reach the stick comes along, so the way back is never long.
        thumb.from = thumb.at - (thumb.at - thumb.from).clamp_length_max(REACH);
        Some(thumb)
    });
    // A button is held by a finger on it now, wherever that finger landed: thumbs roll from
    // one to the next.
    let stick = touch.thumb.as_ref().map(|thumb| thumb.id);
    for finger in touches.iter().filter(|finger| Some(finger.id()) != stick) {
        if let Some(button) = button_at(finger.position()) {
            touch.held.push(button);
        }
    }
}

/// A button for a finger: `label` in a rounded box.
pub fn button(button: TouchButton, label: &str, width: f32, height: f32) -> impl Bundle {
    (
        button,
        Node {
            width: Val::Px(width),
            height: Val::Px(height),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(FILL),
        BorderColor::all(LINE),
        children![(
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(Color::WHITE),
            TextLayout {
                justify: Justify::Center,
                ..default()
            },
        )],
    )
}

fn light_buttons(touch: Res<Touch>, mut buttons: Query<(&TouchButton, &mut BackgroundColor)>) {
    for (button, mut color) in &mut buttons {
        let fill = if touch.pressed(*button) {
            PRESSED
        } else {
            FILL
        };
        if color.0 != fill {
            color.0 = fill;
        }
    }
}

/// The stick as drawn while a thumb is on it: a ring around where it landed, and a disc
/// under it.
#[derive(Component, Clone, Copy)]
enum StickPart {
    Ring,
    Knob,
}

impl StickPart {
    fn across(self) -> f32 {
        match self {
            StickPart::Ring => REACH * 2.0 + KNOB,
            StickPart::Knob => KNOB,
        }
    }
}

fn spawn_stick(mut commands: Commands) {
    for part in [StickPart::Ring, StickPart::Knob] {
        let mut drawn = commands.spawn((
            part,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(part.across()),
                height: Val::Px(part.across()),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            // Over the game, under the controls panel.
            GlobalZIndex(2),
            Visibility::Hidden,
        ));
        match part {
            StickPart::Ring => drawn.insert(BorderColor::all(LINE)),
            StickPart::Knob => drawn.insert(BackgroundColor(PRESSED)),
        };
    }
}

fn show_stick(touch: Res<Touch>, mut parts: Query<(&StickPart, &mut Node, &mut Visibility)>) {
    for (part, mut node, mut visibility) in &mut parts {
        let Some(thumb) = &touch.thumb else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let middle = match part {
            StickPart::Ring => thumb.from,
            StickPart::Knob => thumb.at,
        };
        node.left = Val::Px(middle.x - part.across() / 2.0);
        node.top = Val::Px(middle.y - part.across() / 2.0);
        visibility.set_if_neq(Visibility::Inherited);
    }
}

/// The game's buttons, in the bottom-right corner.
#[derive(Component)]
struct Pad;

/// Lays out the pad once the page says what the game calls its buttons (not for a watcher).
fn lay_out_pad(
    mut commands: Commands,
    buttons: Res<GameButtons>,
    game: Option<Res<PlayingGame>>,
    pads: Query<Entity, With<Pad>>,
) {
    for pad in &pads {
        commands.entity(pad).despawn();
    }
    if game.is_none_or(|game| game.watching) {
        return;
    }
    let named = |id: u16| {
        buttons
            .0
            .iter()
            .find(|(button, _)| *button == id)
            .map(|(_, name)| name.as_str())
    };
    let row = || Node {
        column_gap: Val::Px(GAP),
        ..default()
    };
    commands
        .spawn((
            Pad,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(16.0),
                bottom: Val::Px(24.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::End,
                row_gap: Val::Px(GAP),
                ..default()
            },
            // Over the game.
            GlobalZIndex(2),
        ))
        .with_children(|pad| {
            pad.spawn(row()).with_children(|row| {
                for (id, label) in [(COIN, "Coin"), (START, "Start")] {
                    if named(id).is_some() {
                        row.spawn(button(TouchButton::Pad(id), label, BUTTON, 32.0));
                    }
                }
            });
            for ids in PAD_ROWS {
                if !ids.into_iter().any(|id| named(id).is_some()) {
                    continue;
                }
                pad.spawn(row()).with_children(|row| {
                    for id in ids {
                        if let Some(name) = named(id) {
                            // A word a line: "Low Punch" fits a round button that way.
                            let label = name.replace(' ', "\n");
                            row.spawn(button(TouchButton::Pad(id), &label, BUTTON, BUTTON));
                        }
                    }
                });
            }
        });
}

fn remove_pad(mut commands: Commands, pads: Query<Entity, With<Pad>>) {
    for pad in &pads {
        commands.entity(pad).despawn();
    }
}
