//! The selected feature's label, kind, rank, reveal scale, parent and note link.
//!
//! The label is the one property that cannot be landed as it is typed. A text field
//! reports every character, so an edit per report would spend a fifth of the undo stack on
//! renaming one settlement — the change is therefore held here and committed once, when
//! the field is left or Enter is pressed.
//!
//! The panel shows a note link and clears it. Creating one is issue #6's, and there is
//! deliberately no button here that would.

use bevy::feathers::controls::{
    FeathersButton, FeathersTextInput, FeathersTextInputContainer,
};
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, TextEdit, TextEditChange};
use bevy::ui_widgets::Activate;
use campaign::edit::Edit;
use campaign::feature::{FeatureId, FeatureKind, Rank};

use crate::StatusMessage;
use crate::features::doc::{self, WorldDoc};
use crate::features::select::Selection;
use crate::features::tool::{kind_label, rank_label};
use crate::map::pointer::MapPointer;

/// Which property a line of the panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Property {
    Kind,
    Rank,
    Reveal,
    Parent,
    Note,
}

/// The panel's root.
#[derive(Component, Default, Clone)]
pub struct PropertyPanelRoot;

/// A line of the panel showing one property.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct PropertyReadout {
    pub property: Property,
}

/// The field the selected feature's label is typed into.
#[derive(Component, Default, Clone)]
pub struct LabelField;

/// A button that sets the selected feature's kind.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct SetKindButton {
    pub kind: FeatureKind,
}

/// A button that sets the selected feature's rank.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct SetRankButton {
    pub rank: Rank,
}

/// The button that records the map's current scale as the selected feature's reveal
/// threshold.
#[derive(Component, Default, Clone)]
pub struct RevealHereButton;

/// A button that clears one of the selection's links.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ClearButton {
    pub property: Property,
}

/// The label being typed, and which feature it belongs to.
///
/// Held rather than applied so that a rename is one entry on the undo stack rather than
/// one per keystroke.
#[derive(Resource, Debug, Default)]
pub struct PendingLabel {
    pub feature: Option<FeatureId>,
    pub text: String,
}

/// Hangs the property panel over the map.
pub fn build_property_panel(mut commands: Commands) {
    commands.spawn_scene(panel());
}

/// Puts the selected feature's label, kind, rank, reveal scale, parent and note link into
/// the panel.
///
/// Runs only when the selection or the document changed, and writes only where the text
/// differs: a panel rewritten every frame re-lays-out the UI for the life of the process.
///
/// The label field is seeded only when the selection moves to another feature — seeding it
/// on every document change would overwrite what is being typed into it. A parent is named
/// by its label, falling back to its id when it has never been named.
pub fn show_properties(
    doc: Res<WorldDoc>,
    selection: Res<Selection>,
    mut pending: ResMut<PendingLabel>,
    mut readouts: Query<(&PropertyReadout, &mut Text)>,
    mut fields: Query<&mut EditableText, With<LabelField>>,
) {
    let world = doc.document.world();
    let chosen = selection.only();
    let feature = chosen.and_then(|id| world.feature(id));

    for (readout, mut text) in readouts.iter_mut() {
        let shown = match (feature, readout.property) {
            (None, Property::Kind) if selection.features.len() > 1 => {
                format!("{} features selected", selection.features.len())
            }
            (None, Property::Kind) => "nothing selected".to_owned(),
            (None, _) => String::new(),
            (Some(feature), Property::Kind) => kind_label(feature.kind).to_owned(),
            (Some(feature), Property::Rank) => match feature.rank {
                Some(rank) => rank_label(rank).to_owned(),
                None => "no rank".to_owned(),
            },
            (Some(feature), Property::Reveal) => match feature.max_cells_per_pixel {
                Some(scale) => format!("revealed at {scale:.3} cells/px or finer"),
                None => "revealed at every zoom".to_owned(),
            },
            (Some(feature), Property::Parent) => match feature.parent {
                Some(parent) => format!("inside {}", name_of(world, parent)),
                None => "no parent".to_owned(),
            },
            (Some(feature), Property::Note) => match feature.note.as_deref() {
                Some(note) => format!("note: {note}"),
                None => "no note".to_owned(),
            },
        };
        if text.0 != shown {
            text.0 = shown;
        }
    }

    if pending.feature == chosen {
        return;
    }
    pending.feature = chosen;
    pending.text = feature.map(|feature| feature.label.clone()).unwrap_or_default();
    for mut field in fields.iter_mut() {
        field.queue_edit(TextEdit::SelectAll);
        field.queue_edit(TextEdit::Backspace);
        if !pending.text.is_empty() {
            field.queue_edit(TextEdit::Insert(pending.text.as_str().into()));
        }
    }
}

/// Lands a typed label as one edit, once it is finished with.
///
/// "Finished with" is Enter, or focus moving off the field. There is no submit or blur
/// event to observe in this version, so the focus resource is the signal.
pub fn commit_label(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    fields: Query<Entity, With<LabelField>>,
    pending: Res<PendingLabel>,
    mut doc: ResMut<WorldDoc>,
    mut status: ResMut<StatusMessage>,
) {
    let focused = focus.get().is_some_and(|entity| fields.contains(entity));
    let entered = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter);
    if focused && !entered {
        return;
    }

    let Some(id) = pending.feature else {
        return;
    };
    let Some(feature) = doc.document.world().feature(id) else {
        return;
    };
    if feature.label == pending.text {
        return;
    }

    let label = pending.text.clone();
    doc::apply(&mut doc, &mut status, Edit::SetLabel { id, label });
}

fn name_of(world: &campaign::world::World, id: FeatureId) -> String {
    world
        .feature(id)
        .filter(|feature| !feature.label.is_empty())
        .map(|feature| feature.label.clone())
        .unwrap_or_else(|| id.to_string())
}

const RANKS: [Rank; 3] = Rank::all();

const KINDS: [FeatureKind; 8] = [
    FeatureKind::Settlement,
    FeatureKind::DungeonEntry,
    FeatureKind::Poi,
    FeatureKind::Road,
    FeatureKind::River,
    FeatureKind::Trail,
    FeatureKind::Landcover,
    FeatureKind::Territory,
];

fn panel() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            right: px(12),
            width: px(280),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: px(8),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        PropertyPanelRoot
        Children [
            (Text("Selection") ThemedText),
            (
                @FeathersTextInputContainer
                Node { width: percent(100) }
                Children [
                    (
                        @FeathersTextInput
                        LabelField
                        on(|change: On<TextEditChange>,
                            texts: Query<&EditableText>,
                            mut pending: ResMut<PendingLabel>| {
                            if let Ok(text) = texts.get(change.event_target()) {
                                pending.text = text.value().to_string();
                            }
                        })
                    )
                ]
            ),
            (Text("") ThemedText PropertyReadout { property: {Property::Kind} }),
            (Text("") ThemedText PropertyReadout { property: {Property::Rank} }),
            (Text("") ThemedText PropertyReadout { property: {Property::Reveal} }),
            (Text("") ThemedText PropertyReadout { property: {Property::Parent} }),
            (Text("") ThemedText PropertyReadout { property: {Property::Note} }),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(4),
                    row_gap: px(4),
                }
                Children [
                    kind_button(KINDS[0]), kind_button(KINDS[1]), kind_button(KINDS[2]),
                    kind_button(KINDS[3]), kind_button(KINDS[4]), kind_button(KINDS[5]),
                    kind_button(KINDS[6]), kind_button(KINDS[7])
                ]
            ),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(4),
                    row_gap: px(4),
                }
                Children [
                    rank_button(RANKS[0]), rank_button(RANKS[1]), rank_button(RANKS[2]),
                    clear_button("Clear rank", Property::Rank)
                ]
            ),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(4),
                    row_gap: px(4),
                }
                Children [
                    reveal_here_button(),
                    clear_button("Always reveal", Property::Reveal)
                ]
            ),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                }
                Children [
                    clear_button("Clear parent", Property::Parent),
                    clear_button("Clear note", Property::Note)
                ]
            )
        ]
    }
}

fn kind_button(kind: FeatureKind) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text({kind_label(kind).to_string()}) ThemedText },
        }
        SetKindButton { kind: {kind} }
        on(|activate: On<Activate>,
            buttons: Query<&SetKindButton>,
            selection: Res<Selection>,
            mut doc: ResMut<WorldDoc>,
            mut status: ResMut<StatusMessage>| {
            let (Ok(button), Some(id)) = (buttons.get(activate.event_target()), selection.only())
            else {
                return;
            };
            let already = doc
                .document
                .world()
                .feature(id)
                .is_some_and(|feature| feature.kind == button.kind);
            if already {
                return;
            }
            doc::apply(&mut doc, &mut status, Edit::SetKind { id, kind: button.kind });
        })
    }
}

fn rank_button(rank: Rank) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text({rank_label(rank).to_string()}) ThemedText },
        }
        SetRankButton { rank: {rank} }
        on(|activate: On<Activate>,
            buttons: Query<&SetRankButton>,
            selection: Res<Selection>,
            mut doc: ResMut<WorldDoc>,
            mut status: ResMut<StatusMessage>| {
            let (Ok(button), Some(id)) = (buttons.get(activate.event_target()), selection.only())
            else {
                return;
            };
            let already = doc
                .document
                .world()
                .feature(id)
                .is_some_and(|feature| feature.rank == Some(button.rank));
            if already {
                return;
            }
            doc::apply(&mut doc, &mut status, Edit::SetRank { id, rank: Some(button.rank) });
        })
    }
}

fn reveal_here_button() -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text("Reveal from here") ThemedText },
        }
        RevealHereButton
        on(|_activate: On<Activate>,
            selection: Res<Selection>,
            pointer: Res<MapPointer>,
            mut doc: ResMut<WorldDoc>,
            mut status: ResMut<StatusMessage>| {
            let Some(id) = selection.only() else {
                return;
            };
            doc::apply(
                &mut doc,
                &mut status,
                Edit::SetMaxCellsPerPixel {
                    id,
                    scale: Some(pointer.cells_per_pixel),
                },
            );
        })
    }
}

fn clear_button(caption: &'static str, property: Property) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text({caption.to_string()}) ThemedText },
        }
        ClearButton { property: {property} }
        on(|activate: On<Activate>,
            buttons: Query<&ClearButton>,
            selection: Res<Selection>,
            mut doc: ResMut<WorldDoc>,
            mut status: ResMut<StatusMessage>| {
            let (Ok(button), Some(id)) = (buttons.get(activate.event_target()), selection.only())
            else {
                return;
            };
            let edit = match button.property {
                Property::Parent => Edit::SetParent { id, parent: None },
                Property::Note => Edit::SetNote { id, note: None },
                Property::Rank => Edit::SetRank { id, rank: None },
                Property::Reveal => Edit::SetMaxCellsPerPixel { id, scale: None },
                Property::Kind => return,
            };
            doc::apply(&mut doc, &mut status, edit);
        })
    }
}

impl Default for PropertyReadout {
    fn default() -> Self {
        Self {
            property: Property::Kind,
        }
    }
}

impl Default for SetKindButton {
    fn default() -> Self {
        Self {
            kind: FeatureKind::Poi,
        }
    }
}

impl Default for SetRankButton {
    fn default() -> Self {
        Self { rank: Rank::Hamlet }
    }
}

impl Default for ClearButton {
    fn default() -> Self {
        Self {
            property: Property::Note,
        }
    }
}
