//! Touch controls, for phones and tablets: the page says when the screen is one
//! (web/index.html). In the bar a thumb dragged anywhere off the buttons is the stick, and
//! walks. Buttons are UI nodes with a [`TouchButton`], spawned by whoever knows what they do,
//! who then asks [`Touch`] about them: Play and Watch by a cabinet (cabinets.rs) and Leave at
//! one (emulator.rs). The game's own controls are here: the pad, with its buttons under the
//! right thumb, laid out like the keys and named as the game names them, and a d-pad under the
//! left. The d-pad stays put, unlike the stick in the bar, so that each way is always in the
//! same place to tap twice or roll through, as a fighting game's moves ask. The chat is the
//! page's, as a canvas can't bring up a phone's keyboard.

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
/// The d-pad across, each of its arms, and how far outside it a thumb still lands on it.
const DPAD: f32 = 144.0;
const ARM: f32 = DPAD / 3.0;
const DPAD_SLOP: f32 = 24.0;
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
    /// On the d-pad, whose middle stays where it is.
    on_dpad: bool,
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

    /// The RetroPad mask of the d-pad, as a cabinet's 8-way stick, and the pad's buttons.
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
                    light_dpad,
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
    dpad: Query<(&ComputedNode, &UiGlobalTransform), With<Dpad>>,
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
    // The d-pad's middle and how far from it a thumb lands on it, while a game has one.
    let dpad = dpad.single().ok().map(|(node, transform)| {
        let scale = node.inverse_scale_factor();
        let reach = node.size().x * scale / 2.0 + DPAD_SLOP;
        (transform.translation * scale, reach)
    });
    for finger in touches.iter_just_pressed() {
        let at = finger.position();
        match button_at(at) {
            Some(button) => touch.tapped.push(button),
            None if touch.thumb.is_none() => {
                // The stick is the d-pad where there is one, for the thumb that lands on it.
                // In the bar it is wherever the first finger off the buttons lands.
                let from = match dpad {
                    Some((middle, reach)) => (at.distance(middle) <= reach).then_some(middle),
                    None => Some(at),
                };
                touch.thumb = from.map(|from| Thumb {
                    id: finger.id(),
                    from,
                    at,
                    on_dpad: dpad.is_some(),
                });
            }
            None => {}
        }
    }
    touch.thumb = touch.thumb.take().and_then(|mut thumb| {
        thumb.at = touches.get_pressed(thumb.id)?.position();
        if !thumb.on_dpad {
            // Pulled past its reach the stick comes along, so the way back is never long.
            thumb.from = thumb.at - (thumb.at - thumb.from).clamp_length_max(REACH);
        }
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
        // The d-pad shows itself.
        let Some(thumb) = touch.thumb.as_ref().filter(|thumb| !thumb.on_dpad) else {
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

/// The game's controls: its buttons in the bottom-right corner, the d-pad in the bottom-left.
#[derive(Component)]
struct Pad;

/// The d-pad: a thumb on it pushes the way it is from the middle.
#[derive(Component)]
pub struct Dpad;

/// One of the d-pad's four arms, by RetroPad id, lit while its way is pushed.
#[derive(Component)]
struct DpadArm(u16);

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
    commands
        .spawn((
            Pad,
            Dpad,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                bottom: Val::Px(24.0),
                width: Val::Px(DPAD),
                height: Val::Px(DPAD),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(FILL),
            GlobalZIndex(2),
        ))
        .with_children(|dpad| {
            // The arms of a cross, in thirds of the pad: the corners between them are the
            // diagonals.
            for (id, column, row) in [
                (UP, 1.0, 0.0),
                (LEFT, 0.0, 1.0),
                (RIGHT, 2.0, 1.0),
                (DOWN, 1.0, 2.0),
            ] {
                let upright = matches!(id, UP | DOWN);
                dpad.spawn((
                    DpadArm(id),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(column * ARM),
                        top: Val::Px(row * ARM),
                        width: Val::Px(ARM),
                        height: Val::Px(ARM),
                        flex_direction: if upright {
                            FlexDirection::Column
                        } else {
                            FlexDirection::Row
                        },
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(10.0)),
                        ..default()
                    },
                    BackgroundColor(FILL),
                    BorderColor::all(LINE),
                ))
                .with_children(|arm| {
                    // An arrow in steps, like the bar's pixels: its point first, up and left.
                    let mut steps = [6.0, 14.0, 22.0];
                    if matches!(id, DOWN | RIGHT) {
                        steps.reverse();
                    }
                    for step in steps {
                        let (width, height) = if upright { (step, 5.0) } else { (5.0, step) };
                        arm.spawn((
                            Node {
                                width: Val::Px(width),
                                height: Val::Px(height),
                                ..default()
                            },
                            BackgroundColor(Color::WHITE),
                        ));
                    }
                });
            }
        });
}

fn light_dpad(touch: Res<Touch>, mut arms: Query<(&DpadArm, &mut BackgroundColor)>) {
    let pushed = touch.pad();
    for (arm, mut color) in &mut arms {
        let fill = if pushed & 1 << arm.0 != 0 {
            PRESSED
        } else {
            FILL
        };
        if color.0 != fill {
            color.0 = fill;
        }
    }
}

fn remove_pad(mut commands: Commands, mut touch: ResMut<Touch>, pads: Query<Entity, With<Pad>>) {
    for pad in &pads {
        commands.entity(pad).despawn();
    }
    // A thumb still on the d-pad doesn't walk the bar from there.
    touch.thumb = None;
}
