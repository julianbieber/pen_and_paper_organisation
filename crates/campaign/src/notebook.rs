//! The campaign's notebook: what makes the notes directory one, what a note of each
//! kind is made from, and the one place this workspace runs `zk`.
//!
//! `zk` is the authority on the notebook, its templates and its filenames. Nothing here
//! reads a note's markdown, parses one, or writes one — a second implementation would be
//! a second answer to what the notebook contains. What this module owns is the argument
//! vector, and it owns it as a *value*: [`new_args`], [`init_args`] and [`edit_args`] are
//! pure functions over their inputs, so which template a kind names, which flags carry
//! which values, and how a printed path becomes a stored one are all decided where they
//! can be checked without `zk` installed. The single impure function is [`Runner::run`].
//!
//! The crate's tests still pass with nothing on `PATH`: everything above is tested
//! against a recorded runner, and the handful of tests that want the real program ask
//! [`zk_is_installed`] first and return early when it is not.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::feature::{FeatureId, note_path_refusal};
use crate::layout;

/// The program every invocation here runs. Not configurable: the notebook format is
/// `zk`'s, so a different program would be a different format.
pub const ZK: &str = "zk";

/// The directory `zk` keeps a notebook's configuration and templates in.
pub const ZK_DIR: &str = ".zk";

/// Where a notebook's templates live, under [`ZK_DIR`].
pub const TEMPLATES_DIR: &str = "templates";

/// The environment variable `zk` reads a notebook location from.
///
/// Removed from every child rather than trusted to lose: it names a notebook, and a GM
/// who exports it for their own notes would otherwise have campaign notes written there
/// whenever the working directory is not itself inside a notebook.
pub const NOTEBOOK_DIR_VAR: &str = "ZK_NOTEBOOK_DIR";

/// The configuration written into a notebook this tool initialises.
///
/// Fixes a filename a GM can read in a directory listing, and turns on the hashtags the
/// templates write their tags as. It is a convenience and not a contract: a template
/// writes its tag from `{{filename-stem}}`, so a slug still matches its own filename
/// under whatever filename rule the GM later sets here.
pub const CONFIG: &str = "\
[note]
filename = \"{{slug title}}-{{id}}\"
extension = \"md\"

[format.markdown]
hashtags = true
";

/// What a note is about, which decides the template it is made from and the tag it is
/// found by.
///
/// Deliberately not `#[non_exhaustive]`: a consumer matching on a kind to decide what to
/// offer should fail to compile when a kind is added, rather than fall into a catch-all
/// arm that silently offers nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteKind {
    Place,
    Person,
    Faction,
    Session,
}

impl NoteKind {
    /// Every kind, places first.
    ///
    /// Written out rather than derived, and the array length is the count — adding a
    /// variant without extending this fails to compile, the same guarantee the enum's
    /// lack of `#[non_exhaustive]` gives a `match`.
    pub const fn all() -> [Self; 4] {
        [Self::Place, Self::Person, Self::Faction, Self::Session]
    }

    /// The kinds nothing on the map creates, because they have no geometry: a person, a
    /// faction and a session hang off no feature.
    pub const fn without_geometry() -> [Self; 3] {
        [Self::Person, Self::Faction, Self::Session]
    }

    /// The kind a note made for a map feature is, whatever the feature is.
    ///
    /// Every feature is a place — a settlement, a road and a territory are all things
    /// somewhere on the map — so this takes no argument. It exists as the seam that a
    /// kind mapping elsewhere would go through, rather than as a table with one answer.
    pub const fn of_a_feature() -> Self {
        Self::Place
    }

    /// The template file this kind is made from, extension and all.
    ///
    /// `zk` resolves `--template` as a file name inside the notebook's template
    /// directory and refuses one without its extension, so the extension is part of the
    /// name rather than something a caller appends.
    pub const fn template(self) -> &'static str {
        match self {
            Self::Place => "place.md",
            Self::Person => "person.md",
            Self::Faction => "faction.md",
            Self::Session => "session.md",
        }
    }

    /// What this kind's tags start with, before the note's own slug.
    pub const fn tag_prefix(self) -> &'static str {
        match self {
            Self::Place => "place",
            Self::Person => "person",
            Self::Faction => "faction",
            Self::Session => "session",
        }
    }

    /// The template's text, compiled in.
    ///
    /// Carried in the binary rather than read from a directory beside it, so a `pnp`
    /// installed away from this source tree still has templates to install.
    pub const fn body(self) -> &'static str {
        match self {
            Self::Place => include_str!("templates/place.md"),
            Self::Person => include_str!("templates/person.md"),
            Self::Faction => include_str!("templates/faction.md"),
            Self::Session => include_str!("templates/session.md"),
        }
    }

    /// The word a GM reads for this kind.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Place => "place",
            Self::Person => "person",
            Self::Faction => "faction",
            Self::Session => "session",
        }
    }

    /// The kind `name` names, matching [`NoteKind::label`], or `None`.
    pub fn from_label(name: &str) -> Option<Self> {
        Self::all().into_iter().find(|kind| kind.label() == name)
    }
}

/// Why a note could not be made, installed or opened.
///
/// Every variant names what a GM can do something about: which program is missing, what
/// it said, which file could not be written, which path was refused.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NoteError {
    /// From every call: `zk` is not on `PATH`. The map, the document and every edit go
    /// on working; only notes are unavailable.
    #[error("`{ZK}` is not installed, or not on PATH, so notes cannot be created")]
    ZkMissing,

    /// From every call: `zk` ran and refused. The message is the first thing it said for
    /// itself, because a status line holds one line and its stderr is many.
    #[error("`{ZK} {verb}` failed: {message}")]
    ZkRefused { verb: &'static str, message: String },

    /// From `ensure`: the notebook's own directory, or one of the templates, could not
    /// be written. Whatever did get written stays, and the next note finishes the job.
    #[error("`{}` could not be written: {source}", .path.display())]
    Unwritable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// From `create`: `zk` printed a path this campaign cannot store on a feature — one
    /// that is outside the notebook, or that [`note_path_refusal`] rejects.
    #[error("`{}` is not a note inside `{}`: it {reason}", .printed.display(), .notes.display())]
    BadPath {
        printed: PathBuf,
        notes: PathBuf,
        reason: &'static str,
    },

    /// From `create`: a title that is empty or only spaces. `zk` would name such a note
    /// `Untitled`, and a GM would have no way to tell two of them apart.
    #[error("a note needs a title")]
    EmptyTitle,

    /// From `open`: there is no editor to open a note with.
    #[error("no editor is set: neither EDITOR nor VISUAL names one")]
    NoEditor,
}

/// A note `zk` made: where it is, and the slug it is found by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNote {
    /// The note's path, relative to the campaign's notes directory — what a feature
    /// stores.
    pub path: String,
    /// The note's slug: the stem of `path`, and the string its tag carries.
    pub slug: String,
}

/// How a `zk` invocation is actually run.
///
/// Exists so the argument vectors above can be checked without the program installed:
/// tests supply a runner that records what it was asked to run and hands back a chosen
/// answer. [`SystemRunner`] is the one that runs anything.
pub trait Runner {
    /// Run `ZK` with `args`, from inside `dir`, and hand back what it said.
    ///
    /// Returns [`NoteError::ZkMissing`] when the program is not there, and `Ok` for a
    /// program that ran and failed — the exit status is in the output, and classifying
    /// it is the caller's.
    fn run(&self, dir: &Path, args: &[OsString]) -> Result<std::process::Output, NoteError>;

    /// Start `ZK` with `args` from inside `dir` and do not wait for it.
    ///
    /// For `zk edit` alone, which runs the GM's editor: that does not return until the
    /// note is closed, so waiting would hold a thread for the length of the session.
    /// Nothing the child says afterwards can be heard.
    fn start(&self, dir: &Path, args: &[OsString]) -> Result<(), NoteError>;
}

/// The runner that runs the real program.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl SystemRunner {
    fn command(dir: &Path, args: &[OsString]) -> std::process::Command {
        let mut command = std::process::Command::new(ZK);
        command
            .args(args)
            .current_dir(dir)
            .env_remove(NOTEBOOK_DIR_VAR)
            .stdin(std::process::Stdio::null());
        command
    }
}

impl Runner for SystemRunner {
    fn run(&self, dir: &Path, args: &[OsString]) -> Result<std::process::Output, NoteError> {
        Self::command(dir, args).output().map_err(missing_or)
    }

    fn start(&self, dir: &Path, args: &[OsString]) -> Result<(), NoteError> {
        Self::command(dir, args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_child| ())
            .map_err(missing_or)
    }
}

fn missing_or(error: std::io::Error) -> NoteError {
    if error.kind() == std::io::ErrorKind::NotFound {
        NoteError::ZkMissing
    } else {
        NoteError::ZkRefused {
            verb: "run",
            message: error.to_string(),
        }
    }
}

/// Whether `zk` answers at all.
///
/// One process, asking only for a version. The editor asks once a session so a button
/// can say notes are unavailable before it is pressed rather than after.
pub fn zk_is_installed() -> bool {
    SystemRunner
        .run(Path::new("."), &[OsString::from("--version")])
        .is_ok_and(|output| output.status.success())
}

/// A campaign's notes directory, and everything done to it through `zk`.
///
/// Holds only where the notebook is. Whether it *is* a notebook yet is not a field: that
/// is a fact about the disk, which another process may change, and a copy here would be
/// a second answer that can go stale.
#[derive(Debug, Clone)]
pub struct Notebook {
    root: PathBuf,
}

impl Notebook {
    /// The notebook belonging to the campaign rooted at `campaign_root`.
    ///
    /// The notes directory is reached through [`layout::notes`] rather than spelled
    /// here, so the name lives in exactly one place.
    pub fn of(campaign_root: impl AsRef<Path>) -> Self {
        Self {
            root: layout::notes(campaign_root.as_ref()),
        }
    }

    /// Where the notes are.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether the notes directory already carries `zk`'s own directory.
    ///
    /// This decides whether `zk init` is run at all rather than whether it is worth
    /// running: `zk init` refuses a directory that is already a notebook.
    pub fn is_initialised(&self) -> bool {
        self.root.join(ZK_DIR).is_dir()
    }

    /// Make the notes directory a notebook, and make sure it holds the four templates.
    ///
    /// Two decisions, deliberately separate. `zk init` runs only when there is no
    /// notebook. The templates are ensured *every* time, each written only where no file
    /// of that name is there — so a notebook the GM made themselves gains them, one
    /// whose templates were half-written repairs itself rather than failing for ever,
    /// and a template a GM has edited is never overwritten.
    ///
    /// Writing a template with `create_new` is what gives that last guarantee: asking
    /// whether a file exists and then writing it would follow a symlink sitting where a
    /// template should be. The configuration is the one file written unconditionally, and
    /// only on the branch that just created it.
    pub fn ensure(&self, runner: &impl Runner) -> Result<(), NoteError> {
        if !self.is_initialised() {
            let args = init_args(&self.root);
            let output = runner.run(Path::new("."), &args)?;
            refused(&output, "init")?;

            let config = self.root.join(ZK_DIR).join("config.toml");
            std::fs::write(&config, CONFIG).map_err(|source| NoteError::Unwritable {
                path: config,
                source,
            })?;
        }

        let templates = self.root.join(ZK_DIR).join(TEMPLATES_DIR);
        std::fs::create_dir_all(&templates).map_err(|source| NoteError::Unwritable {
            path: templates.clone(),
            source,
        })?;
        for kind in NoteKind::all() {
            self.write(&templates.join(kind.template()), kind.body())?;
        }
        Ok(())
    }

    fn write(&self, path: &Path, text: &str) -> Result<(), NoteError> {
        use std::io::Write;

        match std::fs::File::create_new(path) {
            Ok(mut file) => file
                .write_all(text.as_bytes())
                .map_err(|source| NoteError::Unwritable {
                    path: path.to_owned(),
                    source,
                }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(source) => Err(NoteError::Unwritable {
                path: path.to_owned(),
                source,
            }),
        }
    }

    /// Make a note of `kind` titled `title`, belonging to `subject` when it has one.
    ///
    /// Initialises the notebook if it is not one yet, so a campaign created before there
    /// was such a thing grows one the first time a note is asked for, and one whose GM
    /// never takes a note never grows a `.zk` directory at all.
    ///
    /// Refuses an empty or whitespace-only `title` before running anything. Fails
    /// [`NoteError::ZkMissing`] when the program is absent, [`NoteError::ZkRefused`]
    /// when it ran and refused, and [`NoteError::BadPath`] when what it printed is not a
    /// note this campaign can store — in which case the file `zk` wrote is still there,
    /// and the error names it.
    ///
    /// A campaign reached through a symlink still yields a note: the printed path and the
    /// notes directory are both resolved to where they really are before one is taken out
    /// of the other, so the two agree however the campaign was opened.
    pub fn create(
        &self,
        runner: &impl Runner,
        kind: NoteKind,
        title: &str,
        subject: Option<FeatureId>,
    ) -> Result<NewNote, NoteError> {
        if title.trim().is_empty() {
            return Err(NoteError::EmptyTitle);
        }
        self.ensure(runner)?;

        let output = runner.run(&self.root, &new_args(kind, title, subject))?;
        refused(&output, "new")?;

        let printed = printed_path(&output.stdout).ok_or_else(|| NoteError::ZkRefused {
            verb: "new",
            message: "it printed no path".to_owned(),
        })?;
        let path = self.relative(&printed)?;
        let slug = slug_of(&path).to_owned();
        Ok(NewNote { path, slug })
    }

    /// Open the note at `path` — relative to the notes directory — in the GM's editor.
    ///
    /// Refuses a path that leaves the notebook once resolved.
    /// [`note_path_refusal`]'s containment is lexical, and a campaign directory is
    /// something a GM may have been handed rather than made, so the one place that opens
    /// a note is the place to check where the bytes really are.
    ///
    /// Decides there is an editor before starting anything, because the child is never
    /// waited on and so nothing it says for itself afterwards can be heard.
    pub fn open(&self, runner: &impl Runner, path: &str) -> Result<(), NoteError> {
        let inside = self.contained(path)?;
        if !has_an_editor() {
            return Err(NoteError::NoEditor);
        }
        runner.start(&self.root, &edit_args(&inside))
    }

    fn contained(&self, path: &str) -> Result<PathBuf, NoteError> {
        let bad = |reason| NoteError::BadPath {
            printed: PathBuf::from(path),
            notes: self.root.clone(),
            reason,
        };
        if let Some(reason) = note_path_refusal(path) {
            return Err(bad(reason));
        }
        let joined = self.root.join(path);
        let real = joined.canonicalize().map_err(|_| bad("is not there"))?;
        let root = self.root.canonicalize().map_err(|_| bad("is not there"))?;
        if !real.starts_with(&root) {
            return Err(bad("resolves to somewhere outside the notes directory"));
        }
        Ok(real)
    }

    fn relative(&self, printed: &Path) -> Result<String, NoteError> {
        let bad = |reason| NoteError::BadPath {
            printed: printed.to_owned(),
            notes: self.root.clone(),
            reason,
        };

        let real = printed.canonicalize().unwrap_or_else(|_| printed.to_owned());
        let root = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());

        let rest = real
            .strip_prefix(&root)
            .or_else(|_| printed.strip_prefix(&self.root))
            .map_err(|_| bad("is not inside the notes directory"))?;

        let rest = rest.to_str().ok_or_else(|| bad("is not valid UTF-8"))?;
        match note_path_refusal(rest) {
            Some(reason) => Err(bad(reason)),
            None => Ok(rest.to_owned()),
        }
    }
}

/// The arguments that make `dir` a notebook.
///
/// `zk init` takes the directory positionally and creates it, and any missing parent,
/// when it is not there.
pub fn init_args(dir: &Path) -> Vec<OsString> {
    vec![
        OsString::from("--no-input"),
        OsString::from("init"),
        dir.as_os_str().to_owned(),
    ]
}

/// The arguments that make one note.
///
/// Every value is joined to its flag as one argument rather than following it as a
/// separate word: `zk` reads a following word beginning with a dash as another option
/// and refuses, so a settlement named `-Kai` would otherwise be unnameable.
///
/// `subject` becomes an extra template variable carrying the feature's *number* — not
/// the way a [`FeatureId`] prints itself, which names a feature rather than numbering
/// it. The place template writes it into the note's frontmatter, so the link survives a
/// rename from either side.
///
/// The caller runs this from inside the notes directory: `zk new` resolves the note it
/// creates against the working directory, so naming the notebook is not on its own
/// enough to say where the note goes.
pub fn new_args(kind: NoteKind, title: &str, subject: Option<FeatureId>) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("--no-input"),
        OsString::from("new"),
        OsString::from(format!("--template={}", kind.template())),
        OsString::from(format!("--title={title}")),
    ];
    if let Some(id) = subject {
        args.push(OsString::from(format!("--extra=feature={}", id.0)));
    }
    args.push(OsString::from("--print-path"));
    args
}

/// The arguments that open one note.
pub fn edit_args(path: &Path) -> Vec<OsString> {
    vec![
        OsString::from("--no-input"),
        OsString::from("edit"),
        OsString::from("--force"),
        path.as_os_str().to_owned(),
    ]
}

/// The path `zk --print-path` printed, from what it wrote to standard output.
///
/// The last non-empty line: `zk` prints the path alone, but a line ending and anything a
/// notebook hook chose to say ahead of it are both things a caller should survive.
pub fn printed_path(stdout: &[u8]) -> Option<PathBuf> {
    let text = std::str::from_utf8(stdout).ok()?;
    text.lines()
        .map(str::trim)
        .rev()
        .find(|line| !line.is_empty())
        .map(PathBuf::from)
}

/// A note's slug: the stem of its own filename.
///
/// The templates write their tag from `{{filename-stem}}`, so this is the same string
/// the note is tagged with rather than a second derivation that has to agree with it.
pub fn slug_of(path: &str) -> &str {
    Path::new(path)
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or(path)
}

/// Whether the environment names an editor for `zk edit` to hand a note to.
pub fn has_an_editor() -> bool {
    ["EDITOR", "VISUAL"]
        .into_iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
}

fn refused(output: &std::process::Output, verb: &'static str) -> Result<(), NoteError> {
    if output.status.success() {
        return Ok(());
    }
    Err(NoteError::ZkRefused {
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
