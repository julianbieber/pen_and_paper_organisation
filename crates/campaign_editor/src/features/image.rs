//! Placing the picture a document is drawn over: a drag, a corner scale, a two-point
//! calibration, an opacity, and getting an imported file into the campaign.
//!
//! Authoring rather than map, though what it moves is drawn under every feature. Every one
//! of these lands an [`Edit`] on the open document, and a map-set system writing the
//! document would race the authoring set's reads of it in the same frame.
//!
//! The gesture is held in a resource for the reason a drag and a stroke are: Escape has to
//! be able to abandon it and the control socket has to be able to drive one without a
//! mouse. The **press** is gated on the tool and the pointer, never the system — a run
//! condition would abandon the gesture the moment the cursor crossed the tool strip and
//! the release would never be seen.
//!
//! Everything lands on release, and each whole gesture is one [`Edit`]: a drag, a scale, a
//! calibration and an opacity drag are each a single press of undo. Nothing is applied
//! while the button is held, so a gesture also costs one change-detection wake rather than
//! one a frame — which matters here because three systems and a `zk` query watch the open
//! document for changes.
//!
//! Every sum is [`campaign::image`]'s. What is left here is the gesture.

use std::path::PathBuf;

use bevy::feathers::controls::{
    FeathersButton, FeathersSlider, FeathersTextInput, FeathersTextInputContainer,
};
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::scene::CommandsSceneExt;
use bevy::tasks::{IoTaskPool, Task, block_on, futures_lite::future};
use bevy::text::{EditableText, TextEditChange};
use bevy::ui_widgets::{Activate, SliderValue, slider_self_update};
use campaign::edit::Edit;
use campaign::feature::CellPoint;
use campaign::image::{self, ImageBackdrop, Imported};

use crate::document::{self, WorldDoc};
use crate::features::PointerOverUi;
use crate::features::tool::ActiveTool;
use crate::map::backdrop::Backdrop;
use crate::map::image::ImageAsset;
use crate::map::pointer::MapPointer;
use crate::{OpenCampaign, StatusMessage};

const HANDLE_PIXELS: f32 = 8.0;

/// What the gesture in flight is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacingKind {
    Moving,
    Scaling,
    Calibrating,
}

/// The placement gesture in flight, if there is one.
///
/// `what` is an `Option` rather than the enum carrying an idle variant, matching
/// [`Dragging`](crate::features::select::Dragging): two ways to say "nothing in flight" can
/// disagree, and one of them is then wrong.
#[derive(Resource, Debug, Default)]
pub struct Placing {
    what: Option<PlacingKind>,
    /// Moving: the offset from the pointer to the picture's origin when it was grabbed.
    grab: Option<CellPoint>,
    /// Scaling: the corner that stays put, as a pixel of the picture and the cell it is on.
    anchor: Option<((f32, f32), CellPoint)>,
    /// Scaling: the pixel of the picture being dragged.
    dragged: Option<(f32, f32)>,
    /// What this gesture would commit, recomputed on every frame the pointer moves.
    preview: Option<(CellPoint, f32)>,
    /// Calibrating: the two places the GM says they know the distance between.
    first_mark: Option<CellPoint>,
    second_mark: Option<CellPoint>,
}

impl Placing {
    /// Whether a gesture is in flight.
    pub fn is_live(&self) -> bool {
        self.what.is_some()
    }

    /// Whether the gesture in flight is a calibration.
    pub fn is_calibrating(&self) -> bool {
        self.what == Some(PlacingKind::Calibrating)
    }

    /// What the gesture would commit, or `None` when nothing is in flight.
    ///
    /// What the picture is drawn at while a gesture is live, so that what is shown and
    /// what would be committed cannot disagree.
    pub fn preview(&self) -> Option<(CellPoint, f32)> {
        self.preview
    }

    /// The two marks placed so far.
    pub fn marks(&self) -> (Option<CellPoint>, Option<CellPoint>) {
        (self.first_mark, self.second_mark)
    }

    /// Begin a calibration, forgetting any marks a previous one left.
    pub fn calibrate(&mut self) {
        self.cancel();
        self.what = Some(PlacingKind::Calibrating);
    }

    /// Take `at` as the next calibration mark, and say whether both are now placed.
    pub fn mark(&mut self, at: CellPoint) -> bool {
        match self.first_mark {
            None => {
                self.first_mark = Some(at);
                false
            }
            Some(_) => {
                self.second_mark = Some(at);
                true
            }
        }
    }

    /// Drop the gesture without applying it, as Escape, a tool change and a document
    /// switch all do.
    pub fn cancel(&mut self) {
        self.what = None;
        self.grab = None;
        self.anchor = None;
        self.dragged = None;
        self.preview = None;
        self.first_mark = None;
        self.second_mark = None;
    }
}

/// A path the GM has asked to import, waiting to be acted on.
///
/// An intent resource rather than an observer writing the document, matching
/// [`DungeonIntent`](crate::features::dungeon::DungeonIntent), so the control socket drives
/// the same path a button does.
#[derive(Resource, Debug, Default)]
pub struct ImportRequest {
    pub path: Option<String>,
}

/// The distance the GM has given for the two calibration marks, waiting to be acted on.
#[derive(Resource, Debug, Default)]
pub struct CalibrationDistance {
    pub apart: Option<f32>,
}

/// The copy in flight, and the document it was started for.
///
/// The document is carried for the reason a note job carries one: a copy takes long enough
/// for the GM to leave for another document, and a declaration landing on the wrong one
/// would name a picture that document never asked for.
#[derive(Resource, Default)]
pub struct ImportJob {
    task: Option<Task<Result<Imported, String>>>,
    document: Option<PathBuf>,
}

impl ImportJob {
    /// Whether a copy is running. Nothing may start another while this is true.
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }
}

/// Copies the file the GM named into the campaign's images directory and declares it on
/// the open document.
///
/// The copy runs off the frame, because a scanned map is tens of megabytes and the map must
/// not stutter while one is written. The declaration lands only once the bytes are down: a
/// document must never name a file that was not written.
pub fn import_image(
    open: Res<OpenCampaign>,
    backdrop: Res<Backdrop>,
    mut asked: ResMut<ImportRequest>,
    mut job: ResMut<ImportJob>,
    mut doc: ResMut<WorldDoc>,
    mut status: ResMut<StatusMessage>,
) {
    if let Some(path) = asked.path.take() {
        if job.busy() {
            status.say("an import is already running");
        } else {
            let root = open.0.root().to_owned();
            let source = PathBuf::from(path);
            job.document = Some(doc.path.clone());
            job.task = Some(IoTaskPool::get().spawn(async move {
                image::import(&root, &source).map_err(|error| error.to_string())
            }));
            status.say("importing…");
        }
    }

    let Some(task) = job.task.as_mut() else {
        return;
    };
    let Some(landed) = block_on(future::poll_once(task)) else {
        return;
    };
    job.task = None;
    let started_on = job.document.take();

    let imported = match landed {
        Ok(imported) => imported,
        Err(reason) => {
            status.say(reason);
            return;
        }
    };

    if started_on.as_ref() != Some(&doc.path) {
        status.say(format!(
            "`{}` was imported, and the document it was for is no longer on screen",
            imported.name
        ));
        return;
    }

    let view = backdrop.view;
    let (origin, cells_per_pixel) =
        ImageBackdrop::fit_to(view.width, view.height, imported.size_in_pixels);
    let declared = match ImageBackdrop::new(&imported.name, origin, cells_per_pixel, 1.0) {
        Ok(declared) => declared,
        Err(problem) => {
            status.say(problem.to_string());
            return;
        }
    };

    if document::apply(
        &mut doc,
        &mut status,
        Edit::SetImage {
            image: Some(declared),
        },
    ) {
        status.say(format!("imported `{}`", imported.name));
    }
}

/// Carries a move, a corner scale or a two-point calibration until it lands as one
/// [`Edit::PlaceImage`].
///
/// A press inside the picture moves it; a press within a handle's reach of a corner scales
/// it about the opposite corner. Either way nothing reaches the document until the button
/// is let go.
///
/// A calibration is the third shape and does not use the button at all: two clicks mark the
/// places, and the scale lands when a distance arrives. The marks are turned back into
/// pixels of the picture through the placement in force, so the answer does not depend on
/// where the picture currently sits, and the **first** mark is what stays put — a rescale
/// about anything else slides the landmark the GM lined up out from under whatever is drawn
/// on it. The distance is read in the open document's own unit, which is the grid's metres
/// in a dungeon and the manifest's units on the world map.
///
/// Does nothing while the picture has not been read, since a corner that is not known
/// cannot be pressed.
pub fn place_image(
    buttons: Res<ButtonInput<MouseButton>>,
    pointer: Res<MapPointer>,
    over_ui: Res<PointerOverUi>,
    active: Res<ActiveTool>,
    held: Res<ImageAsset>,
    mut apart: ResMut<CalibrationDistance>,
    mut placing: ResMut<Placing>,
    mut doc: ResMut<WorldDoc>,
    mut status: ResMut<StatusMessage>,
) {
    let Some(declared) = doc.document.world().image().cloned() else {
        if placing.is_live() {
            placing.cancel();
        }
        apart.apart = None;
        return;
    };
    let Some((across, down)) = held.size_in_pixels else {
        if placing.is_live() {
            placing.cancel();
        }
        return;
    };

    if placing.is_calibrating() {
        calibrating(
            &buttons, &pointer, &over_ui, &declared, &mut apart, &mut placing, &mut doc,
            &mut status,
        );
        return;
    }

    if !active.placing() {
        if placing.is_live() {
            placing.cancel();
        }
        return;
    }

    let at = pointer.cell;
    let slack = pointer.slack(HANDLE_PIXELS);

    if buttons.just_pressed(MouseButton::Left)
        && !over_ui.0
        && !placing.is_live()
        && let Some(at) = at
    {
        grab(&mut placing, &declared, (across, down), at, slack);
    }

    if !placing.is_live() {
        return;
    }

    if buttons.pressed(MouseButton::Left)
        && let Some(at) = at
    {
        placing.preview = pending(&placing, &declared, at);
    }

    if !buttons.just_released(MouseButton::Left) {
        return;
    }

    let landing = placing.preview;
    placing.cancel();
    let Some((origin, cells_per_pixel)) = landing else {
        return;
    };
    if document::apply(
        &mut doc,
        &mut status,
        Edit::PlaceImage {
            origin,
            cells_per_pixel,
        },
    ) {
        status.say("placed the backdrop");
    }
}

fn grab(
    placing: &mut Placing,
    declared: &ImageBackdrop,
    size_in_pixels: (u32, u32),
    at: CellPoint,
    slack: f32,
) {
    let (across, down) = (size_in_pixels.0 as f32, size_in_pixels.1 as f32);
    let corners = [(0.0, 0.0), (across, 0.0), (0.0, down), (across, down)];

    for (index, (x, y)) in corners.iter().enumerate() {
        let corner = declared.cell_of_pixel(*x, *y);
        if (corner.x - at.x).abs() <= slack && (corner.y - at.y).abs() <= slack {
            let (ax, ay) = corners[3 - index];
            placing.what = Some(PlacingKind::Scaling);
            placing.anchor = Some(((ax, ay), declared.cell_of_pixel(ax, ay)));
            placing.dragged = Some((*x, *y));
            placing.preview = Some((declared.origin(), declared.cells_per_pixel()));
            return;
        }
    }

    let (x, y) = declared.pixel_of_cell(at);
    if x < 0.0 || y < 0.0 || x > across || y > down {
        return;
    }
    placing.what = Some(PlacingKind::Moving);
    placing.grab = Some(CellPoint::new(
        declared.origin().x - at.x,
        declared.origin().y - at.y,
    ));
    placing.preview = Some((declared.origin(), declared.cells_per_pixel()));
}

fn pending(
    placing: &Placing,
    declared: &ImageBackdrop,
    at: CellPoint,
) -> Option<(CellPoint, f32)> {
    match placing.what? {
        PlacingKind::Moving => {
            let grab = placing.grab?;
            Some((
                CellPoint::new(at.x + grab.x, at.y + grab.y),
                declared.cells_per_pixel(),
            ))
        }
        PlacingKind::Scaling => {
            let ((ax, ay), anchor) = placing.anchor?;
            let (dx, dy) = placing.dragged?;
            let corner_apart = ((dx - ax).powi(2) + (dy - ay).powi(2)).sqrt();
            let pointer_apart =
                ((at.x - anchor.x).powi(2) + (at.y - anchor.y).powi(2)).sqrt();
            if corner_apart <= 0.0 {
                return None;
            }
            let cells_per_pixel = pointer_apart / corner_apart;
            image::scale_refusal(cells_per_pixel)?;
            Some((
                declared.anchored(ax, ay, anchor, cells_per_pixel),
                cells_per_pixel,
            ))
        }
        PlacingKind::Calibrating => None,
    }
}

fn calibrating(
    buttons: &ButtonInput<MouseButton>,
    pointer: &MapPointer,
    over_ui: &PointerOverUi,
    declared: &ImageBackdrop,
    apart: &mut CalibrationDistance,
    placing: &mut Placing,
    doc: &mut WorldDoc,
    status: &mut StatusMessage,
) {
    if buttons.just_pressed(MouseButton::Left)
        && !over_ui.0
        && let Some(at) = pointer.cell
    {
        if placing.mark(at) {
            status.say("both marks placed; give the distance between them");
        } else {
            status.say("first mark placed; click the second");
        }
    }

    let Some(distance) = apart.apart.take() else {
        return;
    };
    let (Some(first), Some(second)) = placing.marks() else {
        status.say("place both marks before giving a distance");
        return;
    };

    let first_pixel = declared.pixel_of_cell(first);
    let second_pixel = declared.pixel_of_cell(second);
    let units_per_cell = units_per_cell(doc);

    let cells_per_pixel = match image::calibrate(first_pixel, second_pixel, distance, units_per_cell)
    {
        Ok(scale) => scale,
        Err(problem) => {
            status.say(problem.to_string());
            return;
        }
    };
    let origin = declared.anchored(first_pixel.0, first_pixel.1, first, cells_per_pixel);

    placing.cancel();
    if document::apply(
        doc,
        status,
        Edit::PlaceImage {
            origin,
            cells_per_pixel,
        },
    ) {
        status.say(format!("calibrated: one cell is {units_per_cell}"));
    }
}

fn units_per_cell(doc: &WorldDoc) -> f32 {
    doc.document
        .world()
        .grid()
        .map(|grid| grid.metres_per_cell())
        .unwrap_or(1.0)
}

/// The slider the backdrop's opacity is read from.
#[derive(Component, Default, Clone)]
pub struct OpacitySlider;

/// The panel's root, so it can be taken off screen where it controls nothing.
#[derive(Component, Default, Clone)]
pub struct ImagePanel;

/// The rows that control a picture, hidden while the document declares none.
#[derive(Component, Default, Clone)]
pub struct DeclaredRow;

/// What has been typed into the panel's two fields.
///
/// Written on every keystroke by the fields themselves and read only when the GM presses
/// Enter, which is the idiom the campaign dialog and the label field both use: a keystroke
/// is not an instruction.
#[derive(Resource, Debug, Default)]
pub struct ImageFields {
    pub path: String,
    pub distance: String,
}

/// Hangs the backdrop controls over the map, at the opacity the open document declares.
///
/// The slider is spawned already showing the document's own value, for the reason the
/// river panel spawns already showing the threshold: a control that started somewhere else
/// and was corrected a frame later would land its spawn default on the document in
/// between.
pub fn build_image_panel(mut commands: Commands, doc: Res<WorldDoc>) {
    let opacity = doc
        .document
        .world()
        .image()
        .map_or(1.0, ImageBackdrop::opacity);
    commands.spawn_scene(panel(opacity));
}

/// Shows the backdrop controls while they control something, and keeps the slider showing
/// what the document says.
///
/// The panel is up whenever the image tool is in hand — that is how a picture is imported
/// in the first place, when there is none — and the rows that act on a declared picture are
/// hidden until there is one, so no control is ever up over nothing.
///
/// The slider is written back **only on the frame the document changed**, never merely
/// because the two differ. Writing whenever they differ is a loop with the thing that reads
/// it: every frame it would undo whatever moved the slider — the GM's own drag, or the
/// control socket — and the value could never reach the document at all.
pub fn show_image_panel(
    mut commands: Commands,
    doc: Res<WorldDoc>,
    active: Res<ActiveTool>,
    mut panels: Query<&mut Node, (With<ImagePanel>, Without<DeclaredRow>)>,
    mut rows: Query<&mut Node, (With<DeclaredRow>, Without<ImagePanel>)>,
    sliders: Query<(Entity, &SliderValue), With<OpacitySlider>>,
) {
    let declared = doc.document.world().image().map(ImageBackdrop::opacity);
    show(&mut panels, active.placing() || declared.is_some());
    show(&mut rows, declared.is_some());

    if !doc.is_changed() {
        return;
    }
    let Some(opacity) = declared else {
        return;
    };
    for (entity, value) in sliders.iter() {
        if value.0 != opacity {
            commands.entity(entity).insert(SliderValue(opacity));
        }
    }
}

fn show<F: bevy::ecs::query::QueryFilter>(nodes: &mut Query<&mut Node, F>, shown: bool) {
    let display = if shown { Display::Flex } else { Display::None };
    for mut node in nodes.iter_mut() {
        if node.display != display {
            node.display = display;
        }
    }
}

/// Lands the opacity slider on the document when the drag ends.
///
/// On release rather than on every change, which is the whole point: the slider moves every
/// frame it is dragged, and an [`Edit`] per frame would fill an undo stack that holds a
/// hundred and twenty-eight entries and discard everything the GM drew before it. It would
/// also wake the property panel and a `zk` query once a frame, which watch the open
/// document for changes. While the button is held the value is only shown.
///
/// The condition is that the button is **not down**, rather than that it was just let go:
/// the two agree for a drag, and only the first also lands a value the control socket
/// wrote, which has no mouse to release. Once landed the slider and the document agree, so
/// there is nothing to land again.
pub fn land_opacity(
    buttons: Res<ButtonInput<MouseButton>>,
    sliders: Query<&SliderValue, With<OpacitySlider>>,
    mut doc: ResMut<WorldDoc>,
    mut status: ResMut<StatusMessage>,
) {
    if buttons.pressed(MouseButton::Left) {
        return;
    }
    let Some(declared) = doc.document.world().image().map(ImageBackdrop::opacity) else {
        return;
    };
    let Some(value) = sliders.iter().next().map(|value| value.0) else {
        return;
    };
    if value == declared {
        return;
    }
    document::apply(
        &mut doc,
        &mut status,
        Edit::SetImageOpacity { opacity: value },
    );
}

/// Acts on what was typed, when the GM presses Enter in one of the two fields.
pub fn commit_image_fields(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    paths: Query<(), With<ImportField>>,
    distances: Query<(), With<DistanceField>>,
    fields: Res<ImageFields>,
    mut asked: ResMut<ImportRequest>,
    mut apart: ResMut<CalibrationDistance>,
    mut status: ResMut<StatusMessage>,
) {
    if !keys.just_pressed(KeyCode::Enter) && !keys.just_pressed(KeyCode::NumpadEnter) {
        return;
    }
    let Some(entity) = focus.get() else {
        return;
    };

    if paths.contains(entity) {
        let typed = fields.path.trim();
        if !typed.is_empty() {
            asked.path = Some(typed.to_owned());
        }
    } else if distances.contains(entity) {
        let typed = fields.distance.trim();
        if typed.is_empty() {
            return;
        }
        match typed.parse::<f32>() {
            Ok(distance) => apart.apart = Some(distance),
            Err(_) => status.say(format!("`{typed}` is not a distance")),
        }
    }
}

/// The field an import path is typed into.
#[derive(Component, Default, Clone)]
pub struct ImportField;

/// The field a calibration distance is typed into.
#[derive(Component, Default, Clone)]
pub struct DistanceField;

fn panel(opacity: f32) -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            bottom: px(76),
            left: px(12),
            display: Display::None,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
            padding: px(8),
            width: px(560),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        ImagePanel
        Children [
            (Text("Import") ThemedText),
            (
                @FeathersTextInputContainer
                Node { width: px(200) }
                Children [
                    (
                        @FeathersTextInput
                        ImportField
                        on(|change: On<TextEditChange>,
                            texts: Query<&EditableText>,
                            mut fields: ResMut<ImageFields>| {
                            if let Ok(text) = texts.get(change.event_target()) {
                                fields.path = text.value().to_string();
                            }
                        })
                    )
                ]
            ),
            (
                Node {
                    display: Display::None,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                }
                DeclaredRow
                Children [
                    (Text("Fade") ThemedText),
                    (
                        @FeathersSlider {
                            @value: {opacity},
                            @min: 0.0,
                            @max: 1.0,
                        }
                        OpacitySlider
                        on(slider_self_update)
                    ),
                    (
                        @FeathersButton {
                            @caption: bsn! { Text("Calibrate") ThemedText },
                        }
                        on(|_activate: On<Activate>, mut placing: ResMut<Placing>, mut status: ResMut<StatusMessage>| {
                            placing.calibrate();
                            status.say("click the first of two places you know the distance between");
                        })
                    ),
                    (
                        @FeathersTextInputContainer
                        Node { width: px(90) }
                        Children [
                            (
                                @FeathersTextInput
                                DistanceField
                                on(|change: On<TextEditChange>,
                                    texts: Query<&EditableText>,
                                    mut fields: ResMut<ImageFields>| {
                                    if let Ok(text) = texts.get(change.event_target()) {
                                        fields.distance = text.value().to_string();
                                    }
                                })
                            )
                        ]
                    )
                ]
            )
        ]
    }
}
