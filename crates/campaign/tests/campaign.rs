//! What a campaign directory admits and refuses, exercised end to end against real
//! directories on disk — including a real `watershed` terrain, because that is what
//! `Campaign::open` demands.

use std::path::{Path, PathBuf};

use campaign::{CAMPAIGN_VERSION, Campaign, CampaignError, CampaignManifest};
use glam::UVec2;
use watershed::{ChannelMeta, FieldInfo, FieldRole, IoError, LayerTexels, Terrain, TerrainLayer};

fn write_loadable_terrain(dir: &Path) {
    let size = UVec2::new(8, 8);
    let bytes: Vec<u8> = (0..64u32).map(|i| (i * 4) as u8).collect();
    let texels = LayerTexels::from_bytes(size, 1, bytes).expect("8x8x1 texels");
    let layer = TerrainLayer::new(Some(0), vec![ChannelMeta::linear(0.0, 100.0)], texels);
    let field = FieldInfo {
        name: "height".to_owned(),
        role: FieldRole::Height,
        shift: 0,
        categorical: false,
        layer: 0,
        channel: 0,
    };

    Terrain::new(size, vec![field], vec![layer], None)
        .save_to_dir(dir)
        .expect("write the fixture terrain");

    Terrain::load_from_dir(dir).expect("the fixture terrain must load, or it tests nothing");
}

fn root_with_terrain() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path().join("my-campaign");
    write_loadable_terrain(&root.join("terrain"));
    (tmp, root)
}

fn entries(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .expect("read the directory")
        .map(|entry| {
            let entry = entry.expect("a directory entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            (name, std::fs::read(entry.path()).unwrap_or_default())
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

// The acceptance round trip: what `create` writes is what `open` reads back.
#[test]
fn create_then_open_round_trips_the_manifest() {
    let (_tmp, root) = root_with_terrain();

    let created = Campaign::create(&root, "terrain").expect("create");
    let opened = Campaign::open(&root).expect("open");

    assert_eq!(created.manifest(), opened.manifest());
    assert_eq!(opened.manifest().version, CAMPAIGN_VERSION);
    assert_eq!(opened.manifest().terrain, "terrain");
    assert_eq!(opened.manifest().name, "my-campaign");
}

// `create` makes exactly the layout it promises, and no terrain directory.
#[test]
fn create_writes_the_layout_and_not_a_terrain() {
    let (_tmp, root) = root_with_terrain();
    Campaign::create(&root, "terrain").expect("create");

    for subdir in ["dungeons", "images", "notes"] {
        assert!(root.join(subdir).is_dir(), "{subdir} was not created");
    }
    assert!(root.join("campaign.ron").is_file());
    assert!(
        !root.join("world.ron").exists(),
        "world.ron belongs to the document format, not to create"
    );
    assert!(
        !root.join("notes/.zk").exists(),
        "making notes a zk notebook is not this crate's business"
    );
}

// The terrain is recorded, never touched — the one invariant a caller cannot see.
#[test]
fn create_does_not_write_into_the_terrain_directory() {
    let (_tmp, root) = root_with_terrain();
    let terrain = root.join("terrain");

    let before = entries(&terrain);
    Campaign::create(&root, "terrain").expect("create");
    let after = entries(&terrain);

    assert_eq!(before, after);
}

// A directory with no manifest is not a campaign, and neither is one where
// `campaign.ron` is a directory.
#[test]
fn a_root_without_a_manifest_is_not_a_campaign() {
    let (_tmp, root) = root_with_terrain();
    std::fs::create_dir_all(&root).unwrap();
    assert!(matches!(
        Campaign::open(&root),
        Err(CampaignError::NotACampaign(_))
    ));

    std::fs::create_dir(root.join("campaign.ron")).unwrap();
    assert!(matches!(
        Campaign::open(&root),
        Err(CampaignError::NotACampaign(_))
    ));
}

// The version is refused on its own, before the rest of the file is understood —
// which is the whole reason the field exists, and only holds because the version is
// parsed first. The renamed field is the point: a v2 that drops `terrain` must
// still be refused for its version rather than for being unparseable.
#[test]
fn a_later_format_is_refused_for_its_version_not_its_shape() {
    let (_tmp, root) = root_with_terrain();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("campaign.ron"),
        r#"(version: 2, name: "x", terrain_dirs: ["terrain"], units_per_cell: 1.0, unit: "km")"#,
    )
    .unwrap();

    match Campaign::open(&root) {
        Err(CampaignError::UnsupportedVersion { found, expected }) => {
            assert_eq!((found, expected), (2, CAMPAIGN_VERSION));
        }
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

// A manifest that is read but is not RON for this type is told apart from one that
// could not be read at all.
#[test]
fn an_unparseable_manifest_is_malformed_not_unreadable() {
    let (_tmp, root) = root_with_terrain();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("campaign.ron"), "not ron at all").unwrap();

    assert!(matches!(
        Campaign::open(&root),
        Err(CampaignError::ManifestMalformed(_))
    ));
}

// A scale that cannot be compared to itself would make the round-trip invariant
// false, so the format refuses it rather than storing it.
#[test]
fn a_scale_that_is_not_a_positive_finite_number_is_refused() {
    for scale in ["NaN", "0.0", "-2.0", "inf"] {
        let (_tmp, root) = root_with_terrain();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("campaign.ron"),
            format!(
                r#"(version: 1, name: "x", terrain: "terrain", units_per_cell: {scale}, unit: "km")"#
            ),
        )
        .unwrap();

        assert!(
            matches!(
                Campaign::open(&root),
                Err(CampaignError::ManifestMalformed(_))
            ),
            "units_per_cell: {scale} was accepted"
        );
    }
}

// A manifest naming no terrain at all resolves to the campaign root, which would be
// reported as a confusing terrain error rather than as the malformed manifest it is.
#[test]
fn an_empty_terrain_field_is_malformed() {
    let (_tmp, root) = root_with_terrain();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("campaign.ron"),
        r#"(version: 1, name: "x", terrain: "", units_per_cell: 1.0, unit: "km")"#,
    )
    .unwrap();

    assert!(matches!(
        Campaign::open(&root),
        Err(CampaignError::ManifestMalformed(_))
    ));
}

// The three ways a terrain directory can be wrong stay distinguishable by what
// watershed said, which is the reason the error carries its `IoError`.
#[test]
fn a_rejected_terrain_reports_what_watershed_said() {
    let (_tmp, root) = root_with_terrain();
    Campaign::create(&root, "terrain").expect("create");

    let cases: Vec<(&str, Box<dyn Fn(&Path)>)> = vec![
        ("missing", Box::new(|dir: &Path| std::fs::remove_dir_all(dir).unwrap())),
        (
            "empty",
            Box::new(|dir: &Path| {
                std::fs::remove_dir_all(dir).unwrap();
                std::fs::create_dir_all(dir).unwrap();
            }),
        ),
        (
            "unparseable meta",
            Box::new(|dir: &Path| std::fs::write(dir.join("terrain.ron"), "junk").unwrap()),
        ),
    ];

    let mut seen = Vec::new();
    for (label, break_it) in cases {
        let (_tmp, root) = root_with_terrain();
        Campaign::create(&root, "terrain").expect("create");
        break_it(&root.join("terrain"));

        match Campaign::open(&root) {
            Err(CampaignError::TerrainUnreadable { source, .. }) => {
                seen.push((label, std::mem::discriminant(&source)));
            }
            other => panic!("{label}: expected TerrainUnreadable, got {other:?}"),
        }
    }

    let unparseable = seen
        .iter()
        .find(|(label, _)| *label == "unparseable meta")
        .unwrap();
    let missing = seen.iter().find(|(label, _)| *label == "missing").unwrap();
    assert_ne!(
        missing.1, unparseable.1,
        "a missing terrain and a malformed one must not collapse into one error"
    );
}

// Creating over an existing campaign refuses rather than overwrites, and leaves the
// manifest that was already there byte for byte.
#[test]
fn create_refuses_an_existing_campaign_without_touching_it() {
    let (_tmp, root) = root_with_terrain();
    Campaign::create(&root, "terrain").expect("create");
    let before = std::fs::read(root.join("campaign.ron")).unwrap();

    assert!(matches!(
        Campaign::create(&root, "terrain"),
        Err(CampaignError::AlreadyACampaign(_))
    ));
    assert_eq!(before, std::fs::read(root.join("campaign.ron")).unwrap());
}

// A terrain watershed refuses must leave nothing behind, or a mistyped path writes
// a campaign that `open` rejects and `create` then refuses to retry.
#[test]
fn create_writes_nothing_when_the_terrain_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("my-campaign");

    assert!(matches!(
        Campaign::create(&root, "terrain"),
        Err(CampaignError::TerrainUnreadable { .. })
    ));
    assert!(!root.exists(), "create left a directory behind");
}

// A terrain outside the campaign directory is legitimate: the manifest records
// where one is rather than owning it.
#[test]
fn a_terrain_opens_whether_its_path_is_relative_or_absolute() {
    let tmp = tempfile::tempdir().unwrap();
    let elsewhere = tmp.path().join("shared-terrain");
    write_loadable_terrain(&elsewhere);

    let root = tmp.path().join("absolute-campaign");
    let created = Campaign::create(&root, elsewhere.to_str().unwrap()).expect("create");
    assert_eq!(created.terrain_dir(), elsewhere);
    assert!(Campaign::open(&root).is_ok());

    let (_tmp2, relative_root) = root_with_terrain();
    Campaign::create(&relative_root, "terrain").expect("create");
    assert_eq!(
        Campaign::open(&relative_root).unwrap().terrain_dir(),
        relative_root.join("terrain")
    );
}

// Creating into a directory that already has content adopts it, which is what a GM
// pointing at an existing folder expects; a root that is a file cannot be adopted.
#[test]
fn create_adopts_an_existing_directory_but_not_a_file() {
    let (_tmp, root) = root_with_terrain();
    std::fs::create_dir_all(root.join("notes")).unwrap();
    std::fs::write(root.join("notes/already-here.md"), "keep me").unwrap();

    Campaign::create(&root, "terrain").expect("create into a populated directory");
    assert_eq!(
        std::fs::read_to_string(root.join("notes/already-here.md")).unwrap(),
        "keep me"
    );

    let tmp = tempfile::tempdir().unwrap();
    let terrain = tmp.path().join("terrain");
    write_loadable_terrain(&terrain);
    let file_root = tmp.path().join("a-file");
    std::fs::write(&file_root, "not a directory").unwrap();

    assert!(matches!(
        Campaign::create(&file_root, terrain.to_str().unwrap()),
        Err(CampaignError::LayoutUnwritable { .. })
    ));
}

// A manifest over the size cap is refused before it is read into memory.
#[test]
fn an_oversized_manifest_is_refused_before_it_is_read() {
    let (_tmp, root) = root_with_terrain();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("campaign.ron"),
        vec![b' '; (campaign::MAX_MANIFEST_BYTES + 1) as usize],
    )
    .unwrap();

    assert!(matches!(
        Campaign::open(&root),
        Err(CampaignError::ManifestUnreadable { .. })
    ));
}

// The manifest a caller builds by hand serializes to the same shape `create` writes,
// so a hand-edited `campaign.ron` and a generated one are the same format.
#[test]
fn a_hand_written_manifest_reads_back_equal() {
    let (_tmp, root) = root_with_terrain();
    std::fs::create_dir_all(&root).unwrap();
    let manifest = CampaignManifest {
        version: CAMPAIGN_VERSION,
        name: "Hand Written".to_owned(),
        terrain: "terrain".to_owned(),
        units_per_cell: 2.5,
        unit: "leagues".to_owned(),
    };
    std::fs::write(
        root.join("campaign.ron"),
        ron::ser::to_string_pretty(&manifest, ron::ser::PrettyConfig::default()).unwrap(),
    )
    .unwrap();

    assert_eq!(Campaign::open(&root).unwrap().manifest(), &manifest);
}

// An unknown field must not break a build that predates it, or the format cannot
// gain a field without breaking every saved campaign.
#[test]
fn an_unknown_field_is_ignored_rather_than_refused() {
    let (_tmp, root) = root_with_terrain();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("campaign.ron"),
        r#"(version: 1, name: "x", terrain: "terrain", units_per_cell: 1.0, unit: "km", revealed: true)"#,
    )
    .unwrap();

    assert!(Campaign::open(&root).is_ok());
}

// A `Campaign` has to cross a thread boundary, because the editor loads one off the
// render thread; a future field that broke that would otherwise only show up there.
#[test]
fn a_campaign_can_be_moved_to_another_thread() {
    fn assert_send<T: Send + 'static>() {}
    assert_send::<Campaign>();
    assert_send::<CampaignError>();
}

// `IoError` is re-exported through the error, so a caller can tell terrain failures
// apart without depending on watershed by name.
#[test]
fn the_terrain_error_carries_watersheds_own_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("c");
    match Campaign::create(&root, "nowhere") {
        Err(CampaignError::TerrainUnreadable { source, .. }) => {
            assert!(matches!(source, IoError::Io(_)));
        }
        other => panic!("expected TerrainUnreadable, got {other:?}"),
    }
}
