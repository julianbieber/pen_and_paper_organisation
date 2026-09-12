//! What `repo.rs` decides before `git` is ever involved: the argument vectors, the
//! order the birth sequence runs its commands in, adoption, the ignore file's content
//! and its refusal to overwrite one, and every error variant.
//!
//! Every test here runs against a recorded runner, in `tests/notebook.rs`'s shape, so
//! this crate's tests keep passing with nothing on `PATH`.

use std::cell::RefCell;
use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};

use campaign::repo::{
    GitError, GitRunner, IGNORE, IGNORE_FILE, Repo, add_args, commit_args, inside_work_tree_args,
    init_args,
};

#[derive(Default)]
struct Recorder {
    calls: RefCell<Vec<(PathBuf, Vec<String>)>>,
    inside_work_tree: bool,
    fail_verb: Option<&'static str>,
}

impl Recorder {
    fn args(&self) -> Vec<Vec<String>> {
        self.calls.borrow().iter().map(|(_, a)| a.clone()).collect()
    }

    fn dirs(&self) -> Vec<PathBuf> {
        self.calls.borrow().iter().map(|(d, _)| d.clone()).collect()
    }
}

impl GitRunner for Recorder {
    fn run(&self, dir: &Path, args: &[OsString]) -> Result<Output, GitError> {
        let text: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        self.calls.borrow_mut().push((dir.to_owned(), text.clone()));

        let verb = text.first().map(String::as_str);
        if verb == Some("rev-parse") {
            let status = if self.inside_work_tree { 0 } else { 128 };
            return Ok(Output {
                status: ExitStatus::from_raw(status),
                stdout: Vec::new(),
                stderr: Vec::new(),
            });
        }

        if self.fail_verb.is_some() && self.fail_verb == verb {
            return Ok(Output {
                status: ExitStatus::from_raw(256),
                stdout: Vec::new(),
                stderr: b"it refused".to_vec(),
            });
        }

        Ok(Output {
            status: ExitStatus::from_raw(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }
}

struct AlwaysMissing;

impl GitRunner for AlwaysMissing {
    fn run(&self, _dir: &Path, _args: &[OsString]) -> Result<Output, GitError> {
        Err(GitError::GitMissing)
    }
}

fn campaign_repo() -> (tempfile::TempDir, Repo) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let repo = Repo::at(tmp.path());
    (tmp, repo)
}

// `init` takes no argument: the directory is the current one, set by the runner.
#[test]
fn init_args_is_exactly_init() {
    assert_eq!(init_args(), vec![OsString::from("init")]);
}

// `rev-parse --is-inside-work-tree` is what `begin` asks before deciding whether to run
// `init` at all.
#[test]
fn inside_work_tree_args_asks_rev_parse() {
    assert_eq!(
        inside_work_tree_args(),
        vec![
            OsString::from("rev-parse"),
            OsString::from("--is-inside-work-tree"),
        ]
    );
}

// The `-- .` pathspec on `add` is what keeps an adopted campaign from staging the rest
// of the GM's own repository.
#[test]
fn add_args_stages_only_the_working_directory() {
    assert_eq!(
        add_args(),
        vec![
            OsString::from("add"),
            OsString::from("--all"),
            OsString::from("--"),
            OsString::from("."),
        ]
    );
}

// The message is joined to its flag, and the `-- .` pathspec matches `add_args`, for
// the reason `add_args` carries one.
#[test]
fn commit_args_joins_the_message_and_matches_adds_pathspec() {
    assert_eq!(
        commit_args("Create the campaign"),
        vec![
            OsString::from("commit"),
            OsString::from("--message=Create the campaign"),
            OsString::from("--"),
            OsString::from("."),
        ]
    );
}

// A message beginning with a dash is the case a following word would be misread as
// another option; the joined form gets it through unchanged.
#[test]
fn commit_args_joins_a_dash_leading_message() {
    let args = commit_args("-not-an-option");
    assert_eq!(args[1], OsString::from("--message=-not-an-option"));
}

// The birth sequence on a fresh directory: rev-parse, init, add, commit, in that order,
// every one run with the root as the working directory.
#[test]
fn begin_on_a_fresh_root_runs_the_whole_sequence_in_order() {
    let (_tmp, repo) = campaign_repo();
    let runner = Recorder::default();

    repo.begin(&runner).expect("begin");

    let args = runner.args();
    let verbs: Vec<&str> = args.iter().map(|a| a[0].as_str()).collect();
    assert_eq!(verbs, vec!["rev-parse", "init", "add", "commit"]);
    assert!(
        runner.dirs().iter().all(|dir| *dir == repo.root()),
        "{:?}",
        runner.dirs()
    );
}

// A root already inside a work tree is adopted: no `init`, and the campaign is still
// added and committed.
#[test]
fn begin_on_an_adopted_root_runs_no_init() {
    let (_tmp, repo) = campaign_repo();
    let runner = Recorder {
        inside_work_tree: true,
        ..Recorder::default()
    };

    repo.begin(&runner).expect("begin");

    let verbs: Vec<String> = runner.args().into_iter().map(|a| a[0].clone()).collect();
    assert_eq!(verbs, vec!["rev-parse", "add", "commit"]);
}

// The ignore file carries exactly the constant's bytes, so the two patterns the issue
// names are the only two written.
#[test]
fn begin_writes_the_ignore_file_verbatim() {
    let (tmp, repo) = campaign_repo();
    repo.begin(&Recorder::default()).expect("begin");

    let written = std::fs::read_to_string(tmp.path().join(IGNORE_FILE)).expect("read");
    assert_eq!(written, IGNORE);
}

// A `.gitignore` the GM already wrote is never overwritten — `create_new` is what makes
// that a refusal rather than a race.
#[test]
fn begin_never_overwrites_an_existing_ignore_file() {
    let (tmp, repo) = campaign_repo();
    std::fs::write(tmp.path().join(IGNORE_FILE), "# mine\n").expect("write");

    repo.begin(&Recorder::default()).expect("begin");

    let written = std::fs::read_to_string(tmp.path().join(IGNORE_FILE)).expect("read");
    assert_eq!(written, "# mine\n");
}

// A runner answering `GitMissing` makes `begin` fail the same way, and write nothing
// but the ignore file — the campaign this belongs to is still openable.
#[test]
fn begin_with_no_git_writes_only_the_ignore_file() {
    let (tmp, repo) = campaign_repo();

    let error = repo.begin(&AlwaysMissing).expect_err("git is missing");
    assert!(matches!(error, GitError::GitMissing));

    let entries: Vec<String> = std::fs::read_dir(tmp.path())
        .expect("read dir")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, vec![IGNORE_FILE]);
}

// A commit that exits non-zero is reported with the verb and the first line of stderr,
// so a status line has something concrete to say.
#[test]
fn a_refused_commit_names_its_verb_and_the_first_line_of_stderr() {
    let (_tmp, repo) = campaign_repo();
    let runner = Recorder {
        fail_verb: Some("commit"),
        ..Recorder::default()
    };

    let error = repo.begin(&runner).expect_err("the commit is refused");
    match error {
        GitError::GitRefused { verb, message } => {
            assert_eq!(verb, "commit");
            assert_eq!(message, "it refused");
        }
        other => panic!("expected GitRefused, got {other:?}"),
    }
}

// An `init` that is refused stops the sequence before `add` or `commit` ever run.
#[test]
fn a_refused_init_stops_the_sequence() {
    let (_tmp, repo) = campaign_repo();
    let runner = Recorder {
        fail_verb: Some("init"),
        ..Recorder::default()
    };

    let error = repo.begin(&runner).expect_err("init is refused");
    assert!(matches!(error, GitError::GitRefused { verb: "init", .. }));

    let verbs: Vec<String> = runner.args().into_iter().map(|a| a[0].clone()).collect();
    assert_eq!(verbs, vec!["rev-parse", "init"]);
}
