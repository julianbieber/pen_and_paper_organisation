//! The handful of things only the real `zk` can answer: that the argument vectors this
//! crate builds are ones it accepts, that a note comes out tagged the way the reference
//! query looks for it, and that the query then finds it.
//!
//! Every test here skips itself when `zk` is not installed, so `cargo test -p campaign`
//! still passes with nothing on `PATH`. Set `PNP_REQUIRE_ZK=1` to turn the skip into a
//! failure — without it a machine with no `zk` reports success having checked none of
//! this, which is exactly how a broken invocation would reach a release.

use campaign::feature::FeatureId;
use campaign::notebook::{self, NoteKind, Notebook, SystemRunner, zk_is_installed};

fn zk_or_skip(test: &str) -> bool {
    if zk_is_installed() {
        return true;
    }
    assert!(
        std::env::var_os("PNP_REQUIRE_ZK").is_none(),
        "PNP_REQUIRE_ZK is set but zk is not installed, so {test} could not run"
    );
    eprintln!("skipping {test}: zk is not on PATH");
    false
}

fn campaign_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path().join("my-campaign");
    std::fs::create_dir_all(root.join("notes")).expect("notes dir");
    (tmp, root)
}

// The acceptance criterion for a place: the note zk makes carries the feature in its
// frontmatter and the tag in its body. Nothing in the running tool reads a note, so this
// is the only place that check can be made at all.
#[test]
fn a_place_note_names_its_feature_and_carries_its_tag() {
    if !zk_or_skip("a_place_note_names_its_feature_and_carries_its_tag") {
        return;
    }
    let (_tmp, root) = campaign_root();
    let book = Notebook::of(&root);

    let note = book
        .create(&SystemRunner, NoteKind::Place, "Riverford", Some(FeatureId(7)))
        .expect("create a place note");

    let text = std::fs::read_to_string(book.root().join(&note.path)).expect("read the note");
    assert!(text.contains("feature: 7"), "frontmatter names the feature:\n{text}");
    assert!(
        text.contains(&format!("#place/{}", note.slug)),
        "body carries the tag:\n{text}"
    );
    assert!(text.contains("title: Riverford"), "{text}");
}

// A person and a faction come from the notes panel with no feature, and have to be
// tagged the same way or issue #7 can find a place and nothing else.
#[test]
fn a_person_and_a_faction_are_tagged_by_their_own_kind() {
    if !zk_or_skip("a_person_and_a_faction_are_tagged_by_their_own_kind") {
        return;
    }
    let (_tmp, root) = campaign_root();
    let book = Notebook::of(&root);

    for kind in [NoteKind::Person, NoteKind::Faction, NoteKind::Session] {
        let note = book
            .create(&SystemRunner, kind, "Sir Bedivere", None)
            .unwrap_or_else(|error| panic!("create a {kind:?} note: {error}"));
        let text = std::fs::read_to_string(book.root().join(&note.path)).expect("read");
        assert!(
            text.contains(&format!("#{}/{}", kind.tag_prefix(), note.slug)),
            "{kind:?}:\n{text}"
        );
    }
}

// Two places titled the same must be two findable notes, or a campaign with two
// Riverfords silently has one.
#[test]
fn two_places_titled_the_same_are_two_notes_with_two_slugs() {
    if !zk_or_skip("two_places_titled_the_same_are_two_notes_with_two_slugs") {
        return;
    }
    let (_tmp, root) = campaign_root();
    let book = Notebook::of(&root);

    let first = book
        .create(&SystemRunner, NoteKind::Place, "Riverford", Some(FeatureId(1)))
        .expect("first");
    let second = book
        .create(&SystemRunner, NoteKind::Place, "Riverford", Some(FeatureId(2)))
        .expect("second");

    assert_ne!(first.path, second.path);
    assert_ne!(first.slug, second.slug);
    assert!(book.root().join(&first.path).exists());
    assert!(book.root().join(&second.path).exists());
}

// A title beginning with a dash is the case zk itself rejects when the value is passed as
// a following word; this proves the joined form gets it through.
#[test]
fn a_title_beginning_with_a_dash_still_makes_a_note() {
    if !zk_or_skip("a_title_beginning_with_a_dash_still_makes_a_note") {
        return;
    }
    let (_tmp, root) = campaign_root();
    let book = Notebook::of(&root);

    let note = book
        .create(&SystemRunner, NoteKind::Person, "-Kai the Grey", None)
        .expect("a dash-leading title is a title, not an option");

    let text = std::fs::read_to_string(book.root().join(&note.path)).expect("read");
    assert!(text.contains("-Kai the Grey"), "{text}");
}

// The notebook is made on the first note and not by creating the campaign, so this is
// what proves lazy initialisation actually initialises.
#[test]
fn the_first_note_makes_the_notebook() {
    if !zk_or_skip("the_first_note_makes_the_notebook") {
        return;
    }
    let (_tmp, root) = campaign_root();
    let book = Notebook::of(&root);
    assert!(!book.is_initialised(), "nothing has made it a notebook yet");

    book.create(&SystemRunner, NoteKind::Session, "Session 1", None)
        .expect("create");

    assert!(book.is_initialised());
    for kind in NoteKind::all() {
        assert!(
            book.root().join(".zk").join("templates").join(kind.template()).exists(),
            "{kind:?}"
        );
    }
}

// A notebook naming somewhere else in the environment would put campaign notes in the
// GM's own notes; the working directory has to win, and the variable is removed anyway.
#[test]
fn a_notebook_named_in_the_environment_does_not_steal_the_note() {
    if !zk_or_skip("a_notebook_named_in_the_environment_does_not_steal_the_note") {
        return;
    }
    let (_tmp, root) = campaign_root();
    let elsewhere = root.parent().expect("parent").join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("elsewhere");
    let book = Notebook::of(&root);

    // SAFETY: single-threaded test process, and the variable is read only by the child.
    unsafe { std::env::set_var("ZK_NOTEBOOK_DIR", &elsewhere) };
    let note = book.create(&SystemRunner, NoteKind::Place, "Riverford", None);
    unsafe { std::env::remove_var("ZK_NOTEBOOK_DIR") };

    let note = note.expect("create");
    assert!(book.root().join(&note.path).exists(), "the note is in the campaign");
    assert!(!elsewhere.join(".zk").exists(), "nothing was made elsewhere");
}

// The whole of issue #7 end to end, and the one thing no other test can check: that the
// tag the template writes is a tag `zk list` actually finds. Both halves live in this
// crate, but only the real program connects them.
#[test]
fn a_places_tag_finds_the_note_that_references_it() {
    if !zk_or_skip("a_places_tag_finds_the_note_that_references_it") {
        return;
    }
    let (_tmp, root) = campaign_root();
    let book = Notebook::of(&root);

    let place = book
        .create(&SystemRunner, NoteKind::Place, "Riverford", Some(FeatureId(7)))
        .expect("create a place note");
    let session = book
        .create(&SystemRunner, NoteKind::Session, "Session 3", None)
        .expect("create a session note");

    let path = book.root().join(&session.path);
    let text = std::fs::read_to_string(&path).expect("read the session note");
    std::fs::write(
        &path,
        format!("{text}\nThe party arrived at #place/{} in the rain.\n", place.slug),
    )
    .expect("write the session note");

    let tag = notebook::tag_of(NoteKind::Place, &place.path);
    let found = book
        .references(&SystemRunner, &tag, &place.slug)
        .expect("the references");

    let paths: Vec<&str> = found.iter().map(|note| note.path.as_str()).collect();
    assert_eq!(paths, vec![session.path.as_str()], "found {found:#?}");
    assert_ne!(found[0].excerpt, found[0].lead, "a row shows more than the heading: {found:#?}");
    assert!(
        found[0].excerpt.contains("arrived"),
        "a row's excerpt carries the sentence that references it: {found:#?}"
    );
}

// A tag no note carries: zk prints nothing at all rather than an empty array, and exits
// successfully. The parse decides on emptiness before the exit status, so this pins the
// behaviour the decision rests on against a zk upgrade.
#[test]
fn a_tag_with_no_notes_is_empty_rather_than_a_failure() {
    if !zk_or_skip("a_tag_with_no_notes_is_empty_rather_than_a_failure") {
        return;
    }
    let (_tmp, root) = campaign_root();
    let book = Notebook::of(&root);
    book.create(&SystemRunner, NoteKind::Place, "Riverford", Some(FeatureId(7)))
        .expect("create a place note");

    let found = book
        .references(&SystemRunner, "place/nothing-carries-this", "nothing")
        .expect("an empty answer, not an error");

    assert!(found.is_empty(), "found {found:#?}");
}

// zk's tag filter reads a leading dash as negation, so a title starting with one could
// invert the query into "every note not tagged this" if the slug kept the dash.
#[test]
fn a_dash_leading_titles_tag_still_finds_its_own_reference() {
    if !zk_or_skip("a_dash_leading_titles_tag_still_finds_its_own_reference") {
        return;
    }
    let (_tmp, root) = campaign_root();
    let book = Notebook::of(&root);

    let place = book
        .create(&SystemRunner, NoteKind::Place, "-Kai's Rest", Some(FeatureId(9)))
        .expect("create a place note");
    let session = book
        .create(&SystemRunner, NoteKind::Session, "Session 4", None)
        .expect("create a session note");

    let path = book.root().join(&session.path);
    let text = std::fs::read_to_string(&path).expect("read the session note");
    std::fs::write(&path, format!("{text}\nCamped at #place/{}.\n", place.slug))
        .expect("write the session note");

    let tag = notebook::tag_of(NoteKind::Place, &place.path);
    let found = book
        .references(&SystemRunner, &tag, &place.slug)
        .expect("the references");

    let paths: Vec<&str> = found.iter().map(|note| note.path.as_str()).collect();
    assert_eq!(paths, vec![session.path.as_str()], "tag {tag}, found {found:#?}");
}

// Issue #19's acceptance case against the real program: the row shows the sentence the
// tag is in, not the note's heading, and not an earlier paragraph that only shares a
// word with the place.
#[test]
fn a_reference_shows_the_sentence_carrying_the_tag_not_the_heading() {
    if !zk_or_skip("a_reference_shows_the_sentence_carrying_the_tag_not_the_heading") {
        return;
    }
    let (_tmp, root) = campaign_root();
    let book = Notebook::of(&root);

    let place = book
        .create(&SystemRunner, NoteKind::Place, "Riverford", Some(FeatureId(7)))
        .expect("create a place note");
    let session = book
        .create(&SystemRunner, NoteKind::Session, "Session 3", None)
        .expect("create a session note");

    let path = book.root().join(&session.path);
    let text = std::fs::read_to_string(&path).expect("read the session note");
    std::fs::write(
        &path,
        format!(
            "{text}\n\
             They set out for Riverford at dawn, arguing about the map the whole way.\n\n\
             The road was long, and nobody wanted to talk about the bridge that had washed out.\n\n\
             Late on the fourth day they reached #place/{} and took rooms at the Drowned Rat.\n",
            place.slug
        ),
    )
    .expect("write the session note");

    let tag = notebook::tag_of(NoteKind::Place, &place.path);
    let found = book
        .references(&SystemRunner, &tag, &place.slug)
        .expect("the references");

    assert_eq!(found.len(), 1, "found {found:#?}");
    let excerpt = &found[0].excerpt;
    assert!(excerpt.contains(&tag), "carries the tag: {excerpt}");
    assert!(excerpt.contains("Drowned Rat"), "carries the tag's sentence: {excerpt}");
    assert!(!excerpt.contains("# Session 3"), "not the heading: {excerpt}");
    assert!(!excerpt.contains("washed out"), "not the earlier paragraph: {excerpt}");
}

// The place template itself writes a bare tag with no surrounding prose; that note must
// still show something rather than an empty row.
#[test]
fn a_bare_tag_still_shows_something() {
    if !zk_or_skip("a_bare_tag_still_shows_something") {
        return;
    }
    let (_tmp, root) = campaign_root();
    let book = Notebook::of(&root);

    let place = book
        .create(&SystemRunner, NoteKind::Place, "Riverford", Some(FeatureId(7)))
        .expect("create a place note");
    let person = book
        .create(&SystemRunner, NoteKind::Person, "Sir Bedivere", None)
        .expect("create a person note");

    let path = book.root().join(&person.path);
    let text = std::fs::read_to_string(&path).expect("read the person note");
    std::fs::write(&path, format!("{text}\n#place/{}\n", place.slug)).expect("write");

    let tag = notebook::tag_of(NoteKind::Place, &place.path);
    let found = book
        .references(&SystemRunner, &tag, &place.slug)
        .expect("the references");

    assert_eq!(found.len(), 1, "found {found:#?}");
    assert!(!found[0].excerpt.is_empty(), "a bare tag still shows something");
    assert!(found[0].excerpt.contains(&tag), "{}", found[0].excerpt);
}

// A tag given only in frontmatter is found by the tag query alone; the match query
// cannot see it in the text, so the row falls back to the note's lead rather than
// deciding which notes are listed.
#[test]
fn a_frontmatter_tag_is_still_found_and_shows_its_lead() {
    if !zk_or_skip("a_frontmatter_tag_is_still_found_and_shows_its_lead") {
        return;
    }
    let (_tmp, root) = campaign_root();
    let book = Notebook::of(&root);

    let place = book
        .create(&SystemRunner, NoteKind::Place, "Riverford", Some(FeatureId(7)))
        .expect("create a place note");
    let tag = notebook::tag_of(NoteKind::Place, &place.path);

    let note_path = book.root().join("written-by-hand-c3d4.md");
    std::fs::write(
        &note_path,
        format!("---\ntitle: Written By Hand\ntags: [{tag}]\n---\n\nNo mention of it here.\n"),
    )
    .expect("write the note directly");

    let found = book
        .references(&SystemRunner, &tag, &place.slug)
        .expect("the references");

    assert_eq!(found.len(), 1, "found {found:#?}");
    assert_eq!(found[0].excerpt, found[0].lead, "falls back to lead: {found:#?}");
    assert!(!found[0].excerpt.is_empty(), "still shows something");
}
