mod emulator;

use bevy::camera::ScalingMode;
use bevy::prelude::*;

use emulator::{EmulatorPlugin, Screen};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Arcade Bar".into(),
                        // Render into <canvas id="bevy"> in web/index.html, sized to its parent.
                        canvas: Some("#bevy".into()),
                        fit_canvas_to_parent: true,
                        // Let browser shortcuts through (dev tools, reload).
                        prevent_default_event_handling: false,
                        ..default()
                    }),
                    ..default()
                })
                // Pixel art: sample textures without smoothing.
                .set(ImagePlugin::default_nearest()),
            EmulatorPlugin,
        ))
        .insert_resource(ClearColor(Color::BLACK))
        .add_systems(Startup, setup)
        .run();
}

/// The game, filling the window at the 4:3 of an arcade monitor (letterboxed).
fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn((
        Camera2d,
        // Always fit a 4 x 3 area in the window.
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::AutoMin {
                min_width: 4.0,
                min_height: 3.0,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));

    // MK II renders 400x254, stretched to 4:3 like the original monitor.
    commands.spawn((
        Screen,
        Sprite {
            image: images.add(emulator::screen_image(400, 254)),
            custom_size: Some(Vec2::new(4.0, 3.0)),
            ..default()
        },
    ));
}
