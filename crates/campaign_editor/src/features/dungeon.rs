//! Opening the dungeon a selected entry names, and getting back to the world map.
//!
//! A switch is one act with four parts, and they have to happen together: the document on
//! screen changes, what is selected on the old one stops meaning anything, the backdrop the
//! camera and the chunks are measured against changes, and the camera goes somewhere. Only
//! the last is not done here — the backdrop carries where the camera should go and
//! [`place_camera`](crate::map::camera::place_camera) puts it there, because this set runs
//! after the map's and a transform written here would be overwritten on the next frame.
//!
//! Feature ids are per-document, so a selection carried across a switch would silently name
//! a different feature — world-map feature 7 becoming dungeon feature 7 — and the
//! reconcile pass would not catch it, since it drops only ids the new document does not
//! hold. Everything holding an id is therefore cleared here rather than left to reconcile.
//!
//! The buttons are read the way every other button in this editor is: an observer writes
//! what was pressed into [`DungeonIntent`] and a system consumes it. No system reads a
//! button.

use bevy::prelude::*;
use campaign::edit::Edit;
use campaign::feature::{FeatureId, FeatureKind};
use campaign::grid::{DEFAULT_GRID_CELLS, DEFAULT_METRES_PER_CELL, TileGrid};
use campaign::world::World;
use campaign::{Document, layout, slug};

use crate::document::{self, WorldDoc};
use crate::map::backdrop::{Backdrop, BackdropSource, CameraBookmark};
use crate::map::camera::MapCamera;
use crate::map::load::MapTerrain;
use crate::map::view::MapView;
use crate::{OpenCampaign, StatusMessage};

/// What the GM asked for, written by a button's observer and consumed by a system.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DungeonIntent {
    #[default]
    Nothing,
    /// Open the dungeon the selected entry names, creating it if it has none.
    Enter,
    /// Go back to the world map.
    Leave,
}

/// Whether anything was asked for.
pub fn a_switch_was_asked_for(intent: Res<DungeonIntent>) -> bool {
    *intent != DungeonIntent::Nothing
}

/// Opens the selected dungeon entry's document, or goes back to the world map.
///
/// Creating a dungeon on first open is two changes to the world map and they are separate:
/// the link is an [`Edit`] so it is undoable and is saved with the world map, and the
/// document itself is not written until the GM saves it. A dungeon that has been opened and
/// not saved therefore has a name and no file, which is exactly why the name has to be
/// unique against the names the document already holds as well as against the directory.
pub fn switch_document(
    mut intent: ResMut<DungeonIntent>,
    campaign: Res<OpenCampaign>,
    terrain: Res<MapTerrain>,
    mut doc: ResMut<WorldDoc>,
    mut backdrop: ResMut<Backdrop>,
    mut selection: ResMut<crate::features::select::Selection>,
    mut dragging: ResMut<crate::features::select::Dragging>,
    mut drafting: ResMut<crate::features::draw::Drafting>,
    mut stroking: ResMut<crate::features::paint::Stroking>,
    mut pending: ResMut<crate::features::panel::PendingLabel>,
    mut asking: ResMut<crate::features::prompt::Asking>,
    mut active: ResMut<crate::features::tool::ActiveTool>,
    mut status: ResMut<StatusMessage>,
    camera: Single<(&Transform, &Projection), With<MapCamera>>,
) {
    let asked = std::mem::take(&mut *intent);
    let (transform, projection) = camera.into_inner();
    let here = bookmark_of(transform, projection);

    match asked {
        DungeonIntent::Nothing => return,
        DungeonIntent::Leave => {
            if !doc.in_a_dungeon() {
                status.say("this is the world map");
                return;
            }
            let path = layout::world(campaign.0.root());
            let restore = doc.switch_to(path, None, here, || {
                unreachable!("the world map is parked whenever a dungeon is on screen")
            });
            let cell_size = backdrop.view.cell_size;
            backdrop.switch_to(
                MapView::new(terrain.width, terrain.height, cell_size),
                BackdropSource::Terrain,
                restore,
            );
            status.say("back to the world map");
        }
        DungeonIntent::Enter => {
            if doc.in_a_dungeon() {
                status.say("a dungeon does not open another; go back to the map first");
                return;
            }
            let Some(entry) = the_selected_entry(&doc, &selection, &mut status) else {
                return;
            };

            let name = match doc.document.world().feature(entry).and_then(|f| f.dungeon.clone()) {
                Some(name) => name,
                None => {
                    let Some(name) = name_for(&doc, entry, campaign.0.root()) else {
                        status.say("this entry has no label a file name can be made from");
                        return;
                    };
                    if !document::apply(
                        &mut doc,
                        &mut status,
                        Edit::SetDungeon {
                            id: entry,
                            dungeon: Some(name.clone()),
                        },
                    ) {
                        return;
                    }
                    name
                }
            };

            let path = layout::dungeon(campaign.0.root(), &name);
            let resumed = doc.is_parked(&path);
            let mut refused = None;
            let restore = doc.switch_to(path.clone(), Some(entry), here, || {
                match Document::load(&path) {
                    Ok(document) if document.world().grid().is_none() => {
                        if document.world().is_empty() {
                            Document::new(World::on_a_grid(default_grid()))
                        } else {
                            refused = Some(format!(
                                "`{}` is a map document with no grid, so it is not a dungeon",
                                path.display()
                            ));
                            Document::new(World::on_a_grid(default_grid()))
                        }
                    }
                    Ok(document) => document,
                    Err(error) => {
                        refused = Some(error.to_string());
                        Document::new(World::on_a_grid(default_grid()))
                    }
                }
            });

            if let Some(reason) = refused {
                let world = layout::world(campaign.0.root());
                doc.switch_to(world, None, None, || {
                    unreachable!("the world map was parked a moment ago")
                });
                status.say(reason);
                return;
            }

            let grid = doc
                .document
                .world()
                .grid()
                .expect("a dungeon document carries a grid");
            let cell_size = backdrop.view.cell_size;
            backdrop.switch_to(
                MapView::new(grid.width(), grid.height(), cell_size),
                BackdropSource::Grid,
                restore,
            );
            status.say(if resumed {
                format!("back in {name}")
            } else {
                format!("opened {name}")
            });
        }
    }

    selection.clear();
    dragging.cancel();
    drafting.abandon();
    stroking.abandon();
    pending.feature = None;
    pending.text.clear();
    asking.settle();
    active.leave_a_grid_if(!doc.in_a_dungeon());
}

fn default_grid() -> TileGrid {
    TileGrid::new(
        DEFAULT_GRID_CELLS,
        DEFAULT_GRID_CELLS,
        DEFAULT_METRES_PER_CELL,
    )
    .expect("the default extent and scale are legal by construction")
}

fn bookmark_of(transform: &Transform, projection: &Projection) -> Option<CameraBookmark> {
    let Projection::Orthographic(orthographic) = projection else {
        return None;
    };
    Some(CameraBookmark {
        translation: transform.translation.truncate(),
        scale: orthographic.scale,
    })
}

fn the_selected_entry(
    doc: &WorldDoc,
    selection: &crate::features::select::Selection,
    status: &mut StatusMessage,
) -> Option<FeatureId> {
    let mut chosen = None;
    for id in selection.features.iter().copied() {
        if doc.document.world().feature(id).is_some_and(|feature| {
            feature.kind == FeatureKind::DungeonEntry
        }) {
            if chosen.is_some() {
                status.say("select one dungeon entry, not several");
                return None;
            }
            chosen = Some(id);
        }
    }
    if chosen.is_none() {
        status.say("select a dungeon entry first");
    }
    chosen
}

fn name_for(doc: &WorldDoc, entry: FeatureId, root: &std::path::Path) -> Option<String> {
    let label = doc
        .document
        .world()
        .feature(entry)
        .map(|feature| feature.label.clone())
        .unwrap_or_default();

    let mut taken: Vec<String> = doc
        .document
        .world()
        .features()
        .filter_map(|(_, feature)| feature.dungeon.clone())
        .collect();
    if let Ok(entries) = std::fs::read_dir(layout::dungeons(root)) {
        taken.extend(
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().into_string().ok()),
        );
    }

    Some(slug::dungeon_name(
        &label,
        entry,
        taken.iter().map(String::as_str),
    ))
}
