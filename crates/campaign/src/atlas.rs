//! The tileset sidecar as `bevy_sprite_editor` writes it, and every reason this build
//! will not draw with one.
//!
//! The art is drawn by hand in that tool and this repository only ever reads it. The
//! tool grows an atlas rightwards and nowhere else, so what it writes is a single row
//! of square tiles: a PNG that is `width_in_tiles` tiles across and one tile down,
//! row-major over the whole image, beside a JSON sidecar naming the tile size. That
//! shape is the whole contract, and it is what lets a tile's column be its index.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::tiles::TILE_COUNT;

/// The suffix `bevy_sprite_editor` gives a sidecar, appended to the whole file name
/// rather than replacing an extension.
pub const SIDECAR_SUFFIX: &str = "atlas.json";

/// The sidecar format this build was written against.
///
/// A sidecar declaring anything else is read anyway, with a warning: the version is a
/// string the art tool controls, and the only part of the file this build does not
/// read is palette data. Refusing to draw the map over it would be a worse trade than
/// drawing it.
pub const SIDECAR_VERSION: &str = "1.0";

/// Why a tileset could not be drawn with.
///
/// One variant per distinguishable failure, so a message can say which. The last two
/// are not decided here — the image is decoded by whoever loads it — but they are the
/// same kind of answer and share the type so there is one thing to report.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AtlasError {
    /// The sidecar is not there, or could not be read off the disk.
    #[error("`{}` could not be read: {source}", .path.display())]
    SidecarUnreadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The sidecar was read, but is not JSON this build understands.
    #[error("`{}` is not a tileset sidecar: {message}", .path.display())]
    SidecarMalformed { path: PathBuf, message: String },

    /// The sidecar describes an atlas with no tiles in it.
    #[error("`{}` describes no tiles", .0.display())]
    NoTiles(PathBuf),

    /// The sidecar describes tiles with no size.
    #[error("`{}` describes a tile size of zero", .0.display())]
    ZeroTileSize(PathBuf),

    /// The sidecar describes more than one row. A grid would break the one thing the
    /// format is relied on for — that a tile's column is its layer.
    #[error(
        "`{}` is {rows} tiles tall; the tileset must be a single row, as bevy_sprite_editor writes it",
        .path.display()
    )]
    NotAStrip { path: PathBuf, rows: u32 },

    /// The strip is shorter than the map has tiles to draw.
    #[error("`{}` holds {found} tiles; the map draws {TILE_COUNT}", .path.display())]
    TooFewTiles { path: PathBuf, found: u32 },

    /// The decoded image does not have the shape the sidecar promised.
    #[error(
        "`{}` is {width}x{height} pixels, not the {expected_width}x{expected_height} its sidecar describes",
        .path.display()
    )]
    SizeMismatch {
        path: PathBuf,
        width: u32,
        height: u32,
        expected_width: u32,
        expected_height: u32,
    },

    /// The image could not be decoded at all.
    #[error("`{}` could not be read as an image", .0.display())]
    ImageUnreadable(PathBuf),
}

/// What the sidecar says about the strip beside it.
///
/// Fields this build does not read — the palette, the generated ramps — are ignored,
/// so a sidecar saved by a newer `bevy_sprite_editor` still loads.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AtlasMeta {
    #[serde(default)]
    pub version: String,
    pub tile_size: u32,
    pub width_in_tiles: u32,
    #[serde(default = "one")]
    pub height_in_tiles: u32,
}

fn one() -> u32 {
    1
}

/// Where the strip's PNG sits, given the base name both files share.
pub fn image_path(base: &Path) -> PathBuf {
    let mut path = base.to_path_buf();
    path.set_extension("png");
    path
}

/// Where the sidecar sits, given the base name both files share.
pub fn sidecar_path(base: &Path) -> PathBuf {
    let name = base
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "tileset".to_owned());
    let mut path = base.to_path_buf();
    path.set_file_name(format!("{name}.{SIDECAR_SUFFIX}"));
    path
}

impl AtlasMeta {
    /// The sidecar beside `base`, checked far enough that a caller may index tiles by
    /// column.
    ///
    /// Fails [`AtlasError::SidecarUnreadable`] when the file is not there,
    /// [`AtlasError::SidecarMalformed`] when it is not JSON for this type,
    /// [`AtlasError::NoTiles`] / [`AtlasError::ZeroTileSize`] on an atlas with no
    /// tiles or no tile size, [`AtlasError::NotAStrip`] on more than one row, and
    /// [`AtlasError::TooFewTiles`] when the strip is shorter than [`TILE_COUNT`].
    /// Reads nothing but the sidecar; the image is somebody else's to load.
    pub fn read(base: &Path) -> Result<Self, AtlasError> {
        let path = sidecar_path(base);
        let text = std::fs::read_to_string(&path).map_err(|source| {
            AtlasError::SidecarUnreadable {
                path: path.clone(),
                source,
            }
        })?;
        let meta: Self = serde_json::from_str(&text).map_err(|error| {
            AtlasError::SidecarMalformed {
                path: path.clone(),
                message: error.to_string(),
            }
        })?;
        meta.check(&path)?;
        Ok(meta)
    }

    /// How wide the strip must be, in pixels.
    pub fn width_in_pixels(&self) -> u32 {
        self.width_in_tiles * self.tile_size
    }

    /// Whether the sidecar names a version this build was written against.
    pub fn is_known_version(&self) -> bool {
        self.version.is_empty() || self.version == SIDECAR_VERSION
    }

    /// Check a decoded image against what the sidecar promised.
    ///
    /// Fails [`AtlasError::SizeMismatch`], which is the one thing the sidecar can be
    /// wrong about that the sidecar alone cannot catch.
    pub fn check_image(&self, path: &Path, width: u32, height: u32) -> Result<(), AtlasError> {
        if width == self.width_in_pixels() && height == self.tile_size {
            return Ok(());
        }
        Err(AtlasError::SizeMismatch {
            path: path.to_path_buf(),
            width,
            height,
            expected_width: self.width_in_pixels(),
            expected_height: self.tile_size,
        })
    }

    fn check(&self, path: &Path) -> Result<(), AtlasError> {
        if self.width_in_tiles == 0 || self.height_in_tiles == 0 {
            return Err(AtlasError::NoTiles(path.to_path_buf()));
        }
        if self.tile_size == 0 {
            return Err(AtlasError::ZeroTileSize(path.to_path_buf()));
        }
        if self.height_in_tiles != 1 {
            return Err(AtlasError::NotAStrip {
                path: path.to_path_buf(),
                rows: self.height_in_tiles,
            });
        }
        if self.width_in_tiles < u32::from(TILE_COUNT) {
            return Err(AtlasError::TooFewTiles {
                path: path.to_path_buf(),
                found: self.width_in_tiles,
            });
        }
        Ok(())
    }
}
