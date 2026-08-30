//! Writes a campaign directory with a small real terrain in it, so the editor has
//! something to open by hand.
//!
//! Nothing in the tool bakes a terrain — `watershed_editor` does that — so without
//! this there is no way to try `pnp` without one already on disk.
//!
//! ```text
//! cargo run -p campaign --example make_demo -- /tmp/demo-campaign
//! cargo run -p campaign_editor -- /tmp/demo-campaign
//! ```

use glam::UVec2;
use watershed::{ChannelMeta, FieldInfo, FieldRole, LayerTexels, Terrain, TerrainLayer};

fn main() {
    let root = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let size = UVec2::new(64, 64);
    let bytes: Vec<u8> = (0..64 * 64u32).map(|i| ((i / 64 + i % 64) * 2) as u8).collect();
    let texels = LayerTexels::from_bytes(size, 1, bytes).unwrap();
    let layer = TerrainLayer::new(Some(0), vec![ChannelMeta::linear(0.0, 500.0)], texels);
    let field = FieldInfo {
        name: "height".to_owned(), role: FieldRole::Height,
        shift: 0, categorical: false, layer: 0, channel: 0,
    };
    Terrain::new(size, vec![field], vec![layer], None)
        .save_to_dir(root.join("terrain")).unwrap();

    let campaign = campaign::Campaign::create(&root, "terrain").unwrap();
    println!("created {}", campaign.root().display());
    println!("  name           {}", campaign.manifest().name);
    println!("  terrain        {}", campaign.terrain_dir().display());
    println!("  terrain extent {}x{} cells",
        campaign.terrain().width(), campaign.terrain().height());
}
