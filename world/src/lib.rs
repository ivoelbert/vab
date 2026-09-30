//! The bar's map: what is placed on the isometric grid and how it is drawn. Shared by the
//! editor (tools/editor) and, later, the game client.

use bevy::prelude::*;
use bevy::sprite::Anchor;
use serde::{Deserialize, Serialize};

/// Cells are 2:1 isometric diamonds.
pub const TILE_WIDTH: f32 = 32.0;
pub const TILE_HEIGHT: f32 = 16.0;

/// Everything placed on the grid. Tile names are PNG paths under assets/tiles/ without the
/// extension, e.g. "floor/wood" or "objects/cabinet".
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Map {
    pub floor: Vec<Placed>,
    pub objects: Vec<Placed>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Placed {
    pub x: i32,
    pub y: i32,
    pub tile: String,
    /// The ROM set a cabinet runs, e.g. "mk2".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
}

/// A game a cabinet can run, from assets/games.ron.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Game {
    /// The ROM set, served at /roms/<rom>.zip.
    pub rom: String,
    /// The FBNeo core that runs it, served at /fbneo/<core>/fbneo.mjs.
    pub core: String,
    pub title: String,
    /// A BIOS set loaded next to the ROM, e.g. "neogeo".
    #[serde(default)]
    pub bios: Option<String>,
}

pub fn games_from_ron(text: &str) -> Result<Vec<Game>, ron::error::SpannedError> {
    ron::from_str(text)
}

impl Map {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("a map always serializes")
    }
}

/// Center of a cell's diamond. +x runs down-right on screen, +y down-left.
pub fn cell_to_world(x: i32, y: i32) -> Vec2 {
    Vec2::new(
        (x - y) as f32 * TILE_WIDTH / 2.0,
        -(x + y) as f32 * TILE_HEIGHT / 2.0,
    )
}

/// The cell whose diamond contains a world position.
pub fn world_to_cell(position: Vec2) -> IVec2 {
    let across = position.x / (TILE_WIDTH / 2.0);
    let down = -position.y / (TILE_HEIGHT / 2.0);
    IVec2::new(
        ((across + down) / 2.0).round() as i32,
        ((down - across) / 2.0).round() as i32,
    )
}

/// Marks entities drawn from a map.
#[derive(Component)]
pub struct MapSprite;

/// The sprite for a placed floor tile or object.
///
/// Floor tiles are centered on their cell. Objects stand on it: the bottom point of the image
/// is the bottom point of the cell's diamond, and cells nearer the viewer draw on top.
pub fn map_sprite(
    asset_server: &AssetServer,
    placed: &Placed,
    object: bool,
) -> (MapSprite, Sprite, Anchor, Transform) {
    let center = cell_to_world(placed.x, placed.y);
    let depth = (placed.x + placed.y) as f32 * 0.001;
    let image = asset_server.load(format!("tiles/{}.png", placed.tile));
    let (anchor, position) = if object {
        (
            Anchor::BOTTOM_CENTER,
            center.extend(1.0 + depth) - Vec3::Y * TILE_HEIGHT / 2.0,
        )
    } else {
        (Anchor::CENTER, center.extend(depth))
    };
    (
        MapSprite,
        Sprite::from_image(image),
        anchor,
        Transform::from_translation(position),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_positions_map_back_to_their_cell() {
        for (x, y) in [(0, 0), (3, -2), (-5, 7)] {
            let center = cell_to_world(x, y);
            // Anywhere inside the diamond, a little short of its corners.
            for offset in [
                Vec2::ZERO,
                Vec2::new(14.0, 0.0),
                Vec2::new(-14.0, 0.0),
                Vec2::new(0.0, 7.0),
                Vec2::new(0.0, -7.0),
            ] {
                assert_eq!(world_to_cell(center + offset), IVec2::new(x, y));
            }
        }
    }

    #[test]
    fn the_game_catalog_parses() {
        let games = games_from_ron(include_str!("../../assets/games.ron")).unwrap();
        let mslug = games.iter().find(|g| g.rom == "mslug").unwrap();
        assert_eq!(mslug.bios.as_deref(), Some("neogeo"));
        assert!(games.iter().find(|g| g.rom == "mk2").unwrap().bios.is_none());
    }

    #[test]
    fn maps_round_trip_through_ron() {
        let map = Map {
            floor: vec![Placed {
                x: 1,
                y: 2,
                tile: "floor/wood".into(),
                game: None,
            }],
            objects: vec![Placed {
                x: 0,
                y: 0,
                tile: "objects/cabinet".into(),
                game: Some("mk2".into()),
            }],
        };
        let back = Map::from_ron(&map.to_ron()).unwrap();
        assert_eq!(back.floor, map.floor);
        assert_eq!(back.objects, map.objects);
    }
}
