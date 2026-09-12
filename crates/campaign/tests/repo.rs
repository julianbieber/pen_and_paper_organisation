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
    GitError, GitRunner, IGNORE, IGNORE_FILE, RemoteOutcome, Repo, Synced, abort_rebase_args,
    add_args, add_origin_args, changed_between_args, commit_args, committer_ident_args,
    current_branch_args, head_args, inside_work_tree_args, init_args, origin_url_args,
    pull_rebase_args, push_args, remote_has_branch_args, remote_refusal, staged_anything_args,
    stamp_of_ident, sync_subject,
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

struct Reply {
    status: i32,
    stdout: &'static str,
    stderr: &'static str,
}

impl Reply {
    fn ok() -> Self {
        Self {
            status: 0,
            stdout: "",
            stderr: "",
        }
    }

    fn exits(code: i32) -> Self {
        Self {
            status: code << 8,
            stdout: "",
            stderr: "",
        }
    }

    fn stdout(mut self, stdout: &'static str) -> Self {
        self.stdout = stdout;
        self
    }

    fn stderr(mut self, stderr: &'static str) -> Self {
        self.stderr = stderr;
        self.status = 1 << 8;
        self
    }
}

struct Scripted<F> {
    calls: RefCell<Vec<Vec<String>>>,
    respond: F,
}

impl<F: Fn(usize, &[String]) -> Reply> Scripted<F> {
    fn new(respond: F) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            respond,
        }
    }

    fn verbs(&self) -> Vec<String> {
        self.calls
            .borrow()
            .iter()
            .map(|args| args[0].clone())
            .collect()
    }
}

impl<F: Fn(usize, &[String]) -> Reply> GitRunner for Scripted<F> {
    fn run(&self, _dir: &Path, args: &[OsString]) -> Result<Output, GitError> {
        let text: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let index = self.calls.borrow().len();
        self.calls.borrow_mut().push(text.clone());
        let reply = (self.respond)(index, &text);
        Ok(Output {
            status: ExitStatus::from_raw(reply.status),
            stdout: reply.stdout.as_bytes().to_vec(),
            stderr: reply.stderr.as_bytes().to_vec(),
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

// `current_branch_args` is what `sync` asks first: the branch to commit to, or a
// detached `HEAD`.
#[test]
fn current_branch_args_is_symbolic_ref_quiet_short_head() {
    assert_eq!(
        current_branch_args(),
        vec![
            OsString::from("symbolic-ref"),
            OsString::from("--quiet"),
            OsString::from("--short"),
            OsString::from("HEAD"),
        ]
    );
}

// The `-- .` pathspec matches `add_args`, for the reason it carries one.
#[test]
fn staged_anything_args_diffs_the_working_directory_quietly() {
    assert_eq!(
        staged_anything_args(),
        vec![
            OsString::from("diff"),
            OsString::from("--cached"),
            OsString::from("--quiet"),
            OsString::from("--"),
            OsString::from("."),
        ]
    );
}

// `stamp_of_ident` reads this line's last two words.
#[test]
fn committer_ident_args_asks_var() {
    assert_eq!(
        committer_ident_args(),
        vec![OsString::from("var"), OsString::from("GIT_COMMITTER_IDENT")]
    );
}

// `sync` reads a non-zero exit here as "no remote", never a refusal.
#[test]
fn origin_url_args_asks_remote_get_url() {
    assert_eq!(
        origin_url_args(),
        vec![
            OsString::from("remote"),
            OsString::from("get-url"),
            OsString::from("origin"),
        ]
    );
}

// The `refs/heads/<branch>` ref is what turns exit 2 into "the remote has no such branch
// yet", the shape a fresh bare repository answers with.
#[test]
fn remote_has_branch_args_names_the_branchs_ref() {
    assert_eq!(
        remote_has_branch_args("main"),
        vec![
            OsString::from("ls-remote"),
            OsString::from("--exit-code"),
            OsString::from("--heads"),
            OsString::from("origin"),
            OsString::from("refs/heads/main"),
        ]
    );
}

// `sync` calls this twice, before and after a pull, to tell whether it moved `HEAD`.
#[test]
fn head_args_is_rev_parse_head() {
    assert_eq!(
        head_args(),
        vec![OsString::from("rev-parse"), OsString::from("HEAD")]
    );
}

// `origin <branch>` are explicit, unlike a bare `pull --rebase`, because a remote just
// set from the panel has no upstream configured yet.
#[test]
fn pull_rebase_args_names_origin_and_the_branch_explicitly() {
    assert_eq!(
        pull_rebase_args("main"),
        vec![
            OsString::from("pull"),
            OsString::from("--rebase"),
            OsString::from("origin"),
            OsString::from("main"),
        ]
    );
}

// `sync` tells a conflict from every other pull failure by whether this succeeds.
#[test]
fn abort_rebase_args_is_rebase_abort() {
    assert_eq!(
        abort_rebase_args(),
        vec![OsString::from("rebase"), OsString::from("--abort")]
    );
}

// `--relative` is what keeps an adopted campaign's changed paths from naming a file
// outside it.
#[test]
fn changed_between_args_is_relative_to_the_working_directory() {
    assert_eq!(
        changed_between_args("aaa", "bbb"),
        vec![
            OsString::from("diff"),
            OsString::from("--name-only"),
            OsString::from("--relative"),
            OsString::from("aaa"),
            OsString::from("bbb"),
        ]
    );
}

// `--set-upstream` is what fixes the upstream on the first push after a remote is set.
#[test]
fn push_args_sets_the_upstream() {
    assert_eq!(
        push_args("main"),
        vec![
            OsString::from("push"),
            OsString::from("--set-upstream"),
            OsString::from("origin"),
            OsString::from("main"),
        ]
    );
}

// The `--` before the value is what keeps a URL beginning with a dash from being read as
// another option.
#[test]
fn add_origin_args_places_a_pathspec_separator_before_the_url() {
    assert_eq!(
        add_origin_args("git@example.invalid:campaign.git"),
        vec![
            OsString::from("remote"),
            OsString::from("add"),
            OsString::from("--"),
            OsString::from("origin"),
            OsString::from("git@example.invalid:campaign.git"),
        ]
    );
}

fn sync_repo() -> Repo {
    Repo::at(Path::new("/campaign"))
}

// The simplest sync: nothing dirty, no remote. Only branch, add, diff and the origin
// probe run, and nothing is committed.
#[test]
fn sync_with_nothing_staged_and_no_origin_commits_nothing() {
    let runner = Scripted::new(|index, _args| match index {
        0 => Reply::ok().stdout("main\n"),
        1 => Reply::ok(),
        2 => Reply::exits(0),
        3 => Reply::exits(1),
        other => panic!("unexpected call {other}"),
    });

    let synced = sync_repo().sync(&runner).expect("sync");
    assert_eq!(synced.commit, None);
    assert_eq!(synced.remote, RemoteOutcome::NoRemote);
    assert!(synced.changed.is_empty());
    assert_eq!(runner.verbs(), vec!["symbolic-ref", "add", "diff", "remote"]);
}

// With something staged, `sync` reads the committer identity and commits with a `Sync`
// subject, before ever asking about a remote.
#[test]
fn sync_with_staged_changes_runs_var_then_commit() {
    let runner = Scripted::new(|index, _args| match index {
        0 => Reply::ok().stdout("main\n"),
        1 => Reply::ok(),
        2 => Reply::exits(1),
        3 => Reply::ok().stdout("pnp tests <pnp@example.invalid> 1757680000 +0200\n"),
        4 => Reply::ok(),
        5 => Reply::exits(1),
        other => panic!("unexpected call {other}"),
    });

    let synced = sync_repo().sync(&runner).expect("sync");
    assert_eq!(
        synced.commit.as_deref(),
        Some(sync_subject(1_757_680_000, 120).as_str())
    );
    assert_eq!(
        runner.verbs(),
        vec!["symbolic-ref", "add", "diff", "var", "commit", "remote"]
    );
}

// Exit 2 from `ls-remote` is a remote with no such branch yet — a fresh bare repository —
// so the pull is skipped entirely and `sync` goes straight to the push.
#[test]
fn ls_remote_exit_2_skips_pull_and_pushes() {
    let runner = Scripted::new(|index, _args| match index {
        0 => Reply::ok().stdout("main\n"),
        1 => Reply::ok(),
        2 => Reply::exits(0),
        3 => Reply::ok().stdout("origin-url\n"),
        4 => Reply::exits(2),
        5 => Reply::ok(),
        other => panic!("unexpected call {other}"),
    });

    let synced = sync_repo().sync(&runner).expect("sync");
    assert_eq!(synced.remote, RemoteOutcome::Pushed);
    assert!(synced.changed.is_empty());
    assert_eq!(
        runner.verbs(),
        vec!["symbolic-ref", "add", "diff", "remote", "ls-remote", "push"]
    );
}

// A failed pull followed by a successful abort is a conflict: the local commit stays and
// nothing is pushed.
#[test]
fn a_failed_pull_with_a_successful_abort_is_a_conflict_and_never_pushes() {
    let runner = Scripted::new(|index, _args| match index {
        0 => Reply::ok().stdout("main\n"),
        1 => Reply::ok(),
        2 => Reply::exits(0),
        3 => Reply::ok().stdout("origin-url\n"),
        4 => Reply::exits(0),
        5 => Reply::ok().stdout("aaa111\n"),
        6 => Reply::ok().stderr("CONFLICT (content): Merge conflict in world.ron"),
        7 => Reply::ok(),
        other => panic!("unexpected call {other}"),
    });

    let synced = sync_repo().sync(&runner).expect("sync");
    assert_eq!(
        synced.remote,
        RemoteOutcome::Conflict {
            message: "CONFLICT (content): Merge conflict in world.ron".to_owned(),
        }
    );
    assert!(synced.changed.is_empty());
    assert_eq!(
        runner.verbs(),
        vec!["symbolic-ref", "add", "diff", "remote", "ls-remote", "rev-parse", "pull", "rebase"]
    );
}

// A failed pull whose abort also fails — no rebase was in progress, an unreachable
// remote, say — is reported as a plain refusal of `pull` rather than a conflict.
#[test]
fn a_failed_pull_with_a_failed_abort_is_refused_pull() {
    let runner = Scripted::new(|index, _args| match index {
        0 => Reply::ok().stdout("main\n"),
        1 => Reply::ok(),
        2 => Reply::exits(0),
        3 => Reply::ok().stdout("origin-url\n"),
        4 => Reply::exits(0),
        5 => Reply::ok().stdout("aaa111\n"),
        6 => Reply::ok().stderr("fatal: unable to access the remote"),
        7 => Reply::exits(1),
        other => panic!("unexpected call {other}"),
    });

    let synced = sync_repo().sync(&runner).expect("sync");
    assert_eq!(
        synced.remote,
        RemoteOutcome::Refused {
            verb: "pull",
            message: "fatal: unable to access the remote".to_owned(),
        }
    );
}

// A pull that moves `HEAD` runs `diff --name-only` between the two and fills `changed`.
#[test]
fn a_moved_head_fills_changed_from_diff_name_only() {
    let runner = Scripted::new(|index, _args| match index {
        0 => Reply::ok().stdout("main\n"),
        1 => Reply::ok(),
        2 => Reply::exits(0),
        3 => Reply::ok().stdout("origin-url\n"),
        4 => Reply::exits(0),
        5 => Reply::ok().stdout("aaa111\n"),
        6 => Reply::ok(),
        7 => Reply::ok().stdout("bbb222\n"),
        8 => Reply::ok().stdout("world.ron\ndungeons/crypt.ron\n"),
        9 => Reply::ok(),
        other => panic!("unexpected call {other}"),
    });

    let synced = sync_repo().sync(&runner).expect("sync");
    assert_eq!(synced.remote, RemoteOutcome::Pushed);
    assert_eq!(
        synced.changed,
        vec![PathBuf::from("world.ron"), PathBuf::from("dungeons/crypt.ron")]
    );
    assert_eq!(runner.verbs()[8], "diff");
}

// A failed push keeps whatever the pull brought in — the editor still has to reload it,
// whether or not the push that would have shared it back succeeded.
#[test]
fn a_failed_push_keeps_changed() {
    let runner = Scripted::new(|index, _args| match index {
        0 => Reply::ok().stdout("main\n"),
        1 => Reply::ok(),
        2 => Reply::exits(0),
        3 => Reply::ok().stdout("origin-url\n"),
        4 => Reply::exits(0),
        5 => Reply::ok().stdout("aaa111\n"),
        6 => Reply::ok(),
        7 => Reply::ok().stdout("bbb222\n"),
        8 => Reply::ok().stdout("world.ron\n"),
        9 => Reply::ok().stderr("! [rejected] main -> main"),
        other => panic!("unexpected call {other}"),
    });

    let synced = sync_repo().sync(&runner).expect("sync");
    assert_eq!(
        synced.remote,
        RemoteOutcome::Refused {
            verb: "push",
            message: "! [rejected] main -> main".to_owned(),
        }
    );
    assert_eq!(synced.changed, vec![PathBuf::from("world.ron")]);
}

// A detached `HEAD` refuses before anything else is asked, whether or not something is
// staged.
#[test]
fn a_detached_head_is_not_on_a_branch() {
    let runner = Scripted::new(|index, _args| match index {
        0 => Reply::exits(1),
        other => panic!("unexpected call {other}"),
    });

    let error = sync_repo().sync(&runner).expect_err("detached HEAD");
    assert!(matches!(error, GitError::NotOnABranch));
    assert_eq!(runner.verbs(), vec!["symbolic-ref"]);
}

// A missing `git` must stop `sync` before anything else runs, the same way it stops
// `begin`.
#[test]
fn sync_with_missing_git_is_git_missing() {
    let error = sync_repo().sync(&AlwaysMissing).expect_err("git is missing");
    assert!(matches!(error, GitError::GitMissing));
}

// `stamp_of_ident` reads from the back, so a name or a mail address with spaces of its
// own cannot throw the parse off.
#[test]
fn stamp_of_ident_reads_the_trailing_timestamp_and_offset() {
    assert_eq!(
        stamp_of_ident("pnp tests <pnp@example.invalid> 1757680000 +0200"),
        Some((1_757_680_000, 120))
    );
    assert_eq!(
        stamp_of_ident("Julian Bieber <julian@example.invalid> 1700000000 -0530"),
        Some((1_700_000_000, -330))
    );
}

// A line that is not a real committer identity must not be parsed into a bogus
// timestamp — a wrong sync subject would be silently, permanently wrong.
#[test]
fn stamp_of_ident_on_garbage_is_none() {
    assert_eq!(stamp_of_ident(""), None);
    assert_eq!(stamp_of_ident("not a committer ident"), None);
    assert_eq!(stamp_of_ident("name <mail> notanumber +0200"), None);
}

// The epoch is `civil_from_days`' base case: it needs no rounding, so it must come back
// exactly, not off by the timezone or the day.
#[test]
fn sync_subject_at_the_epoch_is_1970_01_01() {
    assert_eq!(sync_subject(0, 0), "Sync 1970-01-01 00:00");
}

// A negative offset crossing midnight rolls the date backward.
#[test]
fn sync_subject_rolls_the_date_across_a_negative_offset_at_midnight() {
    assert_eq!(sync_subject(1_800, -60), "Sync 1969-12-31 23:30");
}

// 2026-01-01T00:00:00Z at UTC+1 is 2026-01-01T01:00 local.
#[test]
fn sync_subject_formats_a_known_2026_instant() {
    assert_eq!(sync_subject(1_767_225_600, 60), "Sync 2026-01-01 01:00");
}

// An empty or whitespace-only URL must be refused before it ever reaches git.
#[test]
fn remote_refusal_on_an_empty_or_blank_url() {
    assert_eq!(remote_refusal(""), Some("a remote needs a URL"));
    assert_eq!(remote_refusal("   "), Some("a remote needs a URL"));
}

// A newline in the URL could read as a second git argument, so it is refused rather
// than passed through.
#[test]
fn remote_refusal_on_a_multiline_url() {
    assert_eq!(
        remote_refusal("git@example.invalid:campaign.git\nrm -rf /"),
        Some("a remote URL may not contain a newline or a NUL")
    );
}

// An ordinary URL is the one shape `remote_refusal` must let through, or Set remote
// could never succeed.
#[test]
fn remote_refusal_accepts_an_ordinary_url() {
    assert_eq!(remote_refusal("git@example.invalid:campaign.git"), None);
}

// Every shape `summary` can report must read differently, or a GM cannot tell two
// outcomes apart on the status line.
#[test]
fn every_summary_case_is_distinct() {
    let subject = || Some("Sync 2026-01-01 00:00".to_owned());
    let cases = [
        Synced {
            commit: None,
            remote: RemoteOutcome::NoRemote,
            changed: Vec::new(),
        },
        Synced {
            commit: subject(),
            remote: RemoteOutcome::NoRemote,
            changed: Vec::new(),
        },
        Synced {
            commit: None,
            remote: RemoteOutcome::Pushed,
            changed: Vec::new(),
        },
        Synced {
            commit: subject(),
            remote: RemoteOutcome::Pushed,
            changed: Vec::new(),
        },
        Synced {
            commit: None,
            remote: RemoteOutcome::Pushed,
            changed: vec![PathBuf::from("world.ron")],
        },
        Synced {
            commit: None,
            remote: RemoteOutcome::Conflict {
                message: "conflict".to_owned(),
            },
            changed: Vec::new(),
        },
        Synced {
            commit: None,
            remote: RemoteOutcome::Refused {
                verb: "push",
                message: "refused".to_owned(),
            },
            changed: Vec::new(),
        },
        Synced {
            commit: subject(),
            remote: RemoteOutcome::Refused {
                verb: "push",
                message: "refused".to_owned(),
            },
            changed: Vec::new(),
        },
    ];

    let mut summaries: Vec<String> = cases.iter().map(Synced::summary).collect();
    let all = summaries.clone();
    summaries.sort();
    summaries.dedup();
    assert_eq!(summaries.len(), all.len(), "{all:#?}");
}
