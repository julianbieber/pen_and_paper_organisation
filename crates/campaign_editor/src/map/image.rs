//! Reading the picture a document declares off the disk, and drawing it between the
//! backdrop and the features.
//!
//! The file is **not** asked of the asset server, and that is a decision rather than an
//! oversight. The asset root is fixed when the app is built and a campaign directory is
//! chosen at runtime, so the server cannot address one; the ways around that are worse
//! than the problem, because the path would then come from a `world.ron` that may have
//! been written by somebody else. Here the bytes are read at the one join
//! [`campaign::layout::image`] produces, off the frame, and handed to the decoder directly
//! — which also makes "missing", "unreadable" and "not a picture" three sentences this
//! module can say rather than one opaque load failure.
//!
//! The decode runs on the IO pool for the reason every `zk` call does: a scanned map is
//! tens of megabytes and the map must not stutter while one arrives.
//!
//! What is drawn is always what the document declares, except while a placement gesture is
//! in flight, when it is what the gesture would commit — so the preview and the sprite are
//! never two answers to where the picture is.

use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy::tasks::{IoTaskPool, Task, block_on, futures_lite::future};
use campaign::image::MAX_IMAGE_BYTES;
use campaign::layout;

use crate::document::WorldDoc;
use crate::features::image::Placing;
use crate::map::backdrop::Backdrop;
use crate::map::view::IMAGE_Z;
use crate::{OpenCampaign, StatusMessage};

/// The sprite the open document's picture is drawn as.
#[derive(Component, Default, Clone)]
pub struct BackdropImage;

/// What the editor knows about the picture the open document declares.
///
/// A resource rather than a component on the sprite: the failure has to be readable when
/// there is no sprite, which is exactly the case where the file could not be read.
///
/// It holds the one strong handle. Nothing else does — a picture kept per parked document
/// would leave every backdrop the GM visited this session resident for the rest of it.
#[derive(Resource, Default)]
pub struct ImageAsset {
    /// The file this is about, which is what says whether the declaration has moved on.
    pub file: String,
    pub image: Handle<Image>,
    /// How big the picture turned out to be, once it has been read.
    ///
    /// The sprite does not need this — it is anchored and scaled, so it draws correctly at
    /// whatever size the texture is — and it is kept for the gesture, which cannot hit-test
    /// a corner without knowing where the corners are.
    pub size_in_pixels: Option<(u32, u32)>,
    pub failed: bool,
    reported: bool,
    task: Option<Task<Result<Image, String>>>,
}

impl ImageAsset {
    /// Whether the picture is in hand and can be drawn.
    pub fn is_ready(&self) -> bool {
        self.size_in_pixels.is_some() && !self.failed
    }

    fn forget(&mut self) {
        self.file.clear();
        self.image = Handle::default();
        self.size_in_pixels = None;
        self.failed = false;
        self.reported = false;
        self.task = None;
    }
}

/// Keeps the picture on screen in step with what the open document declares, and says once
/// when its file cannot be read.
///
/// The reporting is here rather than in a system of its own because "the read failed" is
/// known exactly once, at the moment the job lands, and a second system would need a flag
/// to answer "have I said this yet" that this one does not.
///
/// Writes [`ImageAsset`] only when something actually differs. An unconditional write marks
/// the resource changed every frame, which is the failure the river threshold already has a
/// test pinning.
pub fn sync_backdrop_image(
    mut commands: Commands,
    open: Res<OpenCampaign>,
    doc: Res<WorldDoc>,
    backdrop: Res<Backdrop>,
    placing: Res<Placing>,
    mut held: ResMut<ImageAsset>,
    mut images: ResMut<Assets<Image>>,
    mut status: ResMut<StatusMessage>,
    mut sprites: Query<(Entity, &mut Transform, &mut Sprite), With<BackdropImage>>,
) {
    let declared = doc.document.world().image().cloned();

    let Some(declared) = declared else {
        if !held.file.is_empty() {
            held.forget();
        }
        for (entity, ..) in sprites.iter() {
            commands.entity(entity).despawn();
        }
        return;
    };

    if held.file != declared.file() {
        let root = open.0.root().to_owned();
        let file = declared.file().to_owned();
        held.forget();
        held.file = file.clone();
        held.task = Some(
            IoTaskPool::get().spawn(async move { read_picture(layout::image(&root, &file)) }),
        );
    }

    if let Some(task) = held.task.as_mut()
        && let Some(landed) = block_on(future::poll_once(task))
    {
        held.task = None;
        match landed {
            Ok(picture) => {
                let size = picture.size();
                held.size_in_pixels = Some((size.x, size.y));
                held.image = images.add(picture);
            }
            Err(reason) => {
                warn!("`{}` could not be drawn: {reason}", held.file);
                held.failed = true;
            }
        }
    }

    if held.failed && !held.reported {
        held.reported = true;
        status.say(format!(
            "`{}` could not be drawn; the document and its features are unaffected",
            declared.file()
        ));
    }

    if !held.is_ready() {
        for (entity, ..) in sprites.iter() {
            commands.entity(entity).despawn();
        }
        return;
    }

    let (origin, cells_per_pixel) = placing
        .preview()
        .unwrap_or((declared.origin(), declared.cells_per_pixel()));
    let (across, down) = held.size_in_pixels.expect("the picture is ready");
    let view = backdrop.view;

    let transform = Transform::from_translation(
        view.cell_corner_to_world(origin.x, origin.y).extend(IMAGE_Z),
    );
    let size = Vec2::new(across as f32, down as f32) * cells_per_pixel * view.cell_size;
    let colour = Color::WHITE.with_alpha(declared.opacity());

    match sprites.iter_mut().next() {
        Some((_, mut at, mut sprite)) => {
            if *at != transform {
                *at = transform;
            }
            if sprite.custom_size != Some(size) {
                sprite.custom_size = Some(size);
            }
            if sprite.color != colour {
                sprite.color = colour;
            }
        }
        None => {
            commands.spawn((
                BackdropImage,
                Sprite {
                    image: held.image.clone(),
                    color: colour,
                    custom_size: Some(size),
                    ..default()
                },
                Anchor::TOP_LEFT,
                transform,
            ));
        }
    }
}

/// Whether there is a document and a backdrop to draw a picture against.
pub fn a_document_is_open(
    doc: Option<Res<WorldDoc>>,
    backdrop: Option<Res<Backdrop>>,
) -> bool {
    doc.is_some() && backdrop.is_some()
}

fn read_picture(path: PathBuf) -> Result<Image, String> {
    let meta = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    if !meta.is_file() {
        return Err("it is not a regular file".to_owned());
    }
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "it is {} bytes, over the {MAX_IMAGE_BYTES} byte limit",
            meta.len()
        ));
    }

    let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
    Image::from_buffer(
        &bytes,
        ImageType::Extension(
            path.extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("png"),
        ),
        default(),
        true,
        ImageSampler::linear(),
        RenderAssetUsages::RENDER_WORLD,
    )
    .map_err(|error| error.to_string())
}
