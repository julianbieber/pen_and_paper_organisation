//! Everything this workspace does with git: the one place a `git` process is run, what
//! makes a fresh campaign directory a repository from birth, what a *Sync* commits,
//! pulls and pushes, and cloning one from a remote.
//!
//! Shelled out to exactly as `zk` is in [`crate::notebook`]: the argument vectors are
//! pure functions tested against a recorded runner, and the handful of tests that want
//! the real program skip themselves unless it is installed.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

/// The program every invocation here runs. Not configurable: there is only one git.
pub const GIT: &str = "git";

/// The name of a campaign's ignore file.
pub const IGNORE_FILE: &str = ".gitignore";

/// What the ignore file says.
///
/// `zk list` rewrites the notebook index on every reference query, and every save
/// stages through a `.tmp` sibling — both are churn a committed campaign would
/// otherwise make for itself.
pub const IGNORE: &str = "notes/.zk/notebook.db*\n*.tmp\n";

/// The message the birth commit carries.
pub const FIRST_COMMIT: &str = "Create the campaign";

/// Why a campaign directory could not be made, or kept, a git repository.
///
/// Never propagated out of [`crate::campaign::Campaign::create`] — a git failure costs
/// the GM the repository, never the campaign — so every variant names what a status
/// line can say about it.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GitError {
    /// From every call: `git` is not installed, or not on PATH.
    #[error("`{GIT}` is not installed, or not on PATH, so the campaign is not a git repository")]
    GitMissing,

    /// From every call, including [`Repo::clone_into`]: `git` ran and refused. The
    /// message is the first line of what it said for itself, for the reason
    /// [`crate::notebook::NoteError::ZkRefused`] gives — except a clone's, which is the
    /// *last* line, since `git clone`'s own refusal comes after its `Cloning into '…'`
    /// announcement.
    #[error("`{GIT} {verb}` failed: {message}")]
    GitRefused { verb: &'static str, message: String },

    /// From `begin`: the ignore file could not be written.
    #[error("`{}` could not be written: {source}", .path.display())]
    Unwritable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// From `sync`: the root is in a detached `HEAD`, so there is no branch to commit to.
    #[error("the campaign is not on a branch, so there is nothing to sync")]
    NotOnABranch,
}

/// How a `git` invocation is actually run.
///
/// Exists so the argument vectors above can be checked without the program installed:
/// tests supply a runner that records what it was asked to run and hands back a chosen
/// answer. [`SystemGit`] is the one that runs anything.
pub trait GitRunner {
    /// Run `GIT` with `args`, from inside `dir`, and hand back what it said.
    ///
    /// Returns [`GitError::GitMissing`] when the program is not there, and `Ok` for a
    /// program that ran and failed — the exit status is in the output, and classifying
    /// it is the caller's.
    fn run(&self, dir: &Path, args: &[OsString]) -> Result<Output, GitError>;
}

/// The runner that runs the real program.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemGit;

impl GitRunner for SystemGit {
    fn run(&self, dir: &Path, args: &[OsString]) -> Result<Output, GitError> {
        std::process::Command::new(GIT)
            .args(args)
            .current_dir(dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .output()
            .map_err(missing_or)
    }
}

fn missing_or(error: std::io::Error) -> GitError {
    if error.kind() == std::io::ErrorKind::NotFound {
        GitError::GitMissing
    } else {
        GitError::GitRefused {
            verb: "run",
            message: error.to_string(),
        }
    }
}

/// Whether `git` answers at all.
pub fn git_is_installed() -> bool {
    SystemGit
        .run(Path::new("."), &[OsString::from("--version")])
        .is_ok_and(|output| output.status.success())
}

/// The arguments that make the current directory a repository.
pub fn init_args() -> Vec<OsString> {
    vec![OsString::from("init")]
}

/// The arguments that ask whether the current directory is already inside a work tree.
pub fn inside_work_tree_args() -> Vec<OsString> {
    vec![
        OsString::from("rev-parse"),
        OsString::from("--is-inside-work-tree"),
    ]
}

/// The arguments that stage everything under the working directory, and nothing
/// outside it.
///
/// The `-- .` pathspec is what keeps an adopted campaign from staging the rest of the
/// GM's own repository — verified against git 2.43, a file the outer repository had
/// staged elsewhere stays staged and untouched.
pub fn add_args() -> Vec<OsString> {
    vec![
        OsString::from("add"),
        OsString::from("--all"),
        OsString::from("--"),
        OsString::from("."),
    ]
}

/// The arguments that commit everything staged under the working directory.
///
/// `message` is joined to its flag for the reason
/// [`new_args`](crate::notebook::new_args) gives: a following word beginning with a
/// dash would otherwise be read as another option. The `-- .` pathspec matches
/// [`add_args`], for the same reason it is there.
pub fn commit_args(message: &str) -> Vec<OsString> {
    vec![
        OsString::from("commit"),
        OsString::from(format!("--message={message}")),
        OsString::from("--"),
        OsString::from("."),
    ]
}

/// The subject every sync commit's message starts with.
pub const SYNC_PREFIX: &str = "Sync";

/// The arguments that name the branch checked out at the root, failing when it is a
/// detached `HEAD`.
pub fn current_branch_args() -> Vec<OsString> {
    vec![
        OsString::from("symbolic-ref"),
        OsString::from("--quiet"),
        OsString::from("--short"),
        OsString::from("HEAD"),
    ]
}

/// The arguments that ask whether anything is staged under the working directory: exit
/// 0 for nothing, 1 for something, anything else a refusal.
pub fn staged_anything_args() -> Vec<OsString> {
    vec![
        OsString::from("diff"),
        OsString::from("--cached"),
        OsString::from("--quiet"),
        OsString::from("--"),
        OsString::from("."),
    ]
}

/// The arguments that print the identity `git` would stamp a commit's committer line
/// with, ending in an epoch timestamp and a `+HHMM`/`-HHMM` offset — what [`stamp_of_ident`]
/// reads, so a sync's subject needs no clock of its own.
pub fn committer_ident_args() -> Vec<OsString> {
    vec![OsString::from("var"), OsString::from("GIT_COMMITTER_IDENT")]
}

/// The arguments that print `origin`'s URL, failing when there is none.
pub fn origin_url_args() -> Vec<OsString> {
    vec![
        OsString::from("remote"),
        OsString::from("get-url"),
        OsString::from("origin"),
    ]
}

/// The arguments that ask whether `origin` already carries `branch`: exit 0 when it
/// does, exit 2 when it does not — a fresh bare repository, say.
pub fn remote_has_branch_args(branch: &str) -> Vec<OsString> {
    vec![
        OsString::from("ls-remote"),
        OsString::from("--exit-code"),
        OsString::from("--heads"),
        OsString::from("origin"),
        OsString::from(format!("refs/heads/{branch}")),
    ]
}

/// The arguments that print the commit `HEAD` names.
pub fn head_args() -> Vec<OsString> {
    vec![OsString::from("rev-parse"), OsString::from("HEAD")]
}

/// The arguments that rebase the branch checked out at the root onto `origin/<branch>`,
/// fetching first. `origin <branch>` are explicit rather than a bare `pull --rebase`,
/// because a branch whose remote was just set has no upstream configured yet.
pub fn pull_rebase_args(branch: &str) -> Vec<OsString> {
    vec![
        OsString::from("pull"),
        OsString::from("--rebase"),
        OsString::from("origin"),
        OsString::from(branch),
    ]
}

/// The arguments that abort a rebase in progress, failing when there is none.
pub fn abort_rebase_args() -> Vec<OsString> {
    vec![OsString::from("rebase"), OsString::from("--abort")]
}

/// The arguments that list the paths that differ between `before` and `after`.
///
/// `--relative` makes every path relative to the working directory, which is what keeps
/// an adopted campaign's [`Synced::changed`] from naming a file outside it.
pub fn changed_between_args(before: &str, after: &str) -> Vec<OsString> {
    vec![
        OsString::from("diff"),
        OsString::from("--name-only"),
        OsString::from("--relative"),
        OsString::from(before),
        OsString::from(after),
    ]
}

/// The arguments that push the branch checked out at the root to `origin`, setting it as
/// the upstream — which is what fixes the upstream on a branch whose remote was just set.
pub fn push_args(branch: &str) -> Vec<OsString> {
    vec![
        OsString::from("push"),
        OsString::from("--set-upstream"),
        OsString::from("origin"),
        OsString::from(branch),
    ]
}

/// The arguments that add `url` as `origin`.
///
/// `--` before it, as every value from outside this crate is joined to its flag or
/// preceded by `--` before being passed to git — see [`commit_args`].
pub fn add_origin_args(url: &str) -> Vec<OsString> {
    vec![
        OsString::from("remote"),
        OsString::from("add"),
        OsString::from("--"),
        OsString::from("origin"),
        OsString::from(url),
    ]
}

/// Why `url` may not be set as a campaign's remote, or `None` when it may.
pub fn remote_refusal(url: &str) -> Option<&'static str> {
    if url.trim().is_empty() {
        return Some("a remote needs a URL");
    }
    if url.contains('\n') || url.contains('\0') {
        return Some("a remote URL may not contain a newline or a NUL");
    }
    None
}

/// The arguments that clone `url` into `destination`.
///
/// The `--` before `url` is there for the reason [`add_origin_args`] carries one.
pub fn clone_args(url: &str, destination: &Path) -> Vec<OsString> {
    vec![
        OsString::from("clone"),
        OsString::from("--quiet"),
        OsString::from("--"),
        OsString::from(url),
        destination.as_os_str().to_owned(),
    ]
}

/// The directory name `git clone url` would make, or `None` when [`crate::feature::file_name_refusal`]
/// refuses it.
///
/// Trims `url`, strips trailing `/`s, strips one trailing `.git`, strips trailing `/`s
/// again (so `host/repo/.git` names `repo`), then takes what follows the last `/` or
/// `:` — the `:` is what an scp-style remote (`git@host:user/repo.git`) separates its
/// path on. The name comes back exactly as the repository has it, not slugged: a clone
/// lands at `<where>/<repository name>`, the same name a clone made by hand elsewhere
/// would use.
pub fn repository_name(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let trimmed = trimmed.trim_end_matches('/');
    let name = match trimmed.rfind(['/', ':']) {
        Some(index) => &trimmed[index + 1..],
        None => trimmed,
    };
    if crate::feature::file_name_refusal(name, 255).is_some() {
        return None;
    }
    Some(name.to_owned())
}

/// The epoch seconds and the UTC offset in minutes a `GIT_COMMITTER_IDENT` line ends in,
/// or `None` when its last two words are not a timestamp and an offset.
///
/// Reads `Name <mail> 1757680000 +0200` from the back, so a name or a mail address
/// carrying spaces of its own cannot throw the parse off.
pub fn stamp_of_ident(ident: &str) -> Option<(i64, i32)> {
    let mut words = ident.split_whitespace().rev();
    let offset_word = words.next()?;
    let seconds_word = words.next()?;
    let epoch_seconds: i64 = seconds_word.parse().ok()?;
    let offset_minutes = offset_minutes_of(offset_word)?;
    Some((epoch_seconds, offset_minutes))
}

fn offset_minutes_of(word: &str) -> Option<i32> {
    if word.len() != 5 {
        return None;
    }
    let sign = match word.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hours: i32 = word.get(1..3)?.parse().ok()?;
    let minutes: i32 = word.get(3..5)?.parse().ok()?;
    Some(sign * (hours * 60 + minutes))
}

/// The subject a sync commit made at `epoch_seconds` (as [`stamp_of_ident`] reads it)
/// carries, in the local time `offset_minutes` names: `Sync YYYY-MM-DD HH:MM`.
pub fn sync_subject(epoch_seconds: i64, offset_minutes: i32) -> String {
    let local = epoch_seconds + i64::from(offset_minutes) * 60;
    let days = local.div_euclid(86_400);
    let seconds_of_day = local.rem_euclid(86_400);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let (year, month, day) = civil_from_days(days);
    format!("{SYNC_PREFIX} {year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// What happened when a sync tried to reach `origin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteOutcome {
    /// The campaign has no remote at all.
    NoRemote,
    /// The pull, when there was one to do, and the push both succeeded.
    Pushed,
    /// The pull conflicted; the rebase was aborted and the local commit stays.
    Conflict { message: String },
    /// A remote step other than a conflicting pull failed.
    Refused { verb: &'static str, message: String },
}

/// What one call to [`Repo::sync`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Synced {
    /// The subject of the commit made, or `None` when nothing was staged.
    pub commit: Option<String>,
    pub remote: RemoteOutcome,
    /// The paths, relative to the root, a successful pull brought in.
    pub changed: Vec<PathBuf>,
}

impl Synced {
    /// One line saying what this sync did, distinct for every case above.
    pub fn summary(&self) -> String {
        let dirty = self.commit.is_some();
        let committed = match &self.commit {
            Some(subject) => format!("committed `{subject}`"),
            None => "nothing to commit".to_owned(),
        };
        let join = if dirty { ',' } else { ';' };

        match &self.remote {
            RemoteOutcome::NoRemote => {
                format!("{committed}{join} not pushed: the campaign has no remote")
            }
            RemoteOutcome::Pushed => {
                let joiner = if dirty { " and" } else { ";" };
                let mut summary = format!("{committed}{joiner} synced with origin");
                if !self.changed.is_empty() {
                    summary.push_str(&format!(", {} file(s) came in", self.changed.len()));
                }
                summary
            }
            RemoteOutcome::Conflict { message } => format!(
                "the pull from origin conflicted and was aborted; the local commit stays and nothing was pushed: {message}"
            ),
            RemoteOutcome::Refused { verb, message } => {
                format!("{committed}{join} but `{GIT} {verb}` failed: {message}")
            }
        }
    }

    /// Whether a successful pull brought `path` (relative to `root`, or already relative)
    /// in.
    pub fn touches(&self, root: &Path, path: &Path) -> bool {
        let relative = path.strip_prefix(root).unwrap_or(path);
        self.changed.iter().any(|changed| changed == relative)
    }
}

/// A campaign directory, and the one thing done to it through git: becoming, or being
/// adopted as, a repository, and syncing with a remote.
pub struct Repo {
    root: PathBuf,
}

impl Repo {
    /// The repository rooted at `root`.
    pub fn at(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_owned(),
        }
    }

    /// The directory this repository is, or would be, rooted at.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether this root is already inside a git work tree.
    ///
    /// A non-zero exit is `false`, not a refusal: `rev-parse` answers the question by
    /// failing when there is no work tree here at all.
    pub fn is_inside_work_tree(&self, runner: &impl GitRunner) -> Result<bool, GitError> {
        let output = runner.run(&self.root, &inside_work_tree_args())?;
        Ok(output.status.success())
    }

    /// Make this root a git repository with one commit in it, or adopt it into the
    /// work tree it is already inside.
    ///
    /// Runs, in order: write the ignore file (`create_new`, so a `.gitignore` the GM
    /// already wrote is never overwritten), `init` unless the root is already inside a
    /// work tree, `add`, `commit`. A missing `git` or a refusal leaves the ignore file,
    /// where it could be written, and nothing else — the campaign this belongs to stays
    /// openable either way.
    pub fn begin(&self, runner: &impl GitRunner) -> Result<(), GitError> {
        self.write_ignore()?;

        if !self.is_inside_work_tree(runner)? {
            let output = runner.run(&self.root, &init_args())?;
            refused(&output, "init")?;
        }

        let output = runner.run(&self.root, &add_args())?;
        refused(&output, "add")?;

        let output = runner.run(&self.root, &commit_args(FIRST_COMMIT))?;
        refused(&output, "commit")?;

        Ok(())
    }

    fn write_ignore(&self) -> Result<(), GitError> {
        let path = self.root.join(IGNORE_FILE);
        match std::fs::File::create_new(&path) {
            Ok(mut file) => file
                .write_all(IGNORE.as_bytes())
                .map_err(|source| GitError::Unwritable { path, source }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(source) => Err(GitError::Unwritable { path, source }),
        }
    }

    /// `origin`'s URL, or `None` when there is none.
    pub fn origin(&self, runner: &impl GitRunner) -> Result<Option<String>, GitError> {
        let output = runner.run(&self.root, &origin_url_args())?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(&output.stdout).trim().to_owned()))
    }

    /// Clone `url` into `destination`, running `clone` from `destination`'s parent (or
    /// `.` when there is none).
    ///
    /// Refuses [`remote_refusal`] before running anything, as [`GitError::GitRefused`]
    /// naming `clone`. A failed clone's message is git's own **last** stderr line, not
    /// its first — `git clone` prints `Cloning into '…'` before the `fatal:` line, so the
    /// first line would only repeat the destination the caller already typed. Not named
    /// `clone`, so it cannot be misread as [`Clone::clone`].
    pub fn clone_into(
        runner: &impl GitRunner,
        url: &str,
        destination: &Path,
    ) -> Result<Self, GitError> {
        if let Some(reason) = remote_refusal(url) {
            return Err(GitError::GitRefused {
                verb: "clone",
                message: reason.to_owned(),
            });
        }
        let dir = destination.parent().filter(|parent| !parent.as_os_str().is_empty());
        let output = runner.run(dir.unwrap_or_else(|| Path::new(".")), &clone_args(url, destination))?;
        if !output.status.success() {
            return Err(GitError::GitRefused {
                verb: "clone",
                message: last_line(&output.stderr),
            });
        }
        Ok(Self::at(destination))
    }

    /// Set `origin` to `url`.
    ///
    /// Refuses [`remote_refusal`] before running anything, as [`GitError::GitRefused`]
    /// naming `remote` — the caller's job is to have checked there is no `origin` yet, the
    /// issue's "sets `origin` when there is none".
    pub fn set_origin(&self, runner: &impl GitRunner, url: &str) -> Result<(), GitError> {
        if let Some(reason) = remote_refusal(url) {
            return Err(GitError::GitRefused {
                verb: "remote",
                message: reason.to_owned(),
            });
        }
        let output = runner.run(&self.root, &add_origin_args(url))?;
        refused(&output, "remote")
    }

    /// Save everything dirty, pull and push against `origin` when there is one, in the
    /// shape of `hwt sync`: commit what is dirty, rebase onto the upstream, push, and
    /// abort rather than resolve a conflict.
    ///
    /// Only a missing `git`, a detached `HEAD`, and a refusal of `add`, `diff`, `var` or
    /// `commit` are `Err` — every failure from here on (no remote, a conflicting pull, a
    /// refused push) is an `Ok` [`Synced`] outcome, so a caller can always say whether a
    /// commit was made even when the remote half went wrong.
    pub fn sync(&self, runner: &impl GitRunner) -> Result<Synced, GitError> {
        let branch_output = runner.run(&self.root, &current_branch_args())?;
        if !branch_output.status.success() {
            return Err(GitError::NotOnABranch);
        }
        let branch = String::from_utf8_lossy(&branch_output.stdout).trim().to_owned();

        let add_output = runner.run(&self.root, &add_args())?;
        refused(&add_output, "add")?;

        let staged_output = runner.run(&self.root, &staged_anything_args())?;
        let commit = match staged_output.status.code() {
            Some(0) => None,
            Some(1) => Some(self.commit_sync(runner)?),
            _ => {
                return Err(GitError::GitRefused {
                    verb: "diff",
                    message: first_line(&staged_output.stderr),
                });
            }
        };

        let origin_output = runner.run(&self.root, &origin_url_args())?;
        if !origin_output.status.success() {
            return Ok(Synced {
                commit,
                remote: RemoteOutcome::NoRemote,
                changed: Vec::new(),
            });
        }

        let has_branch_output = runner.run(&self.root, &remote_has_branch_args(&branch))?;
        let has_branch = match has_branch_output.status.code() {
            Some(0) => true,
            Some(2) => false,
            _ => {
                return Ok(Synced {
                    commit,
                    remote: RemoteOutcome::Refused {
                        verb: "ls-remote",
                        message: first_line(&has_branch_output.stderr),
                    },
                    changed: Vec::new(),
                });
            }
        };

        let mut changed = Vec::new();
        if has_branch {
            match self.pull(runner, &branch)? {
                Ok(brought_in) => changed = brought_in,
                Err(outcome) => {
                    return Ok(Synced {
                        commit,
                        remote: outcome,
                        changed: Vec::new(),
                    });
                }
            }
        }

        let push_output = runner.run(&self.root, &push_args(&branch))?;
        if !push_output.status.success() {
            return Ok(Synced {
                commit,
                remote: RemoteOutcome::Refused {
                    verb: "push",
                    message: first_line(&push_output.stderr),
                },
                changed,
            });
        }

        Ok(Synced {
            commit,
            remote: RemoteOutcome::Pushed,
            changed,
        })
    }

    fn commit_sync(&self, runner: &impl GitRunner) -> Result<String, GitError> {
        let ident_output = runner.run(&self.root, &committer_ident_args())?;
        refused(&ident_output, "var")?;
        let ident = String::from_utf8_lossy(&ident_output.stdout);
        let (epoch_seconds, offset_minutes) =
            stamp_of_ident(ident.trim()).ok_or_else(|| GitError::GitRefused {
                verb: "var",
                message: "GIT_COMMITTER_IDENT did not end in a timestamp and an offset".to_owned(),
            })?;
        let subject = sync_subject(epoch_seconds, offset_minutes);

        let commit_output = runner.run(&self.root, &commit_args(&subject))?;
        refused(&commit_output, "commit")?;
        Ok(subject)
    }

    fn pull(
        &self,
        runner: &impl GitRunner,
        branch: &str,
    ) -> Result<Result<Vec<PathBuf>, RemoteOutcome>, GitError> {
        let before_output = runner.run(&self.root, &head_args())?;
        let before = String::from_utf8_lossy(&before_output.stdout).trim().to_owned();

        let pull_output = runner.run(&self.root, &pull_rebase_args(branch))?;
        if !pull_output.status.success() {
            let abort_output = runner.run(&self.root, &abort_rebase_args())?;
            return Ok(Err(if abort_output.status.success() {
                RemoteOutcome::Conflict {
                    message: message_of(&pull_output),
                }
            } else {
                RemoteOutcome::Refused {
                    verb: "pull",
                    message: first_line(&pull_output.stderr),
                }
            }));
        }

        let after_output = runner.run(&self.root, &head_args())?;
        let after = String::from_utf8_lossy(&after_output.stdout).trim().to_owned();
        if !after_output.status.success() || after == before {
            return Ok(Ok(Vec::new()));
        }

        let diff_output = runner.run(&self.root, &changed_between_args(&before, &after))?;
        if !diff_output.status.success() {
            return Ok(Ok(Vec::new()));
        }
        Ok(Ok(String::from_utf8_lossy(&diff_output.stdout)
            .lines()
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect()))
    }
}

fn refused(output: &Output, verb: &'static str) -> Result<(), GitError> {
    if output.status.success() {
        return Ok(());
    }
    Err(GitError::GitRefused {
        verb,
        message: first_line(&output.stderr),
    })
}

fn first_line(stderr: &[u8]) -> String {
    first_nonempty_line(stderr).unwrap_or_else(|| "it said nothing".to_owned())
}

fn last_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| "it said nothing".to_owned())
}

fn message_of(output: &Output) -> String {
    first_nonempty_line(&output.stderr)
        .or_else(|| first_nonempty_line(&output.stdout))
        .unwrap_or_else(|| "it said nothing".to_owned())
}

fn first_nonempty_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}
