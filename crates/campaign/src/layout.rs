//! The names this campaign format fixes, and the joins onto a campaign root, so that
//! no consumer of the format ever spells one of them.
//!
//! `TERRAIN_DIR` fixes where a campaign [`Campaign::create`](crate::campaign::Campaign::create)
//! builds puts its terrain; it is not where every campaign's terrain is, because the
//! manifest's own `terrain` field stays authoritative for [`Campaign::open`](crate::campaign::Campaign::open)
//! — a terrain is still allowed to live outside the campaign directory, just not one
//! this build creates.

use std::path::{Path, PathBuf};

/// The manifest every campaign directory holds, and the only name a reader knows
/// without being told it.
pub const MANIFEST_FILE: &str = "campaign.ron";

/// The world map document, read and written by [`World`](crate::world::World) — the one
/// name here whose contents this crate understands. A campaign that has never been
/// authored holds no such file, and that is not a gap: an absent world document is an
/// empty world.
pub const WORLD_FILE: &str = "world.ron";

/// Child map documents, one file each.
pub const DUNGEONS_DIR: &str = "dungeons";

/// Backdrop images imported into the campaign, copied in so the directory stays
/// portable.
pub const IMAGES_DIR: &str = "images";

/// The `zk` notebook. A campaign directory carries the directory, and
/// [`Campaign::create`](crate::campaign::Campaign::create) leaves it empty; what makes it
/// a notebook is [`Notebook::ensure`](crate::notebook::Notebook::ensure), the first time a
/// note is asked for.
pub const NOTES_DIR: &str = "notes";

/// Where a campaign this build creates copies its terrain into.
pub const TERRAIN_DIR: &str = "terrain";

/// Every subdirectory a campaign directory holds, in the order they are created.
///
/// `TERRAIN_DIR` is not among them: it is made by the copy or adopted from the source
/// [`Campaign::create`](crate::campaign::Campaign::create) is given, never created empty.
pub const SUBDIRS: [&str; 3] = [DUNGEONS_DIR, IMAGES_DIR, NOTES_DIR];

/// Where the manifest sits under `root`.
pub fn manifest(root: &Path) -> PathBuf {
    root.join(MANIFEST_FILE)
}

/// Where the world document sits under `root`, whether or not it exists — an absent
/// world document is an empty world, not an error.
pub fn world(root: &Path) -> PathBuf {
    root.join(WORLD_FILE)
}

/// Where child map documents sit under `root`.
pub fn dungeons(root: &Path) -> PathBuf {
    root.join(DUNGEONS_DIR)
}

/// Where the dungeon named `name` sits under `root`.
///
/// `name` is a file name and not a path — it is constrained by
/// [`dungeon_name_refusal`](crate::feature::dungeon_name_refusal) wherever it is stored,
/// so it carries its own `.ron` and no separator. This exists so that no consumer joins
/// one by hand, which is the whole reason this module does.
pub fn dungeon(root: &Path, name: &str) -> PathBuf {
    dungeons(root).join(name)
}

/// Where imported backdrops sit under `root`.
pub fn images(root: &Path) -> PathBuf {
    root.join(IMAGES_DIR)
}

/// Where the imported backdrop named `name` sits under `root`.
///
/// `name` is a file name and not a path — it is constrained by
/// [`image::name_refusal`](crate::image::name_refusal) wherever it is stored, so it
/// carries its own extension and no separator. This exists so that no consumer joins one
/// by hand, which is the whole reason this module does, and it is the only place a
/// document's image name is turned into somewhere on the disk.
pub fn image(root: &Path, name: &str) -> PathBuf {
    images(root).join(name)
}

/// Where the notebook sits under `root`.
pub fn notes(root: &Path) -> PathBuf {
    root.join(NOTES_DIR)
}

/// Where a campaign this build creates copies its terrain into.
pub fn terrain(root: &Path) -> PathBuf {
    root.join(TERRAIN_DIR)
}
