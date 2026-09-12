//! Everything `recent.rs` decides: the dedup-and-cap rule, reading and writing the
//! file, whether a remembered root still holds a manifest, and the config-directory
//! rule. No test here sets an environment variable — `config_dir_from` is a pure
//! function over its two inputs for exactly that reason.

use campaign::recent::{self, Listed, MAX_RECENT, Recent, RecentError, Recents};

fn recent(root: &std::path::Path, name: &str) -> Recent {
    Recent {
        root: root.to_owned(),
        name: name.to_owned(),
    }
}

// A freshly remembered root is the first entry.
#[test]
fn remember_puts_the_newest_first() {
    let mut recents = Recents::default();
    recents.remember(std::path::Path::new("/a"), "A");
    recents.remember(std::path::Path::new("/b"), "B");

    assert_eq!(recents.entries[0].root, std::path::PathBuf::from("/b"));
    assert_eq!(recents.entries[1].root, std::path::PathBuf::from("/a"));
}

// Re-opening an already-remembered root moves it to the front instead of adding a
// second entry.
#[test]
fn remember_moves_an_existing_root_to_the_front_rather_than_duplicating_it() {
    let mut recents = Recents::default();
    recents.remember(std::path::Path::new("/a"), "A");
    recents.remember(std::path::Path::new("/b"), "B");
    recents.remember(std::path::Path::new("/a"), "A");

    assert_eq!(recents.entries.len(), 2);
    assert_eq!(recents.entries[0].root, std::path::PathBuf::from("/a"));
}

// A root already listed under one name is updated when it is remembered again under
// another.
#[test]
fn remember_updates_the_name_of_a_root_already_listed() {
    let mut recents = Recents::default();
    recents.remember(std::path::Path::new("/a"), "Old Name");
    recents.remember(std::path::Path::new("/a"), "New Name");

    assert_eq!(recents.entries.len(), 1);
    assert_eq!(recents.entries[0].name, "New Name");
}

// The list never grows past `MAX_RECENT`; the oldest entry is dropped first.
#[test]
fn remember_truncates_to_max_recent_dropping_the_oldest() {
    let mut recents = Recents::default();
    for i in 0..MAX_RECENT + 2 {
        recents.remember(&std::path::PathBuf::from(format!("/c{i}")), "C");
    }

    assert_eq!(recents.entries.len(), MAX_RECENT);
    assert_eq!(
        recents.entries[0].root,
        std::path::PathBuf::from(format!("/c{}", MAX_RECENT + 1))
    );
    assert!(
        !recents
            .entries
            .iter()
            .any(|e| e.root == std::path::PathBuf::from("/c0"))
    );
}

// An absent file reads as an empty list, not an error — the same rule an absent
// `world.ron` follows.
#[test]
fn read_of_an_absent_file_is_an_empty_list() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("nowhere").join(recent::RECENT_FILE);

    let recents = Recents::read(&path).unwrap();
    assert_eq!(recents, Recents::default());
}

// A file that is not valid RON for `Recents` is `Malformed`.
#[test]
fn read_of_garbage_is_malformed() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(recent::RECENT_FILE);
    std::fs::write(&path, "not ron at all").unwrap();

    assert!(matches!(
        Recents::read(&path),
        Err(RecentError::Malformed { .. })
    ));
}

// A file over `MAX_RECENT_BYTES` is refused before it is read into memory.
#[test]
fn read_of_an_oversized_file_is_unreadable() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(recent::RECENT_FILE);
    std::fs::write(&path, vec![b' '; (recent::MAX_RECENT_BYTES + 1) as usize]).unwrap();

    assert!(matches!(
        Recents::read(&path),
        Err(RecentError::Unreadable { .. })
    ));
}

// A list written and read back is unchanged, and `write` creates the config
// directory it writes into.
#[test]
fn write_then_read_round_trips_and_creates_its_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("pnp").join(recent::RECENT_FILE);

    let mut recents = Recents::default();
    recents.remember(std::path::Path::new("/a"), "A");
    recents.write(&path).unwrap();

    let read_back = Recents::read(&path).unwrap();
    assert_eq!(read_back, recents);
}

// The temporary file used to write atomically never survives a successful write.
#[test]
fn write_leaves_no_tmp_beside_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(recent::RECENT_FILE);

    Recents::default().write(&path).unwrap();

    assert!(!path.with_extension("ron.tmp").exists());
}

// `survey` marks an entry whose root holds a `campaign.ron` present, and one whose
// root does not, absent — touching the file is enough, so this needs no terrain.
#[test]
fn survey_marks_presence_by_whether_the_root_holds_a_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let present_root = tmp.path().join("present");
    let missing_root = tmp.path().join("missing");
    std::fs::create_dir_all(&present_root).unwrap();
    std::fs::write(present_root.join("campaign.ron"), "").unwrap();

    let mut recents = Recents::default();
    recents.remember(&missing_root, "Missing");
    recents.remember(&present_root, "Present");
    let path = tmp.path().join(recent::RECENT_FILE);
    recents.write(&path).unwrap();

    let listed = recent::survey(&path).unwrap();
    assert_eq!(
        listed,
        vec![
            Listed {
                recent: recent(&present_root, "Present"),
                present: true,
            },
            Listed {
                recent: recent(&missing_root, "Missing"),
                present: false,
            },
        ]
    );
}

// `record` over a file this build cannot read leaves a list holding exactly the new
// campaign, rather than failing to remember anything.
#[test]
fn record_over_a_garbage_file_leaves_only_the_new_campaign() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(recent::RECENT_FILE);
    std::fs::write(&path, "not ron at all").unwrap();

    recent::record(&path, std::path::Path::new("/a"), "A").unwrap();

    let recents = Recents::read(&path).unwrap();
    assert_eq!(recents.entries, vec![recent(std::path::Path::new("/a"), "A")]);
}

// An absolute `XDG_CONFIG_HOME` wins; an empty or relative one is ignored in favour of
// `<home>/.config`; with neither, there is no config directory at all.
#[test]
fn config_dir_from_prefers_an_absolute_xdg_and_falls_back_to_home() {
    use std::ffi::OsStr;

    assert_eq!(
        recent::config_dir_from(Some(OsStr::new("/xdg")), Some(OsStr::new("/home/gm"))),
        Some(std::path::PathBuf::from("/xdg/pnp"))
    );
    assert_eq!(
        recent::config_dir_from(Some(OsStr::new("")), Some(OsStr::new("/home/gm"))),
        Some(std::path::PathBuf::from("/home/gm/.config/pnp"))
    );
    assert_eq!(
        recent::config_dir_from(Some(OsStr::new("relative")), Some(OsStr::new("/home/gm"))),
        Some(std::path::PathBuf::from("/home/gm/.config/pnp"))
    );
    assert_eq!(recent::config_dir_from(None, None), None);
}
