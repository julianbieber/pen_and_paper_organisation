//! The window title says which campaign is open and whether it is unsaved, and nothing
//! else — these are the only shapes it can take.

use campaign::title::{PROGRAM_NAME, window_title};

// No campaign open reads as the bare program name, never a blank title.
#[test]
fn no_campaign_reads_as_the_program_name() {
    assert_eq!(window_title(None), PROGRAM_NAME);
}

// A saved campaign reads as its own name, with no decoration.
#[test]
fn a_saved_campaign_reads_as_its_name() {
    assert_eq!(window_title(Some(("Riverford", false))), "Riverford");
}

// An unsaved campaign is marked, so the GM can tell from the title bar alone.
#[test]
fn an_unsaved_campaign_is_marked_with_a_star() {
    assert_eq!(window_title(Some(("Riverford", true))), "Riverford*");
}

// A manifest name that is blank or only whitespace is not a usable title, so it falls
// back exactly as an absent campaign does.
#[test]
fn a_blank_name_falls_back_to_the_program_name() {
    assert_eq!(window_title(Some(("", false))), PROGRAM_NAME);
    assert_eq!(window_title(Some(("   ", true))), PROGRAM_NAME);
}
