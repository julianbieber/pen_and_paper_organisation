//! The handful of things only the real `git` can answer: that a created campaign is a
//! repository with nothing to commit and one commit in its log, that adoption inside an
//! outer repository does not disturb what the outer repository already had staged, that
//! a clone of a created campaign can save into a directory it never held, the four
//! acceptance shapes a *Sync* must produce — a plain commit, a first push, a pull that
//! brings a change in, and a conflict that leaves the working tree at the wip commit —
//! and [`Campaign::clone_from`]'s two failure shapes: a real clone that lands but holds
//! no manifest, and a real `git clone` refusing outright.
//!
//! Every test here skips itself when `git` is not installed, so `cargo test -p campaign`
//! still passes with nothing on `PATH`. Set `PNP_REQUIRE_GIT=1` to turn the skip into a
//! failure — without it a machine with no `git` reports success having checked none of
//! this, which is exactly how a broken invocation would reach a release.

use std::ffi::OsString;
use std::path::Path;
use std::process::Output;

use campaign::repo::{GitError, GitRunner, RemoteOutcome, Repo, SystemGit, git_is_installed};
use campaign::{Campaign, CampaignError};

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

// The first acceptance shape: with no remote, `sync` commits and says so, and syncing
// again with nothing dirty commits nothing more.
#[test]
fn sync_with_no_remote_commits_once_then_nothing_more() {
    if !git_or_skip("sync_with_no_remote_commits_once_then_nothing_more") {
        return;
    }
    let tmp = tempfile::tempdir().expect("temp dir");
    std::fs::write(tmp.path().join("world.ron"), "()").expect("a file to commit");
    Repo::at(tmp.path()).begin(&WithIdentity).expect("begin");

    std::fs::write(tmp.path().join("world.ron"), "(features: {})").expect("dirty the file");
    let synced = Repo::at(tmp.path()).sync(&WithIdentity).expect("sync");
    assert!(synced.commit.is_some());
    assert_eq!(synced.remote, RemoteOutcome::NoRemote);
    assert_eq!(oneline_log(tmp.path()).len(), 2);

    let synced_again = Repo::at(tmp.path()).sync(&WithIdentity).expect("sync again");
    assert_eq!(synced_again.commit, None);
    assert_eq!(oneline_log(tmp.path()).len(), 2);
}

// The second acceptance shape: a remote that exists but has no branch yet takes the
// exit-2 path straight to a push.
#[test]
fn sync_pushes_to_a_freshly_set_empty_remote() {
    if !git_or_skip("sync_pushes_to_a_freshly_set_empty_remote") {
        return;
    }
    let tmp = tempfile::tempdir().expect("temp dir");
    let campaign_root = tmp.path().join("campaign");
    std::fs::create_dir_all(&campaign_root).expect("campaign dir");
    std::fs::write(campaign_root.join("world.ron"), "()").expect("a file to commit");
    Repo::at(&campaign_root).begin(&WithIdentity).expect("begin");

    let origin = tmp.path().join("origin.git");
    git(tmp.path(), &["init", "--bare", "--quiet", origin.to_str().unwrap()]);
    Repo::at(&campaign_root)
        .set_origin(&WithIdentity, origin.to_str().unwrap())
        .expect("set origin");

    let synced = Repo::at(&campaign_root).sync(&WithIdentity).expect("sync");
    assert_eq!(synced.commit, None, "nothing was dirty after begin");
    assert_eq!(synced.remote, RemoteOutcome::Pushed);

    let branch = current_branch(&campaign_root);
    let remote_branches = git(&campaign_root, &["ls-remote", "--heads", "origin"]);
    let listed = String::from_utf8_lossy(&remote_branches.stdout);
    assert!(
        listed.contains(&format!("refs/heads/{branch}")),
        "{listed}"
    );
}

// The third acceptance shape: A pushes a change, B syncs and gets it without restarting —
// `changed` names the file the pull brought in.
#[test]
fn syncing_b_after_a_pulls_in_as_own_change() {
    if !git_or_skip("syncing_b_after_a_pulls_in_as_own_change") {
        return;
    }
    let (_tmp, a, b) = two_clones_of_a_pushed_origin("syncing_b_after_a_pulls_in_as_own_change");

    std::fs::write(a.join("world.ron"), "(from: \"a\")").expect("a's change");
    let a_synced = Repo::at(&a).sync(&WithIdentity).expect("a syncs");
    assert_eq!(a_synced.remote, RemoteOutcome::Pushed);

    let b_synced = Repo::at(&b).sync(&WithIdentity).expect("b syncs");
    assert_eq!(b_synced.remote, RemoteOutcome::Pushed);
    assert_eq!(b_synced.changed, vec![std::path::PathBuf::from("world.ron")]);
    assert_eq!(
        std::fs::read_to_string(b.join("world.ron")).expect("read"),
        "(from: \"a\")"
    );
}

// The fourth acceptance shape: both sides edit the same line and sync. B's rebase
// conflicts, is aborted, and B's working tree is left exactly at the wip commit it made —
// no rebase left in progress, B's own bytes on disk.
#[test]
fn a_conflicting_sync_aborts_and_leaves_the_wip_commit() {
    if !git_or_skip("a_conflicting_sync_aborts_and_leaves_the_wip_commit") {
        return;
    }
    let (_tmp, a, b) = two_clones_of_a_pushed_origin("a_conflicting_sync_aborts_and_leaves_the_wip_commit");

    std::fs::write(a.join("world.ron"), "(from: \"a\")").expect("a's change");
    std::fs::write(b.join("world.ron"), "(from: \"b\")").expect("b's change");

    let a_synced = Repo::at(&a).sync(&WithIdentity).expect("a syncs");
    assert_eq!(a_synced.remote, RemoteOutcome::Pushed);

    let b_synced = Repo::at(&b).sync(&WithIdentity).expect("b syncs");
    assert!(
        matches!(b_synced.remote, RemoteOutcome::Conflict { .. }),
        "{:?}",
        b_synced.remote
    );

    let subject = git(&b, &["log", "-1", "--format=%s"]);
    assert!(
        String::from_utf8_lossy(&subject.stdout).starts_with("Sync "),
        "{}",
        String::from_utf8_lossy(&subject.stdout)
    );
    assert_eq!(
        std::fs::read_to_string(b.join("world.ron")).expect("read"),
        "(from: \"b\")"
    );

    let status = git(&b, &["status", "--porcelain=v1"]);
    assert!(
        !String::from_utf8_lossy(&status.stdout).contains("rebase in progress"),
        "{}",
        String::from_utf8_lossy(&status.stdout)
    );
    let rebase_merge = git(&b, &["rev-parse", "--git-path", "rebase-merge"]);
    let path = String::from_utf8_lossy(&rebase_merge.stdout).trim().to_owned();
    assert!(!b.join(&path).exists(), "a rebase is still in progress at {path}");
}

// A real clone of a repository with no `campaign.ron` fails `ClonedNotACampaign`, and the
// clone — `.git` included — is left where it landed for the GM to look at.
#[test]
fn clone_from_a_repository_with_no_manifest_fails_and_leaves_the_git_dir() {
    if !git_or_skip("clone_from_a_repository_with_no_manifest_fails_and_leaves_the_git_dir") {
        return;
    }
    let tmp = tempfile::tempdir().expect("temp dir");
    let origin = tmp.path().join("origin");
    std::fs::create_dir_all(&origin).expect("origin dir");
    std::fs::write(origin.join("README"), "not a campaign").expect("a file to commit");
    Repo::at(&origin).begin(&WithIdentity).expect("begin");

    let parent = tmp.path().join("clone-parent");
    let origin_url = origin.to_str().unwrap().to_owned();

    match Campaign::clone_from(&parent, &origin_url, &WithIdentity) {
        Err(CampaignError::ClonedNotACampaign(reported)) => {
            assert_eq!(reported, parent.join("origin"));
        }
        other => panic!("expected ClonedNotACampaign, got {other:?}"),
    }
    assert!(parent.join("origin/.git").is_dir());
}

// A clone of a path that does not exist fails with git's own last line, which for a
// missing source starts with `fatal:` — and git cleans up its own failed clone, so no
// root is left behind.
#[test]
fn clone_from_a_path_that_does_not_exist_fails_and_leaves_no_root() {
    if !git_or_skip("clone_from_a_path_that_does_not_exist_fails_and_leaves_no_root") {
        return;
    }
    let tmp = tempfile::tempdir().expect("temp dir");
    let parent = tmp.path().join("clone-parent");
    let missing = tmp.path().join("does-not-exist");
    let missing_url = missing.to_str().unwrap().to_owned();

    match Campaign::clone_from(&parent, &missing_url, &WithIdentity) {
        Err(CampaignError::CloneFailed(GitError::GitRefused { verb: "clone", message })) => {
            assert!(message.starts_with("fatal:"), "{message}");
        }
        other => panic!("expected CloneFailed(GitRefused), got {other:?}"),
    }
    assert!(!parent.join("does-not-exist").exists());
}

fn two_clones_of_a_pushed_origin(name: &str) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let campaign_root = tmp.path().join("campaign");
    std::fs::create_dir_all(&campaign_root).expect("campaign dir");
    std::fs::write(campaign_root.join("world.ron"), "()").expect("a file to commit");
    Repo::at(&campaign_root).begin(&WithIdentity).expect("begin");

    let origin = tmp.path().join("origin.git");
    git(tmp.path(), &["init", "--bare", "--quiet", origin.to_str().unwrap()]);
    Repo::at(&campaign_root)
        .set_origin(&WithIdentity, origin.to_str().unwrap())
        .expect("set origin");
    let first_push = Repo::at(&campaign_root).sync(&WithIdentity).expect(name);
    assert_eq!(first_push.remote, RemoteOutcome::Pushed);

    let branch = current_branch(&campaign_root);
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    let clone = |dest: &std::path::Path| {
        let result = git(
            tmp.path(),
            &[
                "clone",
                "--quiet",
                "--branch",
                &branch,
                origin.to_str().unwrap(),
                dest.to_str().unwrap(),
            ],
        );
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    };
    clone(&a);
    clone(&b);

    (tmp, a, b)
}

fn current_branch(dir: &Path) -> String {
    let output = git(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn oneline_log(dir: &Path) -> Vec<String> {
    let log = git(dir, &["log", "--oneline"]);
    String::from_utf8_lossy(&log.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}
