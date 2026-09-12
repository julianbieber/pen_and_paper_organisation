//! Writes a campaign directory with a small real terrain in it, so the editor has
//! something to open by hand.
//!
//! Nothing in the tool bakes a terrain — `watershed_editor` does that — so without
//! this there is no way to try `pnp` without one already on disk. The terrain carries
//! a water solve as well as a height field, because a dry one cannot show a coastline,
//! a lake or a river, and those are most of what the map draws.
//!
//! ```text
//! cargo run -p campaign --example make_demo -- /tmp/demo-campaign
//! cargo run -p campaign_editor -- /tmp/demo-campaign
//! ```

use glam::UVec2;
use watershed::{
    ChannelMeta, FieldInfo, FieldRole, LayerTexels, Terrain, TerrainLayer, WaterInfo,
};

const SIDE: u32 = 256;
const SEA_LEVEL: f32 = 0.30;

fn main() {
    let root = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let size = UVec2::splat(SIDE);

    let ground: Vec<f32> = (0..SIDE * SIDE)
        .map(|i| elevation(i % SIDE, i / SIDE))
        .collect();

    let heights: Vec<u8> = ground.iter().map(|h| (h * 255.0) as u8).collect();
    let height_layer = TerrainLayer::new(
        Some(0),
        vec![ChannelMeta::linear(0.0, 1000.0)],
        LayerTexels::from_bytes(size, 1, heights).expect("height texels"),
    );

    let mut water = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let height = ground[(y * SIDE + x) as usize];
            let depth = (SEA_LEVEL - height).max(0.0) / SEA_LEVEL;
            water.push((depth * 255.0) as u8);
            water.push(128);
            water.push(128);
            water.push((channel(x, y, height) * 255.0) as u8);
        }
    }
    let water_layer = TerrainLayer::new(
        Some(0),
        vec![
            ChannelMeta::linear(0.0, 60.0),
            ChannelMeta::linear(-1.0, 1.0),
            ChannelMeta::linear(-1.0, 1.0),
            ChannelMeta::linear(0.0, 10_000.0),
        ],
        LayerTexels::from_bytes(size, 4, water).expect("water texels"),
    );

    let field = FieldInfo {
        name: "height".to_owned(),
        role: FieldRole::Height,
        shift: 0,
        categorical: false,
        layer: 0,
        channel: 0,
    };

    Terrain::new(
        size,
        vec![field],
        vec![height_layer, water_layer],
        Some(WaterInfo { lakes: 1, layer: 1 }),
    )
    .save_to_dir(root.join("terrain"))
    .unwrap();

    let created =
        campaign::Campaign::create(&root, root.join("terrain"), &campaign::SystemGit).unwrap();
    let campaign = created.campaign;
    println!("created {}", campaign.root().display());
    println!("  name           {}", campaign.manifest().name);
    println!("  terrain        {}", campaign.terrain_dir().display());
    println!(
        "  terrain extent {}x{} cells",
        campaign.terrain().width(),
        campaign.terrain().height()
    );
}

fn elevation(x: u32, y: u32) -> f32 {
    let half = SIDE as f32 / 2.0;
    let (dx, dy) = ((x as f32 - half) / half, (y as f32 - half) / half);
    let dome = (1.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
    let ridges = ((x as f32 / 9.0).sin() * (y as f32 / 14.0).cos() * 0.5 + 0.5) * 0.35;
    (dome * 0.75 + dome * ridges).clamp(0.0, 1.0)
}

fn channel(x: u32, y: u32, height: f32) -> f32 {
    if height <= SEA_LEVEL {
        return 0.0;
    }
    let half = SIDE as f32 / 2.0;
    let main = ((x as f32 - half - (y as f32 / 12.0).sin() * 10.0).abs() / 2.0).min(1.0);
    let tributary = ((y as f32 - half - (x as f32 / 10.0).cos() * 8.0).abs() / 2.0).min(1.0);
    let trunk = (1.0 - main) * (y as f32 / SIDE as f32);
    let branch = (1.0 - tributary) * 0.35;
    trunk.max(branch).clamp(0.0, 1.0)
}
