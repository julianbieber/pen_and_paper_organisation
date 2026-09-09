//! The image a document may be drawn over, every rule one must satisfy, and getting the
//! file into the campaign.
//!
//! An image backdrop is a picture the GM made elsewhere — a scanned dungeon, a city plan
//! drawn by hand — sitting under the feature layer so it can be annotated. It is part of
//! the document that declares it and travels with it, exactly as a grid does. Changing one
//! is not this module's business: [`crate::edit`] owns that, through the single
//! crate-visible accessor that exists for it and for nothing else.
//!
//! The file is **named, not pathed**. A document holds one file name, joined onto the
//! campaign's images directory by [`crate::layout`] and nowhere else, which is what lets
//! the whole campaign directory move without breaking. That makes the name a trust
//! boundary, so importing derives it through [`crate::slug`] and stores it only after
//! [`name_refusal`] has passed it.
//!
//! Every sum here is a pure function over the declaration: where the picture lands in
//! cells, which pixel a cell falls on, and what scale a measured distance implies. They
//! are the whole of the placement arithmetic, they are written once, and they are tested
//! without a window.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::feature::{CellPoint, file_name_refusal};
use crate::layout;

/// The most bytes an image this build imports may run to.
///
/// A scanned battle map is megabytes; anything past this is a mistake or an attack, and
/// refusing it before the file is read is what keeps a decode from being handed a size
/// nothing on the machine can hold.
pub const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

/// The most pixels an image may span on a side.
///
/// Checked against the file's header before anything is copied. Past this a texture is
/// refused by the graphics device rather than by the asset system, which surfaces as a
/// blank backdrop and a log line instead of a sentence the GM can act on.
pub const MAX_IMAGE_SIDE: u32 = 16_384;

/// The most bytes an imported file's name may run to.
///
/// Sized as [`MAX_DUNGEON_NAME_BYTES`](crate::feature::MAX_DUNGEON_NAME_BYTES) is, leaving
/// room for the uniquing suffix an import may add.
pub const MAX_IMAGE_NAME_BYTES: usize = 200;

/// The largest scale a declaration may carry, in cells per pixel.
///
/// An upper bound as well as a lower one, because this number is a *multiplier*: a finite
/// but enormous scale times an image's pixel width overflows to infinity in the transform,
/// and a non-finite transform fails inside the renderer rather than here.
pub const MAX_CELLS_PER_PIXEL: f32 = 1.0e6;

/// An image backdrop a document may not hold.
///
/// Its own type for the reason [`GridProblem`](crate::grid::GridProblem) is: the same
/// mistakes are reachable both by reading a document off disk and by applying an edit to
/// one already open, and a reader should get the same sentence either way.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ImageProblem {
    /// The file name is not one this build would itself have written.
    #[error("`{name}` {reason}")]
    BadName { name: String, reason: &'static str },

    /// A scale that is not a finite positive number, or one large enough to overflow the
    /// placement.
    #[error("a scale of {cells_per_pixel} cells per pixel {reason}")]
    BadScale {
        cells_per_pixel: f32,
        reason: &'static str,
    },

    /// An origin that is not a finite position.
    #[error("an origin of {origin} is not a finite position")]
    NonFiniteOrigin { origin: CellPoint },

    /// An opacity outside fully transparent to fully opaque.
    #[error("an opacity of {opacity} is not between 0 and 1")]
    BadOpacity { opacity: f32 },

    /// A calibration whose two marks fall on the same pixel, which fixes no scale.
    #[error("both marks fall on the same point of the image, which sets no scale")]
    MarksCoincide,

    /// A calibration distance that is not a finite positive number.
    #[error("a distance of {distance} {reason}")]
    BadDistance { distance: f32, reason: &'static str },
}

/// Why an image could not be imported into the campaign.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ImportError {
    /// The path holds nothing, or something that is not a regular file.
    ///
    /// Distinct from unreadable: a directory and a FIFO are both "there", and reading the
    /// second would hang for the life of the process.
    #[error("`{}` is not a file that can be imported: {source}", .path.display())]
    NotAFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// The file is larger than this build imports, checked before it is read.
    #[error("`{}` is {bytes} bytes, over the {limit} byte limit", .path.display())]
    TooLarge {
        path: PathBuf,
        bytes: u64,
        limit: u64,
    },

    /// The bytes are not an image this build decodes, whatever the name says.
    #[error("`{}` is not an image this build draws: {reason}", .path.display())]
    NotAnImage { path: PathBuf, reason: String },

    /// The image decodes but is larger on a side than a texture may be.
    #[error("`{}` is {width}x{height} pixels, over the {limit} pixel limit on a side", .path.display())]
    TooManyPixels {
        path: PathBuf,
        width: u32,
        height: u32,
        limit: u32,
    },

    /// The campaign's images directory could not be made, or the copy could not be
    /// written into it.
    #[error("`{}` could not be written: {source}", .path.display())]
    Unwritable {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// The picture a document is drawn over: which file, where it sits, how big it is, and how
/// far it is faded.
///
/// Every field is private and there is no way to change one but [`Edit`](crate::edit::Edit),
/// the same rule [`World`](crate::world::World) holds its features under and
/// [`TileGrid`](crate::grid::TileGrid) its tiles.
///
/// A declaration that exists is always drawable: its name passes [`name_refusal`], its
/// scale is finite, positive and below [`MAX_CELLS_PER_PIXEL`], its origin is finite, and
/// its opacity is between zero and one. [`ImageBackdrop::new`] refuses anything else and
/// [`World::load`](crate::world::World::load) refuses a document carrying one that is not,
/// so nothing downstream has to check.
///
/// Whether the *file* can be read is deliberately not part of this. A document that names a
/// picture nobody can find still opens and still draws its features; only the editor judges
/// the bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageBackdrop {
    file: String,
    origin: CellPoint,
    cells_per_pixel: f32,
    opacity: f32,
}

impl ImageBackdrop {
    /// The declaration naming `file`, with its top-left corner at `origin`.
    ///
    /// Fails [`ImageProblem`] for a name, scale, origin or opacity a document may not hold.
    pub fn new(
        file: impl Into<String>,
        origin: CellPoint,
        cells_per_pixel: f32,
        opacity: f32,
    ) -> Result<Self, ImageProblem> {
        let backdrop = Self {
            file: file.into(),
            origin,
            cells_per_pixel,
            opacity,
        };
        match backdrop.refusal() {
            Some(problem) => Err(problem),
            None => Ok(backdrop),
        }
    }

    /// The file this document is drawn over, a name inside the campaign's images
    /// directory and never a path.
    pub fn file(&self) -> &str {
        &self.file
    }

    /// Where the picture's top-left corner sits, in the document's own cells.
    pub fn origin(&self) -> CellPoint {
        self.origin
    }

    /// How many cells one pixel of the picture spans.
    pub fn cells_per_pixel(&self) -> f32 {
        self.cells_per_pixel
    }

    /// How opaque the picture is drawn, from zero for invisible to one for solid.
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Why this declaration is not one a document may hold, or `None` if it is fine.
    ///
    /// Exists for a declaration that arrived by another route than
    /// [`ImageBackdrop::new`] — deserialization — so that a hand-edited document is
    /// refused with the same sentence an edit would be.
    pub fn refusal(&self) -> Option<ImageProblem> {
        if let Some(reason) = name_refusal(&self.file) {
            return Some(ImageProblem::BadName {
                name: self.file.clone(),
                reason,
            });
        }
        if let Some(reason) = scale_refusal(self.cells_per_pixel) {
            return Some(ImageProblem::BadScale {
                cells_per_pixel: self.cells_per_pixel,
                reason,
            });
        }
        if !self.origin.is_finite() {
            return Some(ImageProblem::NonFiniteOrigin {
                origin: self.origin,
            });
        }
        if !(0.0..=1.0).contains(&self.opacity) {
            return Some(ImageProblem::BadOpacity {
                opacity: self.opacity,
            });
        }
        None
    }

    /// The same declaration moved to `origin` and scaled to `cells_per_pixel`.
    ///
    /// Fails as [`ImageBackdrop::new`] does. The file and the opacity are carried over.
    pub fn placed(&self, origin: CellPoint, cells_per_pixel: f32) -> Result<Self, ImageProblem> {
        Self::new(self.file.clone(), origin, cells_per_pixel, self.opacity)
    }

    /// The same declaration faded to `opacity`.
    ///
    /// Fails [`ImageProblem::BadOpacity`] outside zero to one.
    pub fn faded(&self, opacity: f32) -> Result<Self, ImageProblem> {
        Self::new(self.file.clone(), self.origin, self.cells_per_pixel, opacity)
    }

    /// How far the picture reaches, in cells, given how many pixels it turned out to be.
    ///
    /// The far corner, the near one being [`ImageBackdrop::origin`]. Cell rows run
    /// downwards, so both components grow.
    pub fn far_corner(&self, size_in_pixels: (u32, u32)) -> CellPoint {
        CellPoint::new(
            self.origin.x + size_in_pixels.0 as f32 * self.cells_per_pixel,
            self.origin.y + size_in_pixels.1 as f32 * self.cells_per_pixel,
        )
    }

    /// Which pixel of the picture the cell position `at` falls on.
    ///
    /// Fractional, and may be outside the picture — the caller decides what that means.
    /// This is the inverse of [`ImageBackdrop::cell_of_pixel`], and the two are written
    /// here together because a placement and its inverse computed in two places are two
    /// answers to one question.
    pub fn pixel_of_cell(&self, at: CellPoint) -> (f32, f32) {
        (
            (at.x - self.origin.x) / self.cells_per_pixel,
            (at.y - self.origin.y) / self.cells_per_pixel,
        )
    }

    /// Which cell position the pixel `(x, y)` of the picture falls on.
    pub fn cell_of_pixel(&self, x: f32, y: f32) -> CellPoint {
        CellPoint::new(
            self.origin.x + x * self.cells_per_pixel,
            self.origin.y + y * self.cells_per_pixel,
        )
    }

    /// The origin that would leave the picture's pixel `(x, y)` sitting on `at`, were the
    /// scale `cells_per_pixel`.
    ///
    /// What keeps a rescale anchored: a calibration fixes one of the two marks the GM
    /// placed, so the landmark they aligned does not slide out from under the feature on
    /// top of it.
    pub fn anchored(&self, x: f32, y: f32, at: CellPoint, cells_per_pixel: f32) -> CellPoint {
        CellPoint::new(at.x - x * cells_per_pixel, at.y - y * cells_per_pixel)
    }

    /// The origin and scale that centre a picture of `size_in_pixels` inside a rectangle
    /// `width` by `height` cells, as large as fits.
    ///
    /// What an import declares, so a picture arrives visible over whatever the document is
    /// drawn on rather than at some scale the GM has to hunt for.
    pub fn fit_to(width: u32, height: u32, size_in_pixels: (u32, u32)) -> (CellPoint, f32) {
        let (pixels_across, pixels_down) = size_in_pixels;
        if pixels_across == 0 || pixels_down == 0 {
            return (CellPoint::new(0.0, 0.0), 1.0);
        }
        let across = width as f32 / pixels_across as f32;
        let down = height as f32 / pixels_down as f32;
        let scale = across.min(down).clamp(f32::MIN_POSITIVE, MAX_CELLS_PER_PIXEL);
        let origin = CellPoint::new(
            (width as f32 - pixels_across as f32 * scale) / 2.0,
            (height as f32 - pixels_down as f32 * scale) / 2.0,
        );
        (origin, scale)
    }
}

/// The scale that puts the picture's pixels `first` and `second` exactly `distance` apart,
/// where one cell is worth `units_per_cell` of whatever `distance` is measured in.
///
/// This is the whole of two-point calibration: the GM marks two places they know the real
/// distance between, and the picture is scaled so the map agrees. It takes
/// `units_per_cell` rather than reading it, because a cell is worth the grid's metres in a
/// dungeon and the manifest's units on the world map, and only the caller knows which
/// document this is.
///
/// Fails [`ImageProblem::MarksCoincide`] when the two marks are the same pixel,
/// [`ImageProblem::BadDistance`] for a distance that is not a finite positive number, and
/// [`ImageProblem::BadScale`] when the answer is not a scale a document may hold — which a
/// legal distance over a tiny separation reaches.
pub fn calibrate(
    first: (f32, f32),
    second: (f32, f32),
    distance: f32,
    units_per_cell: f32,
) -> Result<f32, ImageProblem> {
    if let Some(reason) = distance_refusal(distance) {
        return Err(ImageProblem::BadDistance { distance, reason });
    }
    if let Some(reason) = scale_refusal(units_per_cell) {
        return Err(ImageProblem::BadScale {
            cells_per_pixel: units_per_cell,
            reason,
        });
    }

    let across = second.0 - first.0;
    let down = second.1 - first.1;
    let apart = (across * across + down * down).sqrt();
    if !apart.is_finite() || apart <= 0.0 {
        return Err(ImageProblem::MarksCoincide);
    }

    let cells_per_pixel = (distance / units_per_cell) / apart;
    match scale_refusal(cells_per_pixel) {
        Some(reason) => Err(ImageProblem::BadScale {
            cells_per_pixel,
            reason,
        }),
        None => Ok(cells_per_pixel),
    }
}

/// Why `cells_per_pixel` is not a scale a declaration may carry, or `None` if it is fine.
///
/// Bounded above as well as below, unlike
/// [`reveal_refusal`](crate::feature::reveal_refusal), because this number multiplies an
/// image's pixel extent rather than being compared against one.
pub fn scale_refusal(cells_per_pixel: f32) -> Option<&'static str> {
    if !cells_per_pixel.is_finite() {
        return Some("is not a finite number");
    }
    if cells_per_pixel <= 0.0 {
        return Some("is not greater than zero, and names a picture with no extent");
    }
    if cells_per_pixel > MAX_CELLS_PER_PIXEL {
        return Some("is large enough to place the picture beyond any coordinate");
    }
    None
}

/// Why `name` is not one a document may carry as its image, or `None` if it is fine.
///
/// As strict as [`dungeon_name_refusal`](crate::feature::dungeon_name_refusal) and for the
/// same reason — it names a file this tool **creates and writes**, so it is one path
/// component and nothing else — with the extension drawn from the formats this build
/// actually decodes rather than a single fixed one.
///
/// The containment this gives is **lexical**, exactly as it is for a dungeon: a single
/// component cannot climb out of the images directory by name, but the directory or the
/// file may still be a symlink. That is why the import creates its destination with
/// [`std::fs::File::create_new`] rather than trusting the name alone.
pub fn name_refusal(name: &str) -> Option<&'static str> {
    if let Some(reason) = file_name_refusal(name, MAX_IMAGE_NAME_BYTES) {
        return Some(reason);
    }
    if !IMAGE_EXTENSIONS
        .iter()
        .any(|extension| has_extension(name, extension))
    {
        return Some("does not end in an extension this build decodes");
    }
    None
}

/// The extensions an imported image may carry, which are the formats this build decodes.
///
/// The extension is written from the format the bytes turned out to be, never from what
/// the GM's file happened to be called, so this list and the decoder cannot disagree.
pub const IMAGE_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

fn distance_refusal(distance: f32) -> Option<&'static str> {
    if !distance.is_finite() {
        return Some("is not a finite number");
    }
    if distance <= 0.0 {
        return Some("is not greater than zero, and two places are never no distance apart");
    }
    None
}

fn has_extension(name: &str, extension: &str) -> bool {
    name.len() > extension.len() + 1
        && name.as_bytes()[name.len() - extension.len() - 1] == b'.'
        && name[name.len() - extension.len()..].eq_ignore_ascii_case(extension)
}

/// Copy `source` into the campaign at `root` as a backdrop, and hand back the name the
/// document should carry.
///
/// The file lands in the campaign's images directory so the whole directory stays
/// portable — a campaign referring to a picture elsewhere on the disk is broken the moment
/// it is moved or synced.
///
/// The source is judged before anything is written: refused unless it is a regular file
/// ([`ImportError::NotAFile`] — reading a FIFO would hang), unless it is within
/// [`MAX_IMAGE_BYTES`], unless its bytes are a format this build decodes
/// ([`ImportError::NotAnImage`], decided from the header rather than from the name), and
/// unless it is within [`MAX_IMAGE_SIDE`] on each side.
///
/// The destination name is derived from the source's own stem through [`crate::slug`] and
/// carries the extension of the format the bytes actually are. It is made unique by
/// **creating** it: each candidate is opened with [`std::fs::File::create_new`] and the
/// first that succeeds wins, so there is no gap between finding a free name and taking it,
/// and a symlink sitting where the copy would go is refused rather than followed and
/// written through.
///
/// A copy that fails part-way takes its half-written destination with it, so this never
/// leaves a file a document could later name.
///
/// The result always passes [`name_refusal`].
pub fn import(root: &Path, source: &Path) -> Result<Imported, ImportError> {
    let meta = std::fs::metadata(source).map_err(|error| ImportError::NotAFile {
        path: source.to_owned(),
        source: error,
    })?;
    if !meta.is_file() {
        return Err(ImportError::NotAFile {
            path: source.to_owned(),
            source: io::Error::other("not a regular file, so it is refused rather than read"),
        });
    }
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(ImportError::TooLarge {
            path: source.to_owned(),
            bytes: meta.len(),
            limit: MAX_IMAGE_BYTES,
        });
    }

    let (extension, width, height) = probe(source)?;
    if width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
        return Err(ImportError::TooManyPixels {
            path: source.to_owned(),
            width,
            height,
            limit: MAX_IMAGE_SIDE,
        });
    }

    let directory = layout::images(root);
    std::fs::create_dir_all(&directory).map_err(|error| ImportError::Unwritable {
        path: directory.clone(),
        source: error,
    })?;

    let stem = crate::slug::slug_of(&source_stem(source)).unwrap_or_else(|| "backdrop".to_owned());
    let (name, destination, file) = claim(&directory, &stem, extension)?;

    copy_into(source, file, &destination).inspect_err(|_| {
        let _ = std::fs::remove_file(&destination);
    })?;
    Ok(Imported {
        name,
        size_in_pixels: (width, height),
    })
}

/// A picture that has landed in the campaign: the name a document should carry, and how
/// big it turned out to be.
///
/// The extent comes back with the name because the caller needs it immediately, to work
/// out a placement that fits, and the header has just been read to find it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    pub name: String,
    pub size_in_pixels: (u32, u32),
}

fn probe(path: &Path) -> Result<(&'static str, u32, u32), ImportError> {
    let not_an_image = |reason: String| ImportError::NotAnImage {
        path: path.to_owned(),
        reason,
    };

    let file = std::fs::File::open(path).map_err(|error| ImportError::NotAFile {
        path: path.to_owned(),
        source: error,
    })?;
    let reader = image::ImageReader::new(io::BufReader::new(file))
        .with_guessed_format()
        .map_err(|error| not_an_image(error.to_string()))?;

    let extension = match reader.format() {
        Some(image::ImageFormat::Png) => "png",
        Some(image::ImageFormat::Jpeg) => "jpg",
        Some(format) => {
            return Err(not_an_image(format!(
                "it is {format:?}, and this build decodes only PNG and JPEG"
            )));
        }
        None => return Err(not_an_image("its bytes name no format at all".to_owned())),
    };

    let (width, height) = reader
        .into_dimensions()
        .map_err(|error| not_an_image(error.to_string()))?;
    Ok((extension, width, height))
}

fn claim(
    directory: &Path,
    stem: &str,
    extension: &'static str,
) -> Result<(String, PathBuf, std::fs::File), ImportError> {
    for suffix in 1u32..=MAX_IMPORT_ATTEMPTS {
        let name = match suffix {
            1 => format!("{stem}.{extension}"),
            _ => format!("{stem}-{suffix}.{extension}"),
        };
        let destination = directory.join(&name);
        match std::fs::File::create_new(&destination) {
            Ok(file) => return Ok((name, destination, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(ImportError::Unwritable {
                    path: destination,
                    source: error,
                });
            }
        }
    }
    Err(ImportError::Unwritable {
        path: directory.join(format!("{stem}.{extension}")),
        source: io::Error::other("every name this import would take is already spoken for"),
    })
}

fn copy_into(
    source: &Path,
    mut file: std::fs::File,
    destination: &Path,
) -> Result<(), ImportError> {
    use std::io::Write as _;

    let unwritable = |error: io::Error| ImportError::Unwritable {
        path: destination.to_owned(),
        source: error,
    };

    let mut reading = std::fs::File::open(source).map_err(|error| ImportError::NotAFile {
        path: source.to_owned(),
        source: error,
    })?;
    let copied = io::copy(&mut io::Read::take(&mut reading, MAX_IMAGE_BYTES), &mut file)
        .map_err(unwritable)?;
    if copied >= MAX_IMAGE_BYTES {
        return Err(ImportError::TooLarge {
            path: source.to_owned(),
            bytes: copied,
            limit: MAX_IMAGE_BYTES,
        });
    }
    file.flush().map_err(unwritable)?;
    file.sync_all().map_err(unwritable)
}

fn source_stem(source: &Path) -> String {
    source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

const MAX_IMPORT_ATTEMPTS: u32 = 4_096;
