// use bevy::prelude::*;
// use bevy::prelude::Bundle;
// use bevy_ecs_tilemap::prelude::*;

// pub fn setup_world(mut commands: Commands, asset_server: Res<AssetServer>) {
//     let texture_handle: Handle<Image> = asset_server.load("Terrain/Ground/Tilemap_Flat.png");

//     let map_size = TilemapSize { x: 4, y: 2 };

//     // Create a tilemap entity a little early.
//     let tilemap_entity = commands.spawn_empty().id();

//     let mut tile_storage = TileStorage::empty(map_size);

//     // Spawn the elements of the tilemap.

//     let tile_pos = TilePos { x: 0, y: 0 };
//     let tile_entity = commands
//         .spawn(TileBundle {
//             position: tile_pos,
//             texture_index: TileTextureIndex(13),
//             tilemap_id: TilemapId(tilemap_entity),
//             ..Default::default()
//         })
//         .id();
//     tile_storage.set(&tile_pos, tile_entity);

//     let tile_size = TilemapTileSize { x: 48.0, y: 48.0 };
//     let grid_size = tile_size.into();
//     let map_type = TilemapType::default();

//     commands.entity(tilemap_entity).insert(TilemapBundle {
//         grid_size,
//         map_type,
//         size: map_size,
//         storage: tile_storage,
//         texture: TilemapTexture::Single(texture_handle),
//         tile_size,
//         transform: get_tilemap_center_transform(&map_size, &grid_size, &map_type, 0.0),
//         ..Default::default()
//     });
// }
