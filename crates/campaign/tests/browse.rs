//! Everything `browse.rs` decides: what one directory lists, the fallback order a
//! walk starts from, and what "up" is. No test here sets an environment variable —
//! `start_candidates` and `open` take `home` as an argument for exactly that reason.

use std::ffi::OsStr;

use campaign::browse::{self, BrowseError, MAX_LISTED};

// Files sit beside directories in a listing; only the directories are offered.
#[test]
fn files_are_not_listed_directories_are() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("a-dir")).unwrap();
    std::fs::write(tmp.path().join("a-file"), "").unwrap();

    let listing = browse::list(tmp.path()).unwrap();

    assert_eq!(listing.subdirectories.len(), 1);
    assert_eq!(listing.subdirectories[0].name, "a-dir");
}

// A directory named `.something` is hidden and never listed.
#[test]
fn a_hidden_directory_is_not_listed() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join(".hidden")).unwrap();
    std::fs::create_dir(tmp.path().join("visible")).unwrap();

    let listing = browse::list(tmp.path()).unwrap();

    assert_eq!(listing.subdirectories.len(), 1);
    assert_eq!(listing.subdirectories[0].name, "visible");
}

// The order is case-insensitive, so "A" sorts beside "b" rather than before it by
// ASCII case.
#[test]
fn the_order_is_case_insensitive() {
    let tmp = tempfile::tempdir().unwrap();
    for name in ["b", "A", "c"] {
        std::fs::create_dir(tmp.path().join(name)).unwrap();
    }

    let listing = browse::list(tmp.path()).unwrap();

    let names: Vec<_> = listing.subdirectories.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["A", "b", "c"]);
}

// A symlink to a directory is followed and listed; a dangling symlink is skipped like
// any other unreadable entry.
#[cfg(unix)]
#[test]
fn a_symlink_to_a_directory_is_listed_a_dangling_one_is_not() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, tmp.path().join("link")).unwrap();
    std::os::unix::fs::symlink(tmp.path().join("nowhere"), tmp.path().join("dangling")).unwrap();

    let listing = browse::list(tmp.path()).unwrap();

    let names: Vec<_> = listing.subdirectories.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["link", "real"]);
}

// A directory over `MAX_LISTED` subdirectories is truncated, and the excess is
// counted rather than dropped silently.
#[test]
fn over_max_listed_the_excess_is_counted() {
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..MAX_LISTED + 3 {
        std::fs::create_dir(tmp.path().join(format!("d{i:04}"))).unwrap();
    }

    let listing = browse::list(tmp.path()).unwrap();

    assert_eq!(listing.subdirectories.len(), MAX_LISTED);
    assert_eq!(listing.unlisted, 3);
}

// Listing a path that does not exist names that path in the error.
#[test]
fn list_of_a_missing_path_is_unreadable() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("nowhere");

    let error = browse::list(&missing).unwrap_err();

    assert!(matches!(error, BrowseError::Unreadable { .. }));
    assert_eq!(error.path(), missing);
}

// The filesystem root has no directory above it; any other directory's parent does.
#[test]
fn up_is_none_at_the_root_and_the_parent_otherwise() {
    assert_eq!(browse::up(std::path::Path::new("/")), None);
    assert_eq!(
        browse::up(std::path::Path::new("/a/b")),
        Some(std::path::PathBuf::from("/a"))
    );
}

// The fallback order is the trimmed field, then home, then `/`; a blank field or an
// empty home is skipped rather than offered as a candidate.
#[test]
fn start_candidates_falls_back_from_field_to_home_to_root() {
    let home = OsStr::new("/home/gm");
    assert_eq!(
        browse::start_candidates("  ", Some(home)),
        vec![
            std::path::PathBuf::from("/home/gm"),
            std::path::PathBuf::from(std::path::MAIN_SEPARATOR_STR),
        ]
    );
    assert_eq!(
        browse::start_candidates("x", None),
        vec![
            std::path::PathBuf::from("x"),
            std::path::PathBuf::from(std::path::MAIN_SEPARATOR_STR),
        ]
    );
    assert_eq!(
        browse::start_candidates("x", Some(OsStr::new(""))),
        vec![
            std::path::PathBuf::from("x"),
            std::path::PathBuf::from(std::path::MAIN_SEPARATOR_STR),
        ]
    );
}

// An empty field with a usable home opens the home directory, canonicalized.
#[test]
fn open_of_an_empty_field_lists_home() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("child")).unwrap();
    let home = tmp.path().as_os_str();

    let listing = browse::open("", Some(home)).unwrap();

    assert_eq!(listing.dir, std::fs::canonicalize(tmp.path()).unwrap());
}

// A field naming a file, or a missing path, falls back to home rather than to the
// nearest existing ancestor.
#[test]
fn open_of_a_file_or_a_missing_path_falls_back_to_home() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("a-file");
    std::fs::write(&file, "").unwrap();
    let missing = tmp.path().join("nowhere");
    let home = tmp.path().as_os_str();

    for field in [file.to_str().unwrap(), missing.to_str().unwrap()] {
        let listing = browse::open(field, Some(home)).unwrap();
        assert_eq!(listing.dir, std::fs::canonicalize(tmp.path()).unwrap());
    }
}

// A field naming a real directory opens that directory, not home.
#[test]
fn open_of_a_real_directory_opens_it_rather_than_home() {
    let tmp = tempfile::tempdir().unwrap();
    let wanted = tmp.path().join("wanted");
    let other_home = tmp.path().join("other-home");
    std::fs::create_dir(&wanted).unwrap();
    std::fs::create_dir(&other_home).unwrap();

    let listing = browse::open(wanted.to_str().unwrap(), Some(other_home.as_os_str())).unwrap();

    assert_eq!(listing.dir, std::fs::canonicalize(&wanted).unwrap());
}
