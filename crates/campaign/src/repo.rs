//! Everything this workspace does with git: the one place a `git` process is run, and
//! what makes a fresh campaign directory a repository from birth.
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

    /// From every call: `git` ran and refused. The message is the first line of what it
    /// said for itself, for the reason [`crate::notebook::NoteError::ZkRefused`] gives.
    #[error("`{GIT} {verb}` failed: {message}")]
    GitRefused { verb: &'static str, message: String },

    /// From `begin`: the ignore file could not be written.
    #[error("`{}` could not be written: {source}", .path.display())]
    Unwritable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
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

/// A campaign directory, and the one thing done to it through git: becoming, or being
/// adopted as, a repository.
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
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("it said nothing")
        .to_owned()
}
