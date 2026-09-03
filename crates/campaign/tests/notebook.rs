//! What the notebook decides before `zk` is ever involved: which template a kind names,
//! which flags carry which values, what becomes of the path `zk` prints, and which
//! templates a notebook is left holding.
//!
//! Every test here runs against a recorded runner rather than the real program, so the
//! crate keeps its rule that `cargo test -p campaign` passes with nothing on `PATH`. The
//! few tests that want the real `zk` live in `notebook_zk.rs` and skip themselves.
//!
//! The recorded runner imitates `zk init` as far as the config step can tell — it leaves a
//! `config.toml` behind, because that file already existing is the whole reason writing
//! ours has to be deliberate rather than conditional.

use std::cell::RefCell;
use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};

use campaign::feature::FeatureId;
use campaign::notebook::{
    self, NoteError, NoteKind, Notebook, Runner, edit_args, init_args, new_args, printed_path,
    slug_of,
};

#[derive(Default)]
struct Recorder {
    calls: RefCell<Vec<(PathBuf, Vec<String>)>>,
    stdout: String,
    fail_with: Option<String>,
    init_makes: Option<PathBuf>,
}

impl Recorder {
    fn printing(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            ..Self::default()
        }
    }

    fn args(&self) -> Vec<Vec<String>> {
        self.calls.borrow().iter().map(|(_, a)| a.clone()).collect()
    }

    fn dirs(&self) -> Vec<PathBuf> {
        self.calls.borrow().iter().map(|(d, _)| d.clone()).collect()
    }
}

impl Runner for Recorder {
    fn run(&self, dir: &Path, args: &[OsString]) -> Result<Output, NoteError> {
        let text: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        if text.iter().any(|a| a == "init")
            && let Some(root) = &self.init_makes
        {
            std::fs::create_dir_all(root.join(".zk")).expect("fake init");
            std::fs::write(root.join(".zk").join("config.toml"), "# zk's own default\n")
                .expect("fake init config");
        }
        self.calls.borrow_mut().push((dir.to_owned(), text));
        match &self.fail_with {
            Some(message) => Ok(Output {
                status: ExitStatus::from_raw(256),
                stdout: Vec::new(),
                stderr: message.clone().into_bytes(),
            }),
            None => Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout: self.stdout.clone().into_bytes(),
                stderr: Vec::new(),
            }),
        }
    }

    fn start(&self, dir: &Path, args: &[OsString]) -> Result<(), NoteError> {
        self.run(dir, args).map(|_| ())
    }
}

struct NeverRuns;

impl Runner for NeverRuns {
    fn run(&self, _dir: &Path, args: &[OsString]) -> Result<Output, NoteError> {
        panic!("nothing should have been run, but {args:?} was");
    }
    fn start(&self, _dir: &Path, args: &[OsString]) -> Result<(), NoteError> {
        panic!("nothing should have been started, but {args:?} was");
    }
}

fn notebook() -> (tempfile::TempDir, Notebook) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path().join("my-campaign");
    let book = Notebook::of(&root);
    std::fs::create_dir_all(book.root()).expect("notes dir");
    (tmp, book)
}

// `zk` resolves `--template` as a file name and refuses one without its extension, so a
// kind's template name carries the extension rather than leaving a caller to append it.
#[test]
fn every_template_is_named_with_its_extension() {
    for kind in NoteKind::all() {
        assert!(
            kind.template().ends_with(".md"),
            "{kind:?} names {}",
            kind.template()
        );
    }
}

// The templates and the tag prefixes are two halves of one fact, so a template that does
// not write its own kind's tag is the drift this pins.
#[test]
fn every_template_writes_its_own_tag_from_the_filename_stem() {
    for kind in NoteKind::all() {
        let wanted = format!("#{}/{{{{filename-stem}}}}", kind.tag_prefix());
        assert!(
            kind.body().contains(&wanted),
            "{kind:?} should write {wanted}, body was:\n{}",
            kind.body()
        );
    }
}

// The slug in a tag has to be the note's own filename stem, or issue #7's `zk list --tag`
// finds nothing. Deriving it any other way would be a second answer that can disagree.
#[test]
fn no_template_derives_its_tag_from_the_title() {
    for kind in NoteKind::all() {
        let tag_line = kind
            .body()
            .lines()
            .find(|line| line.starts_with('#') && line.contains('/'))
            .unwrap_or_else(|| panic!("{kind:?} writes no tag"));
        assert!(
            !tag_line.contains("slug title"),
            "{kind:?} tags from the title rather than the filename: {tag_line}"
        );
    }
}

// Only the place template is given a feature, so only it may name one — a person note
// carrying an empty `feature:` field would read as a broken link rather than as no link.
#[test]
fn only_the_place_template_names_a_feature() {
    for kind in NoteKind::all() {
        assert_eq!(
            kind.body().contains("{{extra.feature}}"),
            kind == NoteKind::Place,
            "{kind:?}"
        );
    }
}

// The two written-out arrays are the count, so a kind added to one and not the other is
// caught here rather than by a panel that silently offers three buttons for four kinds.
#[test]
fn the_kinds_without_geometry_are_every_kind_but_place() {
    let without: Vec<NoteKind> = NoteKind::without_geometry().into();
    let expected: Vec<NoteKind> = NoteKind::all()
        .into_iter()
        .filter(|kind| *kind != NoteKind::of_a_feature())
        .collect();
    assert_eq!(without, expected);
    assert_eq!(NoteKind::all().len(), without.len() + 1);
}

// Labels are what the control socket parses and what the panel captions, so they have to
// round-trip and they have to be distinct.
#[test]
fn a_kind_round_trips_through_its_label() {
    for kind in NoteKind::all() {
        assert_eq!(NoteKind::from_label(kind.label()), Some(kind));
    }
    assert_eq!(NoteKind::from_label("wobble"), None);
}

// The bug this pins is the one `zk` itself reports: a title beginning with a dash passed
// as a following word is read as another option, so a settlement named `-Kai` would be
// unnameable. Joining every value to its flag is what makes it work.
#[test]
fn every_value_is_joined_to_its_flag() {
    let args = new_args(NoteKind::Place, "-Kai the Grey", Some(FeatureId(7)));
    assert!(args.iter().any(|a| a == "--title=-Kai the Grey"), "{args:?}");
    assert!(args.iter().any(|a| a == "--template=place.md"), "{args:?}");
    assert!(args.iter().any(|a| a == "--extra=feature=7"), "{args:?}");
    assert!(
        !args.iter().any(|a| a == "--title" || a == "--template"),
        "no flag stands alone: {args:?}"
    );
}

// `FeatureId` prints itself as "feature 7", which names a feature rather than numbering
// one; the template variable has to carry the number or the frontmatter link is wrong.
#[test]
fn a_feature_is_handed_over_as_its_number() {
    let args = new_args(NoteKind::Place, "Riverford", Some(FeatureId(12)));
    assert!(args.iter().any(|a| a == "--extra=feature=12"), "{args:?}");
    assert!(
        !args.iter().any(|a| a.to_string_lossy().contains("feature 12")),
        "the Display form must not reach the argv: {args:?}"
    );
}

// A person, a faction and a session hang off no feature, so the variable is absent
// rather than empty — an empty one would render as a blank frontmatter link.
#[test]
fn a_note_with_no_feature_is_given_no_feature_variable() {
    let args = new_args(NoteKind::Person, "Sir Bedivere", None);
    assert!(
        !args.iter().any(|a| a.to_string_lossy().starts_with("--extra")),
        "{args:?}"
    );
}

// A prompting child on the task pool never returns, and with one job slot that kills note
// creation for the life of the process. Every verb has to say it will not be asked.
#[test]
fn every_invocation_refuses_to_be_prompted() {
    let vectors = [
        new_args(NoteKind::Place, "x", None),
        init_args(Path::new("/tmp/notes")),
        edit_args(Path::new("/tmp/notes/a.md")),
    ];
    for args in vectors {
        assert!(args.iter().any(|a| a == "--no-input"), "{args:?}");
    }
}

// `--print-path` is how the path comes back at all; without it a created note is
// unreachable and nothing could be linked to a feature.
#[test]
fn creating_a_note_asks_for_the_path_back() {
    let args = new_args(NoteKind::Session, "Session 4", None);
    assert!(args.iter().any(|a| a == "--print-path"), "{args:?}");
}

// `zk init` takes its directory positionally and creates it; naming it is what lets init
// run from anywhere rather than needing the directory to exist first.
#[test]
fn init_names_the_directory_it_makes() {
    let args = init_args(Path::new("/tmp/c/notes"));
    assert_eq!(args.last().unwrap(), "/tmp/c/notes");
    assert!(args.iter().any(|a| a == "init"), "{args:?}");
}

// `zk new` resolves the note it creates against the working directory, so naming the
// notebook is not on its own enough to say where the note goes.
#[test]
fn a_note_is_created_from_inside_the_notes_directory() {
    let (_tmp, book) = notebook();
    std::fs::create_dir_all(book.root().join(".zk")).unwrap();
    let runner = Recorder::printing(book.root().join("riverford-a1b2.md").display().to_string());

    book.create(&runner, NoteKind::Place, "Riverford", Some(FeatureId(1)))
        .expect("create");

    let dirs = runner.dirs();
    assert_eq!(
        dirs.last().unwrap(),
        book.root(),
        "zk new must run from the notes directory"
    );
}

// The slug a feature is found by is the stem of the note's own filename, which is what the
// template wrote into its tag — one string rather than two that have to agree.
#[test]
fn the_slug_is_the_stem_of_the_notes_own_filename() {
    let (_tmp, book) = notebook();
    std::fs::create_dir_all(book.root().join(".zk")).unwrap();
    let runner = Recorder::printing(book.root().join("riverford-a1b2.md").display().to_string());

    let note = book
        .create(&runner, NoteKind::Place, "Riverford", None)
        .expect("create");

    assert_eq!(note.path, "riverford-a1b2.md");
    assert_eq!(note.slug, "riverford-a1b2");
}

// Two notes titled the same get different filenames from zk, and therefore different
// slugs — which is the acceptance criterion that two Riverfords are two findable places.
#[test]
fn two_notes_titled_the_same_take_their_slugs_from_their_own_filenames() {
    let (_tmp, book) = notebook();
    std::fs::create_dir_all(book.root().join(".zk")).unwrap();

    let first = book
        .create(
            &Recorder::printing(book.root().join("riverford-a1b2.md").display().to_string()),
            NoteKind::Place,
            "Riverford",
            None,
        )
        .expect("first");
    let second = book
        .create(
            &Recorder::printing(book.root().join("riverford-c3d4.md").display().to_string()),
            NoteKind::Place,
            "Riverford",
            None,
        )
        .expect("second");

    assert_ne!(first.slug, second.slug);
    assert_ne!(first.path, second.path);
}

// A path is taken out of the notes directory by components and not by text: stripping the
// text would turn `<notes>foo/x.md` into `foo/x.md`, a path that passes every later check
// and names a file outside the notebook.
#[test]
fn a_sibling_directory_sharing_the_prefix_is_not_inside_the_notebook() {
    let (tmp, book) = notebook();
    let sibling = tmp.path().join("my-campaign").join("notesfoo");
    std::fs::create_dir_all(&sibling).unwrap();
    let runner = Recorder::printing(sibling.join("x.md").display().to_string());

    std::fs::create_dir_all(book.root().join(".zk")).unwrap();
    let error = book
        .create(&runner, NoteKind::Place, "X", None)
        .expect_err("a sibling directory is not the notebook");

    assert!(matches!(error, NoteError::BadPath { .. }), "{error:?}");
}

// A campaign reached through a symlink prints one path and was opened by another; both
// sides are resolved before one is taken out of the other, or a perfectly good note dies.
#[test]
fn a_notes_directory_reached_through_a_symlink_still_yields_a_relative_path() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let real = tmp.path().join("real-campaign");
    std::fs::create_dir_all(real.join("notes").join(".zk")).unwrap();
    let link = tmp.path().join("linked-campaign");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");

    let book = Notebook::of(&link);
    let printed = real.join("notes").join("riverford-a1b2.md");
    std::fs::write(&printed, "x").unwrap();
    let runner = Recorder::printing(printed.display().to_string());

    let note = book
        .create(&runner, NoteKind::Place, "Riverford", None)
        .expect("a note under the real path is still inside the linked notebook");

    assert_eq!(note.path, "riverford-a1b2.md");
}

// The title is refused where it is cheapest, before a process exists — zk would otherwise
// name the note `Untitled` and the GM would have no way to tell two of them apart.
#[test]
fn an_empty_or_blank_title_is_refused_before_anything_runs() {
    let (_tmp, book) = notebook();
    for title in ["", "   ", "\t\n"] {
        let error = book
            .create(&NeverRuns, NoteKind::Person, title, None)
            .expect_err("a note needs a title");
        assert!(matches!(error, NoteError::EmptyTitle), "{title:?}");
    }
}

// A notebook is initialised once; running `zk init` over one that exists is an error from
// zk itself, so the guard decides whether it runs at all rather than whether it is worth
// running.
#[test]
fn a_notebook_that_already_exists_is_not_initialised_again() {
    let (_tmp, book) = notebook();
    std::fs::create_dir_all(book.root().join(".zk")).unwrap();
    let runner = Recorder::default();

    book.ensure(&runner).expect("ensure");

    assert!(
        !runner.args().iter().any(|a| a.contains(&"init".to_owned())),
        "init was run over an existing notebook: {:?}",
        runner.args()
    );
}

// The templates are ensured every time rather than only when the notebook is made, so a
// notebook the GM made themselves gains them instead of failing every note for ever.
#[test]
fn the_templates_are_installed_into_a_notebook_someone_else_made() {
    let (_tmp, book) = notebook();
    std::fs::create_dir_all(book.root().join(".zk")).unwrap();

    book.ensure(&Recorder::default()).expect("ensure");

    for kind in NoteKind::all() {
        let path = book.root().join(".zk").join("templates").join(kind.template());
        assert_eq!(
            std::fs::read_to_string(&path).expect("template"),
            kind.body(),
            "{kind:?}"
        );
    }
}

// A half-written notebook must repair itself rather than refusing every note from then
// on, which is what an init that only ran once could not do.
#[test]
fn a_notebook_missing_one_template_gains_it_on_the_next_note() {
    let (_tmp, book) = notebook();
    let templates = book.root().join(".zk").join("templates");
    std::fs::create_dir_all(&templates).unwrap();
    std::fs::write(templates.join("place.md"), NoteKind::Place.body()).unwrap();

    book.ensure(&Recorder::default()).expect("ensure");

    assert!(templates.join("person.md").exists());
    assert!(templates.join("faction.md").exists());
    assert!(templates.join("session.md").exists());
}

// A template is the campaign's from the moment it lands: a GM's edit has to outlive the
// version that shipped it, or improving one here would silently revert their work.
#[test]
fn a_template_the_gm_has_edited_is_never_overwritten() {
    let (_tmp, book) = notebook();
    let templates = book.root().join(".zk").join("templates");
    std::fs::create_dir_all(&templates).unwrap();
    std::fs::write(templates.join("place.md"), "mine, not yours").unwrap();

    book.ensure(&Recorder::default()).expect("ensure");
    book.ensure(&Recorder::default()).expect("ensure again");

    assert_eq!(
        std::fs::read_to_string(templates.join("place.md")).unwrap(),
        "mine, not yours"
    );
}

// The configuration replaces the annotated default `zk init` writes — declining to
// overwrite it would leave the filename rule as zk's, which is how the notes in a real
// run came out named by a bare id. Over an existing notebook it is the GM's and is left
// alone. It stays only a convenience, because the tag comes from the filename rather than
// from this file.
#[test]
fn the_configuration_is_written_only_when_the_notebook_is_made() {
    let (_tmp, book) = notebook();
    let runner = Recorder {
        init_makes: Some(book.root().to_owned()),
        ..Recorder::default()
    };

    book.ensure(&runner).expect("ensure");

    let config = book.root().join(".zk").join("config.toml");
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        campaign::notebook::CONFIG
    );

    std::fs::write(&config, "the GM's own").unwrap();
    book.ensure(&runner).expect("ensure again");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "the GM's own");
}

// A campaign directory may have been handed to a GM rather than made by them, and
// `note_path_refusal` is lexical by its own admission — so the one place that opens a
// note is the place to find out where the bytes really are.
#[test]
fn a_note_resolving_outside_the_notebook_is_never_opened() {
    let (tmp, book) = notebook();
    let outside = tmp.path().join("secrets.md");
    std::fs::write(&outside, "not yours").unwrap();
    std::os::unix::fs::symlink(&outside, book.root().join("lore.md")).expect("symlink");

    let error = book
        .open(&NeverRuns, "lore.md")
        .expect_err("a note that resolves outside must not be opened");

    assert!(matches!(error, NoteError::BadPath { .. }), "{error:?}");
}

// The path rules a feature is held to are the ones a note is opened under, so neither
// side can be reached with something the other would refuse.
#[test]
fn a_path_a_feature_could_not_carry_is_never_opened() {
    let (_tmp, book) = notebook();
    for path in ["", "../escape.md", "/etc/passwd", "-flag.md"] {
        let error = book
            .open(&NeverRuns, path)
            .expect_err("must be refused: {path}");
        assert!(matches!(error, NoteError::BadPath { .. }), "{path:?}");
    }
}

// zk prints the path and a line ending; a hook that printed something first must not turn
// the last line into part of the path.
#[test]
fn the_printed_path_is_the_last_thing_said() {
    assert_eq!(
        printed_path(b"/c/notes/a.md\n"),
        Some(PathBuf::from("/c/notes/a.md"))
    );
    assert_eq!(
        printed_path(b"indexing...\n/c/notes/a.md\n\n"),
        Some(PathBuf::from("/c/notes/a.md"))
    );
    assert_eq!(printed_path(b"   \n"), None);
    assert_eq!(printed_path(b""), None);
}

// A note that zk claims to have made but did not name leaves nothing to link, and must
// say so rather than storing an empty path on a feature.
#[test]
fn a_note_zk_does_not_name_is_refused() {
    let (_tmp, book) = notebook();
    std::fs::create_dir_all(book.root().join(".zk")).unwrap();

    let error = book
        .create(&Recorder::printing(""), NoteKind::Place, "Riverford", None)
        .expect_err("no path means no note");

    assert!(matches!(error, NoteError::ZkRefused { .. }), "{error:?}");
}

// zk's stderr is many lines of absolute paths and a status line holds one, so what is
// carried is the first thing it said rather than everything.
#[test]
fn a_refusal_carries_the_first_line_zk_said() {
    let (_tmp, book) = notebook();
    std::fs::create_dir_all(book.root().join(".zk")).unwrap();
    let runner = Recorder {
        fail_with: Some("zk: error: cannot find template at wobble.md\n  at /a/b/c\n".to_owned()),
        ..Recorder::default()
    };

    let error = book
        .create(&runner, NoteKind::Place, "Riverford", None)
        .expect_err("a failing zk is a failing note");

    let said = error.to_string();
    assert!(said.contains("cannot find template"), "{said}");
    assert!(!said.contains("/a/b/c"), "only the first line: {said}");
}

// The slug is the stem whatever the filename rule is, because the template writes it
// from the same place — a GM who changes `note.filename` does not break issue #7.
#[test]
fn a_slug_is_a_stem_whatever_the_filename_looks_like() {
    assert_eq!(slug_of("riverford-a1b2.md"), "riverford-a1b2");
    assert_eq!(slug_of("a1b2.md"), "a1b2");
    assert_eq!(slug_of("2026-09-03-session-4.md"), "2026-09-03-session-4");
    assert_eq!(slug_of("no-extension"), "no-extension");
}

// The notes directory is named through `layout` rather than spelled here, so the one
// place the format fixes the name stays the only place.
#[test]
fn the_notebook_is_the_campaigns_notes_directory() {
    let book = Notebook::of("/tmp/my-campaign");
    assert_eq!(book.root(), campaign::layout::notes(Path::new("/tmp/my-campaign")));
}

// The notebook crosses to a task pool, so a future field that broke that would otherwise
// only show up in the editor.
#[test]
fn a_notebook_can_be_moved_to_another_thread() {
    fn assert_send<T: Send + 'static>() {}
    assert_send::<Notebook>();
    assert_send::<NoteError>();
    assert_send::<notebook::NewNote>();
}
