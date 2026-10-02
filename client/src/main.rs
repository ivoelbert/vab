mod cabinets;
mod chat;
mod emulator;
mod help;
mod player;
mod room;
mod voice;

use bevy::asset::AssetId;
use bevy::asset::AssetMetaCheck;
use bevy::prelude::*;
use cabinets::{Cabinets, CabinetsPlugin};
use chat::ChatPlugin;
use emulator::EmulatorPlugin;
use help::HelpPlugin;
use player::{PlayerPlugin, Walkable, spawn_player};
use room::RoomPlugin;
use voice::VoicePlugin;
use world::{Map, map_sprite};

/// The bar, made with the editor (`make editor`) and built into the client.
const BAR_MAP: &str = include_str!("../../assets/maps/bar.ron");
/// All text is in Fira Mono cut down to Latin-1, so names and chat can have accents and ñ
/// (Bevy's own font is ASCII only). SIL Open Font License: fonts/OFL.txt.
const FONT: &[u8] = include_bytes!("../fonts/FiraMono-Latin1.ttf");

fn main() {
    let mut app = App::new();
    app.add_plugins((
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
            .set(ImagePlugin::default_nearest())
            // The server only has the PNGs; don't request a .meta file for each.
            .set(AssetPlugin {
                meta_check: AssetMetaCheck::Never,
                ..default()
            }),
        PlayerPlugin,
        CabinetsPlugin,
        ChatPlugin,
        EmulatorPlugin,
        HelpPlugin,
        RoomPlugin,
        VoicePlugin,
    ))
    .init_state::<Mode>()
    .insert_resource(ClearColor(Color::srgb(0.05, 0.05, 0.08)))
    .add_systems(Startup, setup);
    // Text without a font of its own uses the default handle's.
    app.world_mut()
        .resource_mut::<Assets<Font>>()
        .insert(AssetId::default(), Font::from_bytes(FONT.to_vec()))
        .expect("the default font handle takes a font");
    app.run();
}

/// Walking around the bar, or playing a cabinet's game.
#[derive(States, Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Mode {
    #[default]
    Walking,
    Playing,
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    let map = Map::from_ron(BAR_MAP).expect("assets/maps/bar.ron is a valid map");
    for placed in &map.floor {
        commands.spawn(map_sprite(&asset_server, placed, false));
    }
    for placed in &map.objects {
        commands.spawn(map_sprite(&asset_server, placed, true));
    }
    commands.spawn(Camera2d);

    let walkable = Walkable::from_map(&map);
    spawn_player(&mut commands, &asset_server, &walkable);
    commands.insert_resource(walkable);
    commands.insert_resource(Cabinets::from_map(&map));
}
