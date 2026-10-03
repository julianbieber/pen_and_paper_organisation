//! Tokens on a combat map: what a placed token is called, where it lands, what it covers,
//! which one a cell picks, and what is refused — all without a window.

use campaign::token::{
    MAX_TOKEN_NAME_CHARS, MAX_TOKENS, TokenProblem, Tokens, anchor, initiative_of, name_refusal, next_name,
};

const EXTENT: (u32, u32) = (12, 12);

fn names(tokens: &Tokens) -> Vec<&str> {
    tokens.iter().map(|token| token.name()).collect()
}

fn place(tokens: &mut Tokens, typed: &str, at: (i64, i64)) -> String {
    tokens.place(typed, 2, at, EXTENT).expect("the token places").name().to_owned()
}

// The issue's first acceptance step: a bare name clicked three times numbers from one.
#[test]
fn a_bare_name_numbers_each_token_it_places() {
    let mut tokens = Tokens::default();
    for x in [1, 5, 9] {
        place(&mut tokens, "orc", (x, 1));
    }
    assert_eq!(names(&tokens), ["orc1", "orc2", "orc3"]);
}

// The issue's example: a deleted number below the highest is not reused.
#[test]
fn deleting_a_middle_token_numbers_the_next_past_the_highest() {
    let mut tokens = Tokens::default();
    for x in [1, 5, 9] {
        place(&mut tokens, "orc", (x, 1));
    }
    tokens.remove("orc2").unwrap();
    assert_eq!(place(&mut tokens, "orc", (1, 8)), "orc4");
}

// The acceptance sequence: after a rename and a delete, `orc` never places a second orc2.
#[test]
fn the_acceptance_sequence_never_places_a_second_orc2() {
    let mut tokens = Tokens::default();
    for x in [1, 5, 9] {
        place(&mut tokens, "orc", (x, 1));
    }
    assert_eq!(tokens.rename("orc3", "chief").unwrap(), "chief");
    tokens.remove("orc1").unwrap();
    assert_eq!(place(&mut tokens, "orc", (1, 8)), "orc3");
    assert_eq!(names(&tokens), ["orc2", "chief", "orc3"]);
}

// A name the GM numbered themselves is placed as typed, and names stay unique.
#[test]
fn a_typed_number_is_kept_and_refused_a_second_time() {
    let mut tokens = Tokens::default();
    assert_eq!(place(&mut tokens, "orc7", (0, 0)), "orc7");
    assert_eq!(
        tokens.place("orc7", 1, (4, 4), EXTENT).unwrap_err(),
        TokenProblem::Taken { name: "orc7".to_owned() }
    );
    assert_eq!(place(&mut tokens, "orc", (4, 4)), "orc8");
}

// A name ending in a letter is a base like any other, and a longer base is not the same one.
#[test]
fn only_the_base_followed_by_digits_counts_toward_its_number() {
    assert_eq!(next_name("playerA", []).unwrap(), "playerA1");
    assert_eq!(next_name("orc", ["orca1", "Orc4", "orc2x"]).unwrap(), "orc1");
    assert_eq!(next_name("  orc ", ["orc2"]).unwrap(), "orc3");
}

// The map letters names with a font of ASCII 32–126, so nothing it cannot draw gets in.
#[test]
fn blank_undrawable_and_overlong_names_are_refused() {
    assert_eq!(name_refusal("   "), Some(TokenProblem::Unnamed));
    assert!(matches!(name_refusal("orc\u{e9}"), Some(TokenProblem::Undrawable { .. })));
    assert!(matches!(name_refusal("a\tb"), Some(TokenProblem::Undrawable { .. })));
    let long = "x".repeat(MAX_TOKEN_NAME_CHARS + 1);
    assert!(matches!(name_refusal(&long), Some(TokenProblem::TooLong { .. })));
    let numbered = "x".repeat(MAX_TOKEN_NAME_CHARS);
    assert!(matches!(next_name(&numbered, []), Err(TokenProblem::TooLong { .. })));
    assert_eq!(name_refusal("orc 2"), None);
}

// A base whose number would overflow is refused rather than wrapping onto a taken name.
#[test]
fn a_base_out_of_numbers_is_refused() {
    let highest = format!("orc{}", u32::MAX);
    assert_eq!(
        next_name("orc", [highest.as_str()]),
        Err(TokenProblem::OutOfNumbers { base: "orc".to_owned() })
    );
}

// Sizes are 1 to 4 cells a side, and a token that cannot fit on the map is not placed.
#[test]
fn sizes_outside_the_range_or_the_map_are_refused() {
    let mut tokens = Tokens::default();
    assert_eq!(tokens.place("orc", 0, (0, 0), EXTENT).unwrap_err(), TokenProblem::BadSize { size: 0 });
    assert_eq!(tokens.place("orc", 5, (0, 0), EXTENT).unwrap_err(), TokenProblem::BadSize { size: 5 });
    assert_eq!(
        tokens.place("orc", 4, (0, 0), (3, 8)).unwrap_err(),
        TokenProblem::TooBig { size: 4, width: 3, height: 8 }
    );
    assert!(tokens.is_empty());
}

// The acceptance's "each covering four cells": a size-2 token covers exactly its square.
#[test]
fn a_size_two_token_covers_exactly_four_cells() {
    let mut tokens = Tokens::default();
    let token = tokens.place("orc", 2, (3, 4), EXTENT).unwrap();
    assert_eq!(token.cells().collect::<Vec<_>>(), [(3, 4), (4, 4), (3, 5), (4, 5)]);
    assert!(token.covers(4, 5));
    assert!(!token.covers(5, 4));
    assert!(!token.covers(2, 4));
}

// A press near the right or bottom edge keeps the whole token on the map.
#[test]
fn a_token_pressed_past_an_edge_is_clamped_onto_the_map() {
    assert_eq!(anchor((11, 11), 2, 12, 12), (10, 10));
    assert_eq!(anchor((-3, 5), 3, 12, 12), (0, 5));
    let mut tokens = Tokens::default();
    let token = tokens.place("orc", 4, (40, 40), EXTENT).unwrap();
    assert_eq!((token.x(), token.y()), (8, 8));
}

// Tokens may overlap, and a click picks the one drawn on top.
#[test]
fn the_later_of_two_overlapping_tokens_is_topmost() {
    let mut tokens = Tokens::default();
    place(&mut tokens, "orc", (2, 2));
    place(&mut tokens, "orc", (3, 3));
    assert_eq!(tokens.topmost_at(3, 3).unwrap().name(), "orc2");
    assert_eq!(tokens.topmost_at(2, 2).unwrap().name(), "orc1");
    assert!(tokens.topmost_at(6, 6).is_none());
}

// The acceptance's drag: a token lands on whole cells and never leaves the map.
#[test]
fn a_drag_moves_a_token_whole_cells_and_clamps_at_the_edge() {
    let mut tokens = Tokens::default();
    place(&mut tokens, "orc", (5, 1));
    assert_eq!(tokens.dragged("orc1", (5, 1), (6, 4), EXTENT), Some((6, 4)));
    assert_eq!(tokens.get("orc1").map(|t| (t.x(), t.y())), Some((5, 1)), "dragged changes nothing");

    assert_eq!(tokens.move_by("orc1", (5, 1), (6, 4), EXTENT), Ok(true));
    assert_eq!(tokens.get("orc1").map(|t| (t.x(), t.y())), Some((6, 4)));
    assert_eq!(tokens.move_by("orc1", (6, 4), (6, 4), EXTENT), Ok(false));
    assert_eq!(tokens.move_by("orc1", (6, 4), (60, -9), EXTENT), Ok(true));
    assert_eq!(tokens.get("orc1").map(|t| (t.x(), t.y())), Some((10, 0)));
    assert_eq!(
        tokens.move_by("orc9", (0, 0), (1, 1), EXTENT),
        Err(TokenProblem::Unknown { name: "orc9".to_owned() })
    );
}

// Renames address tokens by name, so a taken name is refused and an unchanged one is not.
#[test]
fn a_rename_onto_a_taken_name_is_refused_and_onto_its_own_is_not() {
    let mut tokens = Tokens::default();
    place(&mut tokens, "orc", (1, 1));
    place(&mut tokens, "orc", (5, 1));
    assert_eq!(
        tokens.rename("orc1", "orc2"),
        Err(TokenProblem::Taken { name: "orc2".to_owned() })
    );
    assert_eq!(tokens.rename("orc1", " orc1 "), Ok("orc1".to_owned()));
    assert_eq!(tokens.rename("orc1", "  "), Err(TokenProblem::Unnamed));
    assert!(matches!(tokens.rename("nobody", "x"), Err(TokenProblem::Unknown { .. })));
    assert_eq!(names(&tokens), ["orc1", "orc2"]);
}

// Removing a name nothing carries is refused rather than silently ignored.
#[test]
fn removing_an_unknown_token_is_refused() {
    let mut tokens = Tokens::default();
    assert_eq!(tokens.remove("orc1"), Err(TokenProblem::Unknown { name: "orc1".to_owned() }));
}

// A board is bounded, so a stuck mouse button cannot grow one without end.
#[test]
fn a_full_board_refuses_another_token() {
    let mut tokens = Tokens::default();
    for _ in 0..MAX_TOKENS {
        tokens.place("orc", 1, (0, 0), EXTENT).unwrap();
    }
    assert_eq!(tokens.place("orc", 1, (0, 0), EXTENT).unwrap_err(), TokenProblem::Full);
    tokens.clear();
    assert!(tokens.is_empty());
}

fn order(tokens: &Tokens) -> Vec<&str> {
    tokens.in_initiative_order().into_iter().map(|token| token.name()).collect()
}

// The initiative list reads highest first, and a token that has not rolled waits at the end
// rather than acting as though it rolled the lowest number.
#[test]
fn initiative_orders_the_highest_roll_first_and_the_unrolled_last() {
    let mut tokens = Tokens::default();
    for x in [1, 4, 7, 10] {
        place(&mut tokens, "orc", (x, 1));
    }
    tokens.set_initiative("orc2", Some(-3)).unwrap();
    tokens.set_initiative("orc3", Some(17)).unwrap();
    tokens.set_initiative("orc4", Some(17)).unwrap();
    assert_eq!(order(&tokens), ["orc3", "orc4", "orc2", "orc1"]);
}

// A roll is the token's own, so renaming it keeps its place in the order.
#[test]
fn a_renamed_token_keeps_its_initiative() {
    let mut tokens = Tokens::default();
    place(&mut tokens, "orc", (1, 1));
    tokens.set_initiative("orc1", Some(12)).unwrap();
    tokens.rename("orc1", "Grukk").unwrap();
    assert_eq!(tokens.get("Grukk").unwrap().initiative(), Some(12));
}

// Clearing the field forgets the roll; anything but a whole number is refused rather than
// read as zero.
#[test]
fn an_initiative_field_reads_empty_as_unrolled_and_refuses_words() {
    assert_eq!(initiative_of("  "), Ok(None));
    assert_eq!(initiative_of(" -2 "), Ok(Some(-2)));
    assert_eq!(
        initiative_of("d20"),
        Err(TokenProblem::BadInitiative { typed: "d20".to_owned() })
    );
}

// Setting the roll of a name nothing carries is refused rather than silently ignored.
#[test]
fn setting_the_initiative_of_an_unknown_token_is_refused() {
    let mut tokens = Tokens::default();
    assert_eq!(
        tokens.set_initiative("orc1", Some(3)),
        Err(TokenProblem::Unknown { name: "orc1".to_owned() })
    );
}
