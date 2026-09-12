//! The window title, and the one thing it is allowed to say: which campaign is open,
//! and whether it has unsaved changes.

/// The title when no campaign is open.
pub const PROGRAM_NAME: &str = "pnp";

/// The window title for a campaign named `open`, or [`PROGRAM_NAME`] with none open.
///
/// `open` is `(name, dirty)`: `dirty` appends a trailing `*`. A name that is empty after
/// trimming whitespace is treated as absent, so a manifest with a blank `name` still
/// produces a readable title rather than a bare `*`.
pub fn window_title(open: Option<(&str, bool)>) -> String {
    match open {
        None => PROGRAM_NAME.to_string(),
        Some((name, _)) if name.trim().is_empty() => PROGRAM_NAME.to_string(),
        Some((name, false)) => name.to_string(),
        Some((name, true)) => format!("{name}*"),
    }
}
