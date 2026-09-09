//! Turning a label the GM typed into the name of a file this tool creates.
//!
//! The only place in this workspace where that happens. A note's name is chosen by `zk`
//! and [`crate::notebook`] merely checks what came back; a dungeon's is derived here, and
//! then opened for writing — so this is the one trust boundary the campaign format
//! crosses, and it is closed with an allowlist rather than a list of things to reject.
//!
//! Deriving a name and deciding whether a name is allowed are kept apart, exactly as they
//! are for a note: [`dungeon_name_refusal`](crate::feature::dungeon_name_refusal) is the
//! rule, and everything this module produces is put through it by the caller that stores
//! it.

use crate::feature::{DUNGEON_EXTENSION, FeatureId};

/// The most characters a derived slug runs to before the extension and any suffix.
///
/// Short enough that the extension and a uniquing suffix still fit inside
/// [`MAX_DUNGEON_NAME_BYTES`](crate::feature::MAX_DUNGEON_NAME_BYTES), so a name this
/// module derives is always one that can be stored.
pub const MAX_SLUG_CHARS: usize = 64;

/// The slug for `label`, or `None` when nothing survives the allowlist.
///
/// Lowercase ASCII letters and digits are kept, everything else becomes a separator, runs
/// of separators collapse, and the ends are trimmed. `None` rather than an empty string,
/// because "the GM typed a label made entirely of punctuation" is a real case and it is
/// not the same as "the GM typed nothing".
///
/// Lowercased rather than merely allowed to be mixed case: two names differing only in
/// case are one file on a case-insensitive filesystem, and a campaign directory is meant
/// to be portable.
pub fn slug_of(label: &str) -> Option<String> {
    let mut slug = String::with_capacity(label.len().min(MAX_SLUG_CHARS));
    let mut pending = false;

    for character in label.chars() {
        if character.is_ascii_alphanumeric() {
            if pending && !slug.is_empty() {
                slug.push('-');
            }
            pending = false;
            slug.push(character.to_ascii_lowercase());
            if slug.chars().count() >= MAX_SLUG_CHARS {
                break;
            }
        } else {
            pending = true;
        }
    }

    (!slug.is_empty()).then_some(slug)
}

/// The dungeon file name for a feature labelled `label`, avoiding every name in `taken`.
///
/// Falls back to the feature's own id when the label yields no slug — which covers an
/// empty label and equally a label of "!!!" or one written entirely in a script the
/// allowlist drops. The fallback is on the **derived slug** being empty rather than on the
/// label being empty, because those are different conditions and only the first is the one
/// that matters.
///
/// `taken` must hold every name already spoken for, from the document *and* from the
/// directory: a dungeon that has been opened but not yet saved has a name and no file, so
/// a directory listing alone would let two entries claim it.
///
/// The result always ends in [`DUNGEON_EXTENSION`] and always passes
/// [`dungeon_name_refusal`](crate::feature::dungeon_name_refusal).
pub fn dungeon_name<'a>(
    label: &str,
    id: FeatureId,
    taken: impl IntoIterator<Item = &'a str>,
) -> String {
    let stem = slug_of(label).unwrap_or_else(|| format!("dungeon-{}", id.0));
    let spoken_for: Vec<&str> = taken.into_iter().collect();

    let candidate = format!("{stem}{DUNGEON_EXTENSION}");
    if !spoken_for.contains(&candidate.as_str()) {
        return candidate;
    }
    for suffix in 2u32.. {
        let candidate = format!("{stem}-{suffix}{DUNGEON_EXTENSION}");
        if !spoken_for.contains(&candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("the suffix range is unbounded, so some candidate is always free")
}
