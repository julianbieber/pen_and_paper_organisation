//! Closing the open campaign and bringing the dialog back, and the window title that
//! says which campaign — if any — is open.
//!
//! [`close_campaign`] is the one teardown list every resource and root a campaign brings
//! is named on. A campaign-owned piece of state left off it leaks into the next campaign
//! opened in the same process — a duplicated panel, a stale selection, a note landing on
//! the wrong document — so anything a campaign brings must be dropped here, and nothing
//! elsewhere in the editor tears any of it down on its own.

use bevy::feathers::controls::FeathersButton;
use bevy::feathers::theme::ThemedText;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use bevy::window::PrimaryWindow;

use crate::combat::CombatMaps;
use crate::dialog::RecentList;
use crate::document::{WorldDoc, WorldState};
use crate::features::combat::{CombatFields, CombatIntent};
use crate::features::dungeon::DungeonIntent;
use crate::features::draw::Drafting;
use crate::features::image::{CalibrationDistance, ImageFields, ImportJob, ImportRequest, Placing};
use crate::features::panel::PendingLabel;
use crate::features::paint::Stroking;
use crate::features::prompt::{Asking, Question};
use crate::features::ruler::{Ruler, TravelSpeed};
use crate::features::select::{Dragging, Selection};
use crate::features::token::{TokenFields, TokenGesture, TokenIntent};
use crate::features::tool::ActiveTool;
use crate::map::backdrop::{Backdrop, RetiredBackdrop};
use crate::map::chunks::{ChunkCoord, MapChunks, PaintedCells};
use crate::map::image::{BackdropImage, ImageAsset};
use crate::map::load::{CombatTileset, DungeonTileset, MapAssets, MapState, MapTerrain};
use crate::map::scale::ScaleFields;
use crate::notes::references::References;
use crate::notes::watch::NotesWatch;
use crate::notes::{NoteJob, NoteTitle};
use crate::sync::{SyncFields, SyncJob};
use crate::{CampaignChrome, EditorSet, OpenCampaign, StatusMessage};

/// What the GM has asked of the open campaign, written by the Close button and the
/// control socket and consumed by [`ask_to_close`] and [`close_campaign`].
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CampaignClose {
    #[default]
    Nothing,
    /// Asked to close. Answered by [`ask_to_close`] on the next `Update`.
    Asked,
    /// Nothing stands in the way; [`close_campaign`] tears the campaign down next.
    Confirmed,
}

/// The close bar, the window title, and the campaign's own teardown.
pub struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CampaignClose>().add_systems(
            Update,
            (
                build_close_bar.run_if(resource_added::<OpenCampaign>),
                ask_to_close.run_if(closing_was_asked),
                close_campaign.run_if(closing_was_confirmed),
                sync_title,
            )
                .after(EditorSet::Authoring),
        );
    }
}

fn closing_was_asked(closing: Res<CampaignClose>) -> bool {
    *closing == CampaignClose::Asked
}

fn closing_was_confirmed(closing: Res<CampaignClose>) -> bool {
    *closing == CampaignClose::Confirmed
}

fn build_close_bar(mut commands: Commands) {
    commands.spawn_scene(close_bar());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloseDecision {
    NoCampaign,
    AnotherQuestionIsUp,
    AskedToSave,
    Confirmed,
}

fn decide_close(
    campaign_open: bool,
    doc: Option<&WorldDoc>,
    combat_unsaved: bool,
    asking: &mut Asking,
    closing: &mut CampaignClose,
) -> CloseDecision {
    *closing = CampaignClose::Nothing;

    if !campaign_open {
        return CloseDecision::NoCampaign;
    }
    if asking.question.is_some() {
        return CloseDecision::AnotherQuestionIsUp;
    }
    if doc.is_some_and(WorldDoc::anything_unsaved) || combat_unsaved {
        asking.raise(Question::UnsavedOnCampaignClose, None);
        return CloseDecision::AskedToSave;
    }
    *closing = CampaignClose::Confirmed;
    CloseDecision::Confirmed
}

fn ask_to_close(
    mut closing: ResMut<CampaignClose>,
    open: Option<Res<OpenCampaign>>,
    doc: Option<Res<WorldDoc>>,
    combat: Option<Res<CombatMaps>>,
    mut asking: ResMut<Asking>,
    mut status: ResMut<StatusMessage>,
) {
    let combat_unsaved = combat.is_some_and(|combat| combat.anything_unsaved());
    let decision = decide_close(open.is_some(), doc.as_deref(), combat_unsaved, &mut asking, &mut closing);
    if decision == CloseDecision::AnotherQuestionIsUp {
        status.say("answer the question on screen first");
    }
}

fn close_campaign(world: &mut World) {
    let name = world
        .get_resource::<OpenCampaign>()
        .map(|open| open.0.manifest().name.clone())
        .unwrap_or_default();
    let retiring = world.get_resource::<Backdrop>().map(|backdrop| backdrop.generation);
    if let Some(generation) = retiring {
        world.insert_resource(RetiredBackdrop(generation));
    }

    let chrome: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<CampaignChrome>, With<ChunkCoord>, With<BackdropImage>)>>()
        .iter(world)
        .collect();
    for entity in chrome {
        world.entity_mut(entity).despawn();
    }

    world.remove_resource::<OpenCampaign>();
    world.remove_resource::<WorldDoc>();
    world.remove_resource::<WorldState>();
    world.remove_resource::<MapState>();
    world.remove_resource::<MapAssets>();
    world.remove_resource::<MapTerrain>();
    world.remove_resource::<DungeonTileset>();
    world.remove_resource::<CombatTileset>();
    world.remove_resource::<Backdrop>();
    world.remove_resource::<NotesWatch>();

    world.insert_resource(MapChunks::default());
    world.insert_resource(PaintedCells::default());
    world.insert_resource(ImageAsset::default());
    world.insert_resource(ScaleFields::default());
    world.insert_resource(Selection::default());
    world.insert_resource(Dragging::default());
    world.insert_resource(Drafting::default());
    world.insert_resource(Stroking::default());
    world.insert_resource(Placing::default());
    world.insert_resource(ImportRequest::default());
    world.insert_resource(ImportJob::default());
    world.insert_resource(CalibrationDistance::default());
    world.insert_resource(ImageFields::default());
    world.insert_resource(DungeonIntent::default());
    world.insert_resource(CombatMaps::default());
    world.insert_resource(CombatIntent::default());
    world.insert_resource(CombatFields::default());
    world.insert_resource(TokenGesture::default());
    world.insert_resource(TokenFields::default());
    world.insert_resource(TokenIntent::default());
    world.insert_resource(PendingLabel::default());
    world.insert_resource(Asking::default());
    world.insert_resource(Ruler::default());
    world.insert_resource(TravelSpeed::default());
    world.insert_resource(NoteJob::default());
    world.insert_resource(NoteTitle::default());
    world.insert_resource(References::default());
    world.insert_resource(RecentList::default());
    world.insert_resource(CampaignClose::default());
    world.insert_resource(SyncJob::default());
    world.insert_resource(SyncFields::default());

    if let Some(mut active) = world.get_resource_mut::<ActiveTool>() {
        active.leave_combat(false);
    }
    if let Some(mut status) = world.get_resource_mut::<StatusMessage>() {
        status.say(format!("closed {name}"));
    }
}

fn sync_title(
    open: Option<Res<OpenCampaign>>,
    doc: Option<Res<WorldDoc>>,
    combat: Option<Res<CombatMaps>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let dirty = doc.as_deref().is_some_and(WorldDoc::anything_unsaved)
        || combat.is_some_and(|combat| combat.anything_unsaved());
    let title = campaign::title::window_title(
        open.as_deref().map(|open| (open.0.manifest().name.as_str(), dirty)),
    );
    for mut window in windows.iter_mut() {
        if window.title != title {
            window.title = title.clone();
        }
    }
}

fn close_bar() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: percent(50),
        }
        UiTransform { translation: {Val2::new(Val::Percent(-50.0), Val::ZERO)} }
        CampaignChrome
        Children [ close_button() ]
    }
}

fn close_button() -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text("Close campaign") ThemedText },
        }
        on(|_: On<Activate>, mut closing: ResMut<CampaignClose>, mut focus: ResMut<InputFocus>| {
            *closing = CampaignClose::Asked;
            focus.clear();
        })
    }
}

#[cfg(test)]
mod tests {
    use campaign::feature::FeatureId;
    use campaign::{Document, World as CampaignWorld};

    use super::*;
    use crate::document::WorldOutcome;
    use crate::map::backdrop::BackdropSource;
    use crate::map::view::MapView;

    fn dirty_doc() -> WorldDoc {
        let mut document = Document::new(CampaignWorld::default());
        document.fresh_id();
        WorldDoc::world_map(document, std::path::PathBuf::from("world.ron"))
    }

    // A dirty document is asked about rather than closed outright, so a GM cannot lose
    // unsaved work to one press of Close.
    #[test]
    fn a_dirty_document_raises_the_question_instead_of_closing() {
        let doc = dirty_doc();
        let mut asking = Asking::default();
        let mut closing = CampaignClose::Asked;

        let decision = decide_close(true, Some(&doc), false, &mut asking, &mut closing);

        assert_eq!(decision, CloseDecision::AskedToSave);
        assert_eq!(asking.question, Some(Question::UnsavedOnCampaignClose));
        assert_eq!(closing, CampaignClose::Nothing);
    }

    // Nothing unsaved closes outright: this is the "no edits" half of the acceptance,
    // which says re-opening the same campaign asks nothing.
    #[test]
    fn a_clean_document_closes_without_asking() {
        let doc = WorldDoc::world_map(
            Document::new(CampaignWorld::default()),
            std::path::PathBuf::from("world.ron"),
        );
        let mut asking = Asking::default();
        let mut closing = CampaignClose::Asked;

        let decision = decide_close(true, Some(&doc), false, &mut asking, &mut closing);

        assert_eq!(decision, CloseDecision::Confirmed);
        assert_eq!(asking.question, None);
        assert_eq!(closing, CampaignClose::Confirmed);
    }

    // A painted combat map is discarded by a close just as a world edit is, so it is asked
    // about even when every world document is clean.
    #[test]
    fn a_dirty_combat_map_raises_the_question_on_a_clean_world() {
        let doc = WorldDoc::world_map(
            Document::new(CampaignWorld::default()),
            std::path::PathBuf::from("world.ron"),
        );
        let mut asking = Asking::default();
        let mut closing = CampaignClose::Asked;

        let decision = decide_close(true, Some(&doc), true, &mut asking, &mut closing);

        assert_eq!(decision, CloseDecision::AskedToSave);
        assert_eq!(asking.question, Some(Question::UnsavedOnCampaignClose));
        assert_eq!(closing, CampaignClose::Nothing);
    }

    // Discarding the campaign-close question confirms the close; cancelling it leaves the
    // campaign open, exactly as the window's own unsaved prompt does for its question.
    #[test]
    fn discard_confirms_and_cancel_leaves_it_open() {
        use crate::features::prompt::Answer;

        fn answered(answer: Answer) -> CampaignClose {
            let mut app = App::new();
            app.add_message::<AppExit>();
            let mut asking = Asking::default();
            asking.raise(Question::UnsavedOnCampaignClose, None);
            app.insert_resource(asking)
                .insert_resource(dirty_doc())
                .insert_resource(Selection::default())
                .insert_resource(StatusMessage::default())
                .insert_resource(CampaignClose::Nothing)
                .add_systems(
                    Update,
                    move |mut asking: ResMut<Asking>,
                          mut doc: ResMut<WorldDoc>,
                          mut selection: ResMut<Selection>,
                          mut status: ResMut<StatusMessage>,
                          mut closing: ResMut<CampaignClose>,
                          mut exit: MessageWriter<AppExit>| {
                        crate::features::prompt::answer(
                            answer,
                            &mut asking,
                            &mut doc,
                            &mut selection,
                            &mut status,
                            &mut closing,
                            &mut exit,
                            None,
                        );
                    },
                );
            app.update();
            *app.world().resource::<CampaignClose>()
        }

        assert_eq!(answered(Answer::Discard), CampaignClose::Confirmed);
        assert_eq!(answered(Answer::Cancel), CampaignClose::Nothing);
    }

    // The teardown's own inventory: everything a campaign brings is gone, the backdrop's
    // generation carries forward, and nothing outside the list is disturbed.
    #[test]
    fn close_campaign_drops_everything_the_campaign_brought() {
        let mut app = App::new();
        app.insert_resource(WorldDoc::world_map(
            Document::new(CampaignWorld::default()),
            std::path::PathBuf::from("world.ron"),
        ));
        app.insert_resource(WorldState {
            outcome: WorldOutcome::Ready,
            message: String::new(),
        });
        app.insert_resource(Backdrop {
            view: MapView::new(4, 4, 32.0),
            source: BackdropSource::Terrain,
            generation: 3,
            restore: None,
        });
        let mut selection = Selection::default();
        selection.features.push(FeatureId(1));
        app.insert_resource(selection);
        app.insert_resource(CampaignClose::Confirmed);
        app.insert_resource(SyncJob::assume_read_origin());
        let mut combat = CombatMaps::default();
        combat.open(campaign::CombatMap::new("Ford", 4, 4).expect("a 4x4 map is valid"), "ford.ron".to_owned(), None);
        let (tokens, extent) = combat.tokens_on_screen_mut().expect("Ford is on screen");
        tokens.place("orc", 1, (0, 0), extent).expect("a token fits on Ford");
        app.insert_resource(combat);
        app.insert_resource(TokenGesture {
            selected: Some("orc1".to_owned()),
            drag: None,
        });
        let chrome = app.world_mut().spawn(CampaignChrome).id();
        let chunk = app.world_mut().spawn(ChunkCoord { x: 0, y: 0 }).id();

        close_campaign(app.world_mut());

        assert!(!app.world().contains_resource::<WorldDoc>());
        assert!(!app.world().contains_resource::<WorldState>());
        assert!(!app.world().contains_resource::<Backdrop>());
        assert_eq!(app.world().resource::<RetiredBackdrop>().0, 3);
        assert!(app.world().get_entity(chrome).is_err());
        assert!(app.world().get_entity(chunk).is_err());
        assert!(app.world().resource::<Selection>().features.is_empty());
        assert_eq!(*app.world().resource::<CampaignClose>(), CampaignClose::Nothing);
        assert!(!app.world().resource::<SyncJob>().read_origin, "the next campaign must ask again");
        assert_eq!(app.world().resource::<CombatMaps>().names().count(), 0, "a combat map is never inherited");
        assert_eq!(app.world().resource::<TokenGesture>().selected, None, "a token selection is never inherited");
    }
}
