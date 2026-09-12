//! The handful of things only the real `git` can answer: that a created campaign is a
//! repository with nothing to commit and one commit in its log, that adoption inside an
//! outer repository does not disturb what the outer repository already had staged, and
//! that a clone of a created campaign can save into a directory it never held.
//!
//! Every test here skips itself when `git` is not installed, so `cargo test -p campaign`
//! still passes with nothing on `PATH`. Set `PNP_REQUIRE_GIT=1` to turn the skip into a
//! failure — without it a machine with no `git` reports success having checked none of
//! this, which is exactly how a broken invocation would reach a release.

use std::ffi::OsString;
use std::path::Path;
use std::process::Output;

use campaign::repo::{GitError, GitRunner, Repo, SystemGit, git_is_installed};

fn git_or_skip(test: &str) -> bool {
    if git_is_installed() {
        return true;
    }
    assert!(
        std::env::var_os("PNP_REQUIRE_GIT").is_none(),
        "PNP_REQUIRE_GIT is set but git is not installed, so {test} could not run"
    );
    eprintln!("skipping {test}: git is not on PATH");
    false
}

struct WithIdentity;

impl GitRunner for WithIdentity {
    fn run(&self, dir: &Path, args: &[OsString]) -> Result<Output, GitError> {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "pnp tests")
            .env("GIT_AUTHOR_EMAIL", "pnp-tests@example.invalid")
            .env("GIT_COMMITTER_NAME", "pnp tests")
            .env("GIT_COMMITTER_EMAIL", "pnp-tests@example.invalid")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    GitError::GitMissing
                } else {
                    GitError::GitRefused {
                        verb: "run",
                        message: error.to_string(),
                    }
                }
            })
    }
}

fn git(dir: &Path, args: &[&str]) -> Output {
    SystemGit
        .run(
            dir,
            &args.iter().map(OsString::from).collect::<Vec<_>>(),
        )
        .expect("git is on PATH, checked by git_or_skip")
}

// The first acceptance criterion: after `begin`, the directory is clean and its log
// holds exactly the one birth commit.
#[test]
fn begin_leaves_a_clean_repo_with_one_commit() {
    if !git_or_skip("begin_leaves_a_clean_repo_with_one_commit") {
        return;
    }
    let tmp = tempfile::tempdir().expect("temp dir");
    std::fs::write(tmp.path().join("world.ron"), "()").expect("a file to commit");

    Repo::at(tmp.path())
        .begin(&WithIdentity)
        .expect("begin with a real git");

    let status = git(tmp.path(), &["status", "--porcelain"]);
    assert!(status.status.success());
    assert!(status.stdout.is_empty(), "{}", String::from_utf8_lossy(&status.stdout));

    let log = git(tmp.path(), &["log", "--oneline"]);
    let lines = String::from_utf8_lossy(&log.stdout);
    assert_eq!(lines.lines().count(), 1, "{lines}");
}

// A root already inside an outer repository is adopted: no nested `.git`, the campaign
// is committed, and a file the outer repository had staged elsewhere is left alone.
#[test]
fn begin_inside_an_outer_repository_adopts_it() {
    if !git_or_skip("begin_inside_an_outer_repository_adopts_it") {
        return;
    }
    let tmp = tempfile::tempdir().expect("temp dir");
    let outer = tmp.path();
    git(outer, &["init"]);
    std::fs::write(outer.join("outer-file.txt"), "the GM's own work").expect("write");
    git(outer, &["add", "--", "outer-file.txt"]);

    let campaign_root = outer.join("my-campaign");
    std::fs::create_dir_all(&campaign_root).expect("campaign dir");
    std::fs::write(campaign_root.join("world.ron"), "()").expect("a file to commit");

    Repo::at(&campaign_root)
        .begin(&WithIdentity)
        .expect("begin adopting the outer repository");

    assert!(
        !campaign_root.join(".git").exists(),
        "a nested repository was created"
    );

    let staged = git(outer, &["diff", "--cached", "--name-only"]);
    let staged_names = String::from_utf8_lossy(&staged.stdout);
    assert!(
        staged_names.lines().any(|line| line == "outer-file.txt"),
        "the outer repository's own staged file was disturbed: {staged_names}"
    );

    let log = git(&campaign_root, &["log", "--oneline", "--", "world.ron"]);
    assert!(
        !String::from_utf8_lossy(&log.stdout).trim().is_empty(),
        "the campaign's own file was not committed"
    );
}

// The third acceptance criterion, minus the painting: a clone of a created campaign
// opens, and a save into its absent `dungeons/` writes the file.
#[test]
fn a_clone_saves_into_a_directory_it_never_held() {
    if !git_or_skip("a_clone_saves_into_a_directory_it_never_held") {
        return;
    }
    let tmp = tempfile::tempdir().expect("temp dir");
    let origin = tmp.path().join("origin");
    std::fs::create_dir_all(&origin).expect("origin dir");
    std::fs::write(origin.join("world.ron"), "()").expect("a file to commit");
    Repo::at(&origin).begin(&WithIdentity).expect("begin");

    let clone = tmp.path().join("clone");
    let result = git(
        tmp.path(),
        &["clone", "--quiet", origin.to_str().unwrap(), clone.to_str().unwrap()],
    );
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert!(!clone.join("dungeons").exists(), "the clone must not carry an empty directory");

    let world = campaign::World::default();
    let dungeon_path = clone.join("dungeons").join("crypt.ron");
    world.save(&dungeon_path).expect("save into the absent directory");

    assert!(dungeon_path.is_file());
}
