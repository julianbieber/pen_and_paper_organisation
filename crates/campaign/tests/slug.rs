//! Turning a label the GM typed into a file name, which is the one place this workspace
//! derives a path from free text rather than checking one handed back to it.

use campaign::feature::{FeatureId, dungeon_name_refusal};
use campaign::slug::{MAX_SLUG_CHARS, dungeon_name, slug_of};

// The ordinary case, and the reason the slug exists at all: a campaign directory is meant
// to be read and hand-edited, so `crypt-of-kel.ron` beats an opaque number.
#[test]
fn a_label_becomes_a_readable_name() {
    assert_eq!(slug_of("Crypt of Kel").as_deref(), Some("crypt-of-kel"));
    assert_eq!(slug_of("The  Old   Barrow").as_deref(), Some("the-old-barrow"));
    assert_eq!(slug_of("Level 2").as_deref(), Some("level-2"));
}

// Two names differing only in case are one file on a case-insensitive filesystem, and a
// campaign directory is meant to travel between machines.
#[test]
fn a_slug_is_lowercased_so_one_name_is_one_file_everywhere() {
    assert_eq!(slug_of("RIVERFORD").as_deref(), Some("riverford"));
    assert_eq!(slug_of("Riverford"), slug_of("riverford"));
}

// The allowlist is what closes the boundary: every hazardous shape is gone because
// nothing but ASCII letters and digits survives, rather than because it was listed.
#[test]
fn nothing_but_letters_and_digits_survives() {
    assert_eq!(slug_of("../../etc/passwd").as_deref(), Some("etc-passwd"));
    assert_eq!(slug_of("a/b\\c").as_deref(), Some("a-b-c"));
    assert_eq!(slug_of("-leading").as_deref(), Some("leading"));
    assert_eq!(slug_of("trailing-").as_deref(), Some("trailing"));
    assert_eq!(slug_of("with\u{0}nul").as_deref(), Some("with-nul"));
    assert_eq!(slug_of("Kelða's Tomb").as_deref(), Some("kel-a-s-tomb"));
}

// The distinction the fallback turns on: a label made entirely of punctuation is not
// empty, but its slug is — and it is the slug being empty that matters.
#[test]
fn a_label_that_slugs_to_nothing_yields_nothing() {
    for label in ["", "   ", "!!!", "...", "---", "。。。", "🎲🎲"] {
        assert_eq!(slug_of(label), None, "`{label}` should slug to nothing");
    }
}

// A file name has a length a filesystem will take, and the extension and any uniquing
// suffix still have to fit inside it.
#[test]
fn a_very_long_label_is_capped() {
    let slug = slug_of(&"a".repeat(500)).expect("letters survive");
    assert_eq!(slug.chars().count(), MAX_SLUG_CHARS);
}

// The fallback is on the derived slug being empty, not on the label being empty — which is
// what makes a label of "!!!" produce a usable name instead of a bare extension.
#[test]
fn a_label_with_no_slug_falls_back_to_the_feature_id() {
    let name = dungeon_name("!!!", FeatureId(7), []);
    assert_eq!(name, "dungeon-7.ron");
    assert_eq!(dungeon_name("", FeatureId(0), []), "dungeon-0.ron");
}

// A dungeon that has been opened but not yet saved has a name and no file, so a directory
// listing alone would let a second entry claim it.
#[test]
fn a_name_already_spoken_for_is_stepped_past() {
    assert_eq!(dungeon_name("Crypt", FeatureId(1), []), "crypt.ron");
    assert_eq!(
        dungeon_name("Crypt", FeatureId(1), ["crypt.ron"]),
        "crypt-2.ron"
    );
    assert_eq!(
        dungeon_name("Crypt", FeatureId(1), ["crypt.ron", "crypt-2.ron"]),
        "crypt-3.ron"
    );
}

// The whole point of deriving a name here is that it can then be stored, so everything
// this module produces has to satisfy the rule that guards the field it is stored in.
#[test]
fn every_derived_name_is_one_the_document_format_accepts() {
    let labels = [
        "Crypt of Kel",
        "",
        "!!!",
        "../../etc/passwd",
        "-dash",
        &"z".repeat(400),
        "🎲",
        "CON",
        "a/b",
    ];
    for label in labels {
        let name = dungeon_name(label, FeatureId(3), ["crypt-of-kel.ron"]);
        assert_eq!(
            dungeon_name_refusal(&name),
            None,
            "`{label}` derived `{name}`, which the format refuses"
        );
    }
}
