//! What the socket understands, and how each verb decides it has finished.
//!
//! Every verb that authors something works the way the GM does: it puts the pointer
//! somewhere and presses a button or a key, then lets the ordinary systems run. None of
//! them reaches into the document. That is deliberate and it is the whole value of the
//! socket — a scripted run proves something about the tool only if it goes through the
//! tool.

use std::path::PathBuf;
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use bevy::camera::Projection;
use campaign::draft::DraftShape;
use campaign::edit::Edit;
use campaign::feature::{CellPoint, FeatureKind, Rank};
use serde_json::{Value, json};

use bevy::input_focus::InputFocus;
use bevy::text::EditableText;

use crate::features::PointerOverUi;
use crate::features::doc::WorldDoc;
use crate::features::draw::Drafting;
use crate::features::prompt::Asking;
use crate::features::select::Selection;
use crate::features::tool::{ActiveTool, Tool};
use crate::features::doc as world_doc;
use crate::map::camera::MapCamera;
use crate::map::load::{MapAssets, MapTerrain};
use crate::map::pointer::{MapPointer, PointerOverride};
use crate::map::view::MapView;
use crate::StatusMessage;

/// How far a command has got.
pub(super) enum Poll {
    /// Not finished. Ask again next frame.
    Running,
    /// Finished, with the fields to send back.
    Done(Value),
    /// Finished badly. The message is sent instead of the fields.
    Failed(String),
}

/// A chord of keys pressed together for one frame.
#[derive(Debug, Clone)]
pub(super) struct Chord {
    keys: Vec<KeyCode>,
}

/// One thing the socket can be asked to do.
pub(super) enum Command {
    /// Answers immediately. Proves the socket.
    Ping,
    /// Lets that many frames pass.
    Step(u32),
    /// Chooses the tool, exactly as the tool strip's buttons do.
    Tool { tool: Tool, shape: DraftShape },
    /// Chooses what the active drawing tool will place.
    Kind(FeatureKind),
    /// Sets the selected feature's rank, or clears it.
    ///
    /// Goes through the same [`Edit`] the panel's buttons build, so a scripted run cannot
    /// set a rank in a way a GM could not, and the change undoes like any other.
    SetRank(Option<Rank>),
    /// Sets the selected feature's reveal threshold to the map's current scale, or clears
    /// it. `None` for the scale means "here", which is what the panel's button does.
    SetReveal(Option<Option<f32>>),
    /// Zooms the camera to a given map scale in cells per logical pixel, clamped to what
    /// the terrain allows.
    ///
    /// The one verb that is not a thing the GM does with a button, and it exists because
    /// the whole of issue #5 is about what changes across a zoom: without it there is no
    /// way to script the reveal at all.
    Zoom(f32),
    /// Puts the pointer over a terrain cell, and leaves it there.
    At(CellPoint),
    /// Presses and releases the left button, optionally moving the pointer first.
    Click { at: Option<CellPoint>, stage: u8 },
    /// Presses at one cell, moves to another, and releases — one whole gesture.
    Drag {
        from: CellPoint,
        to: CellPoint,
        stage: u8,
    },
    /// Presses a chord of keys for one frame and releases it.
    Key { chord: Chord, stage: u8 },
    /// Answers a question about what the editor currently holds — including, through
    /// [`Topic::Input`], every reason a press might author nothing, since a press that
    /// lands on no system is indistinguishable from one that was never delivered and a
    /// scripted run cannot see the screen.
    Observe(Topic),
    /// Writes a PNG of the window and waits until it is on disk.
    Capture {
        path: PathBuf,
        /// The screenshot entity, once spawned. Its *absence from the world* is what
        /// says the PNG is written, so the reply never races a half-written file.
        entity: Option<Entity>,
    },
    /// Pins the frame delta, so a scripted run is reproducible.
    FixedDelta(Duration),
    /// Asks the editor to exit.
    Quit,
}

/// What [`Command::Observe`] can be asked about.
pub(super) enum Topic {
    /// Every feature the document holds.
    World,
    /// What is selected, and what is being drawn.
    Selection,
    /// Which tool is in hand.
    Tool,
    /// Why a press would or would not author anything.
    Input,
}

impl Command {
    /// The word this command was parsed from, for the reply.
    pub(super) fn verb(&self) -> &'static str {
        match self {
            Self::Ping => "ping",
            Self::Step(_) => "step",
            Self::Tool { .. } => "tool",
            Self::Kind(_) => "kind",
            Self::SetRank(_) => "rank",
            Self::SetReveal(_) => "reveal",
            Self::Zoom(_) => "zoom",
            Self::At(_) => "at",
            Self::Click { .. } => "click",
            Self::Drag { .. } => "drag",
            Self::Key { .. } => "key",
            Self::Observe(_) => "observe",
            Self::Capture { .. } => "capture",
            Self::FixedDelta(_) => "fixed-delta",
            Self::Quit => "quit",
        }
    }

    /// The command `line` describes, or why it is not one.
    ///
    /// Whitespace-separated words; the verb first. Every failure names what was wrong
    /// with the line rather than restating it, because a scenario file's error is read
    /// far from the line that caused it.
    pub(super) fn parse(line: &str) -> Result<Self, String> {
        let mut words = line.split_whitespace();
        let verb = words.next().ok_or("empty command")?;
        let rest: Vec<&str> = words.collect();

        match verb {
            "ping" => Ok(Self::Ping),
            "step" => Ok(Self::Step(match rest.first() {
                Some(word) => word.parse().map_err(|_| format!("{word} is not a frame count"))?,
                None => 1,
            })),
            "tool" => {
                let word = rest.first().ok_or("tool needs a name")?;
                let (tool, shape) = match *word {
                    "select" => (Tool::Select, DraftShape::Point),
                    "point" => (Tool::Draw, DraftShape::Point),
                    "line" | "polyline" => (Tool::Draw, DraftShape::Polyline),
                    "area" | "polygon" => (Tool::Draw, DraftShape::Polygon),
                    other => return Err(format!("there is no {other} tool")),
                };
                Ok(Self::Tool { tool, shape })
            }
            "kind" => Ok(Self::Kind(kind(rest.first().ok_or("kind needs a name")?)?)),
            "rank" => Ok(Self::SetRank(match *rest.first().ok_or("rank needs a name")? {
                "none" | "clear" => None,
                word => Some(rank(word)?),
            })),
            "reveal" => Ok(Self::SetReveal(match rest.first().copied() {
                None | Some("here") => None,
                Some("always" | "clear" | "none") => Some(None),
                Some(word) => Some(Some(
                    word.parse()
                        .map_err(|_| format!("{word} is not a scale in cells per pixel"))?,
                )),
            })),
            "zoom" => {
                let word = rest.first().ok_or("zoom needs a scale in cells per pixel")?;
                Ok(Self::Zoom(word.parse().map_err(|_| {
                    format!("{word} is not a scale in cells per pixel")
                })?))
            }
            "at" => Ok(Self::At(cell(&rest, 0)?)),
            "click" => Ok(Self::Click {
                at: if rest.is_empty() {
                    None
                } else {
                    Some(cell(&rest, 0)?)
                },
                stage: 0,
            }),
            "drag" => Ok(Self::Drag {
                from: cell(&rest, 0)?,
                to: cell(&rest, 2)?,
                stage: 0,
            }),
            "key" => Ok(Self::Key {
                chord: chord(rest.first().ok_or("key needs a name")?)?,
                stage: 0,
            }),
            "undo" => Ok(Self::Key {
                chord: chord("ctrl+z")?,
                stage: 0,
            }),
            "redo" => Ok(Self::Key {
                chord: chord("ctrl+shift+z")?,
                stage: 0,
            }),
            "save" => Ok(Self::Key {
                chord: chord("ctrl+s")?,
                stage: 0,
            }),
            "observe" => Ok(Self::Observe(match rest.first().copied().unwrap_or("world") {
                "world" => Topic::World,
                "selection" => Topic::Selection,
                "tool" => Topic::Tool,
                "input" => Topic::Input,
                other => return Err(format!("nothing to observe called {other}")),
            })),
            "capture" => Ok(Self::Capture {
                path: PathBuf::from(rest.first().ok_or("capture needs a path")?),
                entity: None,
            }),
            "fixed-delta" => {
                let word = rest.first().ok_or("fixed-delta needs a number of seconds")?;
                let seconds: f32 = word
                    .parse()
                    .map_err(|_| format!("{word} is not a number of seconds"))?;
                Ok(Self::FixedDelta(Duration::from_secs_f32(seconds)))
            }
            "quit" => Ok(Self::Quit),
            other => Err(format!("there is no command called {other}")),
        }
    }

    /// Advance this command, `elapsed` frames after it was received.
    ///
    /// [`Poll::Running`] means ask again next frame; the client is still waiting, and the
    /// editor has run a whole frame in between — which is what lets a press and its
    /// release land on different frames, as they do for a real mouse.
    ///
    /// `tool` and `kind` make exactly the writes the tool strip's own observers make,
    /// abandoned draft included, so the socket cannot choose a tool a button could not.
    pub(super) fn poll(&mut self, world: &mut World, elapsed: u32) -> Poll {
        match self {
            Self::Ping => Poll::Done(json!({})),

            Self::Step(frames) => {
                if elapsed >= *frames {
                    Poll::Done(json!({}))
                } else {
                    Poll::Running
                }
            }

            Self::Tool { tool, shape } => {
                if let Some(mut drafting) = world.get_resource_mut::<Drafting>() {
                    drafting.abandon();
                }
                let Some(mut active) = world.get_resource_mut::<ActiveTool>() else {
                    return Poll::Failed("no campaign is open".into());
                };
                active.tool = *tool;
                if *tool == Tool::Draw {
                    active.shape = *shape;
                }
                Poll::Done(json!({}))
            }

            Self::Kind(kind) => {
                let Some(mut active) = world.get_resource_mut::<ActiveTool>() else {
                    return Poll::Failed("no campaign is open".into());
                };
                let shape = active.shape;
                match shape {
                    DraftShape::Point => active.point_kind = *kind,
                    DraftShape::Polyline => active.polyline_kind = *kind,
                    DraftShape::Polygon => active.polygon_kind = *kind,
                }
                Poll::Done(json!({}))
            }

            Self::SetRank(rank) => author(world, |id| Edit::SetRank { id, rank: *rank }),

            Self::SetReveal(scale) => {
                let scale = match scale {
                    Some(chosen) => *chosen,
                    None => match world.get_resource::<MapPointer>() {
                        Some(pointer) => Some(pointer.cells_per_pixel),
                        None => return Poll::Failed("no map is open".into()),
                    },
                };
                author(world, |id| Edit::SetMaxCellsPerPixel { id, scale })
            }

            Self::Zoom(cells_per_pixel) => zoom(world, *cells_per_pixel),

            Self::At(at) => {
                point_at(world, Some(*at));
                Poll::Done(json!({ "x": at.x, "y": at.y }))
            }

            Self::Click { at, stage } => match stage {
                0 => {
                    if let Some(at) = at {
                        point_at(world, Some(*at));
                    }
                    press_mouse(world, true);
                    *stage = 1;
                    Poll::Running
                }
                1 => {
                    press_mouse(world, false);
                    *stage = 2;
                    Poll::Running
                }
                _ => Poll::Done(json!({})),
            },

            Self::Drag { from, to, stage } => match stage {
                0 => {
                    point_at(world, Some(*from));
                    press_mouse(world, true);
                    *stage = 1;
                    Poll::Running
                }
                1 => {
                    point_at(world, Some(*to));
                    *stage = 2;
                    Poll::Running
                }
                2 => {
                    press_mouse(world, false);
                    *stage = 3;
                    Poll::Running
                }
                _ => Poll::Done(json!({})),
            },

            Self::Key { chord, stage } => match stage {
                0 => {
                    press_keys(world, &chord.keys, true);
                    *stage = 1;
                    Poll::Running
                }
                1 => {
                    press_keys(world, &chord.keys, false);
                    *stage = 2;
                    Poll::Running
                }
                _ => Poll::Done(json!({})),
            },

            Self::Observe(topic) => Poll::Done(observe(world, topic)),

            Self::Capture { path, entity } => match entity {
                None => {
                    if let Some(parent) = path.parent()
                        && let Err(error) = std::fs::create_dir_all(parent)
                    {
                        return Poll::Failed(format!("cannot create {}: {error}", parent.display()));
                    }
                    *entity = Some(
                        world
                            .spawn(Screenshot::primary_window())
                            .observe(save_to_disk(path.clone()))
                            .id(),
                    );
                    Poll::Running
                }
                Some(id) => {
                    if world.entities().contains(*id) {
                        Poll::Running
                    } else {
                        Poll::Done(json!({ "path": path.display().to_string() }))
                    }
                }
            },

            Self::FixedDelta(delta) => {
                world.insert_resource(TimeUpdateStrategy::ManualDuration(*delta));
                Poll::Done(json!({ "seconds": delta.as_secs_f32() }))
            }

            Self::Quit => {
                world.write_message(AppExit::Success);
                Poll::Done(json!({}))
            }
        }
    }
}

fn point_at(world: &mut World, at: Option<CellPoint>) {
    if let Some(mut forced) = world.get_resource_mut::<PointerOverride>() {
        forced.0 = at;
    }
}

fn press_mouse(world: &mut World, down: bool) {
    let Some(mut buttons) = world.get_resource_mut::<ButtonInput<MouseButton>>() else {
        return;
    };
    if down {
        buttons.press(MouseButton::Left);
    } else {
        buttons.release(MouseButton::Left);
    }
}

fn press_keys(world: &mut World, keys: &[KeyCode], down: bool) {
    let Some(mut input) = world.get_resource_mut::<ButtonInput<KeyCode>>() else {
        return;
    };
    for key in keys {
        if down {
            input.press(*key);
        } else {
            input.release(*key);
        }
    }
}

fn author(world: &mut World, build: impl Fn(campaign::FeatureId) -> Edit) -> Poll {
    let Some(id) = world
        .get_resource::<Selection>()
        .and_then(|selection| selection.only())
    else {
        return Poll::Failed("exactly one feature must be selected".into());
    };
    let edit = build(id);

    let Some(mut doc) = world.remove_resource::<WorldDoc>() else {
        return Poll::Failed("no campaign is open".into());
    };
    let Some(mut status) = world.remove_resource::<StatusMessage>() else {
        world.insert_resource(doc);
        return Poll::Failed("no status line".into());
    };
    let applied = world_doc::apply(&mut doc, &mut status, edit);
    let message = status.0.clone();
    world.insert_resource(doc);
    world.insert_resource(status);

    if applied {
        Poll::Done(json!({ "id": id.0 }))
    } else {
        Poll::Failed(message)
    }
}

fn zoom(world: &mut World, cells_per_pixel: f32) -> Poll {
    if !cells_per_pixel.is_finite() || cells_per_pixel <= 0.0 {
        return Poll::Failed("a scale must be a finite number greater than zero".into());
    }
    let Some(cell_size) = world
        .get_resource::<MapAssets>()
        .map(|assets| assets.tile_size as f32)
    else {
        return Poll::Failed("no map is open".into());
    };
    let Some((width, height)) = world
        .get_resource::<MapTerrain>()
        .map(|terrain| (terrain.width, terrain.height))
    else {
        return Poll::Failed("no map is open".into());
    };
    let view = MapView::new(width, height, cell_size);

    let mut cameras = world.query_filtered::<(&mut Projection, &Camera), With<MapCamera>>();
    let Ok((mut projection, camera)) = cameras.single_mut(world) else {
        return Poll::Failed("there is no map camera".into());
    };
    let Some(viewport) = crate::map::camera::viewport_of(camera) else {
        return Poll::Failed("the window has no viewport yet".into());
    };
    let Projection::Orthographic(orthographic) = &mut *projection else {
        return Poll::Failed("the map camera is not orthographic".into());
    };

    let (low, high) = crate::map::camera::zoom_bounds(view, viewport);
    orthographic.scale = (cells_per_pixel * cell_size).clamp(low, high);
    Poll::Done(json!({ "cells_per_pixel": orthographic.scale / cell_size }))
}

fn observe(world: &mut World, topic: &Topic) -> Value {
    if let Topic::Input = topic {
        return input(world);
    }
    let Some(doc) = world.get_resource::<WorldDoc>() else {
        return json!({ "open": false });
    };
    match topic {
        Topic::World => {
            let features: Vec<Value> = doc
                .document
                .world()
                .features()
                .map(|(id, feature)| {
                    json!({
                        "id": id.0,
                        "kind": format!("{:?}", feature.kind),
                        "shape": feature.geometry.shape(),
                        "label": feature.label,
                        "vertices": feature.geometry.len(),
                        "parent": feature.parent.map(|parent| parent.0),
                        "note": feature.note,
                        "rank": feature.rank.map(|rank| format!("{rank:?}")),
                        "max_cells_per_pixel": feature.max_cells_per_pixel,
                    })
                })
                .collect();
            json!({
                "open": true,
                "features": features,
                "dirty": doc.document.is_dirty(),
                "undo": doc.document.undo_depth(),
                "redo": doc.document.redo_depth(),
            })
        }
        Topic::Selection => {
            let selection = world.get_resource::<Selection>();
            let drafting = world.get_resource::<Drafting>();
            json!({
                "open": true,
                "features": selection
                    .map(|selection| selection.features.iter().map(|id| id.0).collect::<Vec<u64>>())
                    .unwrap_or_default(),
                "vertex": selection
                    .and_then(|selection| selection.vertex)
                    .map(|vertex| json!({ "feature": vertex.feature.0, "index": vertex.index })),
                "drafting": drafting.map(|drafting| {
                    drafting.draft.as_ref().map(|draft| json!({
                        "shape": draft.shape.shape(),
                        "vertices": draft.len(),
                    }))
                }),
            })
        }
        Topic::Input => unreachable!("answered before the document is looked for"),
        Topic::Tool => {
            let active = world.get_resource::<ActiveTool>();
            json!({
                "open": true,
                "tool": active.map(|active| match active.tool {
                    Tool::Select => "select",
                    Tool::Draw => match active.shape {
                        DraftShape::Point => "point",
                        DraftShape::Polyline => "line",
                        DraftShape::Polygon => "area",
                    },
                }),
                "kind": active.map(|active| format!("{:?}", active.kind())),
            })
        }
    }
}

fn input(world: &mut World) -> Value {
    let focused = world
        .get_resource::<InputFocus>()
        .and_then(|focus| focus.get())
        .is_some_and(|entity| world.get::<EditableText>(entity).is_some());

    json!({
        "cell": world.get_resource::<MapPointer>().and_then(|pointer| pointer.cell)
            .map(|cell| json!({ "x": cell.x, "y": cell.y })),
        "forced": world.get_resource::<PointerOverride>().and_then(|forced| forced.0)
            .map(|cell| json!({ "x": cell.x, "y": cell.y })),
        "over_ui": world.get_resource::<PointerOverUi>().map(|over| over.0),
        "text_field_has_focus": focused,
        "question_up": world.get_resource::<Asking>().map(|asking| asking.question.is_some()),
        "has_document": world.get_resource::<WorldDoc>().is_some(),
        "has_terrain": world.get_resource::<MapTerrain>().is_some(),
    })
}

fn cell(rest: &[&str], from: usize) -> Result<CellPoint, String> {
    let x = rest.get(from).ok_or("expected an x in cells")?;
    let y = rest.get(from + 1).ok_or("expected a y in cells")?;
    Ok(CellPoint::new(
        x.parse().map_err(|_| format!("{x} is not a number"))?,
        y.parse().map_err(|_| format!("{y} is not a number"))?,
    ))
}

fn kind(word: &str) -> Result<FeatureKind, String> {
    match word {
        "settlement" => Ok(FeatureKind::Settlement),
        "dungeon" => Ok(FeatureKind::DungeonEntry),
        "road" => Ok(FeatureKind::Road),
        "river" => Ok(FeatureKind::River),
        "trail" => Ok(FeatureKind::Trail),
        "landcover" => Ok(FeatureKind::Landcover),
        "territory" => Ok(FeatureKind::Territory),
        "poi" => Ok(FeatureKind::Poi),
        other => Err(format!("there is no {other} kind")),
    }
}

fn rank(word: &str) -> Result<Rank, String> {
    match word {
        "hamlet" => Ok(Rank::Hamlet),
        "town" => Ok(Rank::Town),
        "city" => Ok(Rank::City),
        other => Err(format!("there is no {other} rank")),
    }
}

fn chord(word: &str) -> Result<Chord, String> {
    let mut keys = Vec::new();
    for part in word.split('+') {
        keys.push(match part {
            "ctrl" => KeyCode::ControlLeft,
            "shift" => KeyCode::ShiftLeft,
            "alt" => KeyCode::AltLeft,
            "enter" | "return" => KeyCode::Enter,
            "escape" | "esc" => KeyCode::Escape,
            "backspace" => KeyCode::Backspace,
            "delete" | "del" => KeyCode::Delete,
            "z" => KeyCode::KeyZ,
            "s" => KeyCode::KeyS,
            other => return Err(format!("there is no key called {other}")),
        });
    }
    Ok(Chord { keys })
}

#[cfg(test)]
mod tests {
    use super::*;

    // The parse table is the socket's whole surface, and a scenario file fails far from
    // the line that caused it — so every verb has to round-trip to the word it came from.
    #[test]
    fn every_verb_parses_back_to_its_own_name() {
        for (line, verb) in [
            ("ping", "ping"),
            ("step 3", "step"),
            ("tool area", "tool"),
            ("kind settlement", "kind"),
            ("at 10 20", "at"),
            ("click", "click"),
            ("click 4 5", "click"),
            ("drag 0 0 10 10", "drag"),
            ("key escape", "key"),
            ("undo", "key"),
            ("redo", "key"),
            ("save", "key"),
            ("observe world", "observe"),
            ("capture /tmp/a.png", "capture"),
            ("fixed-delta 0.016", "fixed-delta"),
            ("quit", "quit"),
        ] {
            let command = Command::parse(line).unwrap_or_else(|error| panic!("{line}: {error}"));
            assert_eq!(command.verb(), verb, "{line}");
        }
    }

    // A mistyped verb or argument must name what was wrong rather than being applied as
    // something else — a scenario that silently ran a different command is worse than one
    // that stopped.
    #[test]
    fn a_line_that_is_not_a_command_is_refused_by_name() {
        for line in ["", "fly", "tool wobble", "kind wobble", "at 1", "at x y", "key wobble"] {
            assert!(Command::parse(line).is_err(), "{line:?} should be refused");
        }
    }

    // Undo, redo and save are the key chords the GM presses, not privileged verbs — the
    // socket must not be able to reach the document a way the keyboard cannot.
    #[test]
    fn undo_redo_and_save_are_the_keyboard_chords() {
        for (line, expected) in [
            ("undo", vec![KeyCode::ControlLeft, KeyCode::KeyZ]),
            ("redo", vec![KeyCode::ControlLeft, KeyCode::ShiftLeft, KeyCode::KeyZ]),
            ("save", vec![KeyCode::ControlLeft, KeyCode::KeyS]),
        ] {
            let Ok(Command::Key { chord, .. }) = Command::parse(line) else {
                panic!("{line} is a key chord");
            };
            assert_eq!(chord.keys, expected, "{line}");
        }
    }
}
