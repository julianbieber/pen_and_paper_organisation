//! Every way a tileset sidecar can be unusable, told apart.
//!
//! The strip is drawn by hand in another tool and this repository only reads it, so
//! the failures here are the ones a person editing art will actually hit.

use std::path::{Path, PathBuf};

use campaign::atlas::{AtlasError, AtlasMeta};
use campaign::tiles::TILE_COUNT;

fn write_sidecar(dir: &Path, json: &str) -> PathBuf {
    let base = dir.join("terrain_tiles");
    std::fs::write(campaign::atlas::sidecar_path(&base), json).expect("write the sidecar");
    base
}

fn good_json(width_in_tiles: u32) -> String {
    format!(
        r#"{{"version":"1.0","tile_size":16,"width_in_tiles":{width_in_tiles},"height_in_tiles":1,
           "palette":{{"colors":[],"selected":0}},"generated":{{"ramps":[]}}}}"#
    )
}

// The paths follow bevy_sprite_editor's own: the sidecar suffix is appended to the
// whole file name rather than replacing the extension.
#[test]
fn both_files_are_found_from_one_base_name() {
    let base = Path::new("/tmp/art/terrain_tiles");
    assert_eq!(
        campaign::atlas::image_path(base),
        Path::new("/tmp/art/terrain_tiles.png")
    );
    assert_eq!(
        campaign::atlas::sidecar_path(base),
        Path::new("/tmp/art/terrain_tiles.atlas.json")
    );
}

// The palette and generated-ramp blocks are the art tool's business, and a sidecar
// carrying them must still load here.
#[test]
fn a_sidecar_from_the_art_tool_loads_with_its_extra_blocks_ignored() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let base = write_sidecar(tmp.path(), &good_json(u32::from(TILE_COUNT)));

    let meta = AtlasMeta::read(&base).expect("a well-formed sidecar loads");
    assert_eq!(meta.tile_size, 16);
    assert_eq!(meta.width_in_tiles, u32::from(TILE_COUNT));
    assert_eq!(meta.height_in_tiles, 1);
    assert!(meta.is_known_version());
    assert_eq!(meta.width_in_pixels(), 16 * u32::from(TILE_COUNT));
}

// A strip longer than the map needs is fine: the artist may park spare tiles at the end.
#[test]
fn a_longer_strip_is_accepted() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let base = write_sidecar(tmp.path(), &good_json(u32::from(TILE_COUNT) + 8));
    assert!(AtlasMeta::read(&base).is_ok());
}

// Two different things to fix — write the file, or fix the file — so they must not
// arrive as one error.
#[test]
fn a_missing_sidecar_is_told_apart_from_a_malformed_one() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let missing = tmp.path().join("terrain_tiles");
    assert!(matches!(
        AtlasMeta::read(&missing),
        Err(AtlasError::SidecarUnreadable { .. })
    ));

    let base = write_sidecar(tmp.path(), "{ this is not json");
    assert!(matches!(
        AtlasMeta::read(&base),
        Err(AtlasError::SidecarMalformed { .. })
    ));
}

// A grid would break the one thing the format is relied on for — that a tile's column
// is its layer — so it is refused rather than half-read.
#[test]
fn a_tileset_that_is_not_a_single_row_is_refused() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let base = write_sidecar(
        tmp.path(),
        r#"{"version":"1.0","tile_size":16,"width_in_tiles":24,"height_in_tiles":2}"#,
    );
    assert!(matches!(
        AtlasMeta::read(&base),
        Err(AtlasError::NotAStrip { rows: 2, .. })
    ));
}

// Either would divide the strip into nothing, so both are refused before an index is
// ever computed against them.
#[test]
fn an_atlas_with_no_tiles_or_no_tile_size_is_refused() {
    let tmp = tempfile::tempdir().expect("temp dir");

    let empty = write_sidecar(
        tmp.path(),
        r#"{"version":"1.0","tile_size":16,"width_in_tiles":0,"height_in_tiles":1}"#,
    );
    assert!(matches!(AtlasMeta::read(&empty), Err(AtlasError::NoTiles(_))));

    let sizeless = write_sidecar(
        tmp.path(),
        r#"{"version":"1.0","tile_size":0,"width_in_tiles":24,"height_in_tiles":1}"#,
    );
    assert!(matches!(
        AtlasMeta::read(&sizeless),
        Err(AtlasError::ZeroTileSize(_))
    ));
}

// The map draws TILE_COUNT tiles; a shorter strip would index past the end of the
// array texture, which is worth refusing before anything is drawn.
#[test]
fn a_strip_shorter_than_the_map_draws_is_refused() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let base = write_sidecar(tmp.path(), &good_json(u32::from(TILE_COUNT) - 1));
    assert!(matches!(
        AtlasMeta::read(&base),
        Err(AtlasError::TooFewTiles { .. })
    ));
}

// The sidecar is the only thing that can disagree with the image, and neither file
// alone can catch it.
#[test]
fn an_image_that_is_not_the_shape_the_sidecar_promised_is_refused() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let base = write_sidecar(tmp.path(), &good_json(u32::from(TILE_COUNT)));
    let meta = AtlasMeta::read(&base).expect("sidecar");
    let png = campaign::atlas::image_path(&base);

    assert!(meta.check_image(&png, 16 * u32::from(TILE_COUNT), 16).is_ok());
    assert!(matches!(
        meta.check_image(&png, 16 * u32::from(TILE_COUNT), 32),
        Err(AtlasError::SizeMismatch { .. })
    ));
}

// The version is a string the art tool controls and its only unread contents are
// palette data, so an unknown one is reported rather than refused.
#[test]
fn an_unknown_version_still_loads_and_says_so() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let base = write_sidecar(
        tmp.path(),
        r#"{"version":"9.9","tile_size":16,"width_in_tiles":24,"height_in_tiles":1}"#,
    );
    let meta = AtlasMeta::read(&base).expect("an unknown version is not a refusal");
    assert!(!meta.is_known_version());
}
