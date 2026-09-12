//! What a campaign directory admits and refuses, exercised end to end against real
//! directories on disk — including a real `watershed` terrain, because that is what
//! `Campaign::open` demands.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use campaign::repo::GitError;
use campaign::{CAMPAIGN_VERSION, Campaign, CampaignError, CampaignManifest, GitRunner};
use glam::UVec2;
use watershed::{ChannelMeta, FieldInfo, FieldRole, IoError, LayerTexels, Terrain, TerrainLayer};

struct NoGit;

impl GitRunner for NoGit {
    fn run(&self, _dir: &Path, _args: &[OsString]) -> Result<std::process::Output, GitError> {
        Err(GitError::GitMissing)
    }
}

fn create(root: &Path, terrain: impl AsRef<Path>) -> Result<Campaign, CampaignError> {
    Campaign::create(root, terrain, &NoGit).map(|created| created.campaign)
}

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

    std::fs::create_dir_all(dir.join("shaders")).expect("shaders dir");
    std::fs::write(dir.join("shaders/tint.wgsl"), b"// fixture shader").expect("write shader");

    Terrain::load_from_dir(dir).expect("the fixture terrain must load, or it tests nothing");
}

fn root_with_terrain() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path().join("my-campaign");
    write_loadable_terrain(&root.join("terrain"));
    (tmp, root)
}

fn tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut worklist = vec![PathBuf::new()];
    while let Some(relative) = worklist.pop() {
        for entry in std::fs::read_dir(dir.join(&relative)).expect("read the directory") {
            let entry = entry.expect("a directory entry");
            let relative_path = relative.join(entry.file_name());
            if entry.file_type().expect("file type").is_dir() {
                worklist.push(relative_path);
            } else {
                let bytes = std::fs::read(entry.path()).unwrap_or_default();
                out.push((relative_path.to_string_lossy().into_owned(), bytes));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

// The acceptance round trip: what `create` writes is what `open` reads back.
#[test]
fn create_then_open_round_trips_the_manifest() {
    let (_tmp, root) = root_with_terrain();

    let created = create(&root, root.join("terrain")).expect("create");
    let opened = Campaign::open(&root).expect("open");

    assert_eq!(created.manifest(), opened.manifest());
    assert_eq!(opened.manifest().version, CAMPAIGN_VERSION);
    assert_eq!(opened.manifest().terrain, "terrain");
    assert_eq!(opened.manifest().name, "my-campaign");
}

// `create` makes exactly the layout it promises, and the terrain.
#[test]
fn create_writes_the_layout_and_the_terrain() {
    let (_tmp, root) = root_with_terrain();
    create(&root, root.join("terrain")).expect("create");

    for subdir in ["dungeons", "images", "notes"] {
        assert!(root.join(subdir).is_dir(), "{subdir} was not created");
    }
    assert!(root.join("terrain").is_dir(), "terrain was not created");
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

// Creating around an existing export — the source is `<root>/terrain` itself —
// adopts it rather than copying it onto itself.
#[test]
fn create_adopts_the_terrain_directory_in_place() {
    let (_tmp, root) = root_with_terrain();
    let terrain = root.join("terrain");

    let before = tree(&terrain);
    create(&root, &terrain).expect("create");
    let after = tree(&terrain);

    assert_eq!(before, after);
    assert!(!terrain.join("terrain").exists());
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
    create(&root, root.join("terrain")).expect("create");

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
        create(&root, root.join("terrain")).expect("create");
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
    create(&root, root.join("terrain")).expect("create");
    let before = std::fs::read(root.join("campaign.ron")).unwrap();

    assert!(matches!(
        create(&root, root.join("terrain")),
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
    let missing_source = tmp.path().join("does-not-exist");

    assert!(matches!(
        create(&root, &missing_source),
        Err(CampaignError::TerrainUnreadable { .. })
    ));
    assert!(!root.exists(), "create left a directory behind");
}

// A terrain outside the campaign directory is legitimate: `open` still reads a
// manifest naming one there, the manifest records where one is rather than owning
// it — though such a campaign no longer travels with its directory.
#[test]
fn a_terrain_opens_whether_its_path_is_relative_or_absolute() {
    let tmp = tempfile::tempdir().unwrap();
    let elsewhere = tmp.path().join("shared-terrain");
    write_loadable_terrain(&elsewhere);

    let root = tmp.path().join("outside-campaign");
    std::fs::create_dir_all(&root).unwrap();
    let manifest = CampaignManifest {
        version: CAMPAIGN_VERSION,
        name: "x".to_owned(),
        terrain: elsewhere.to_str().unwrap().to_owned(),
        units_per_cell: 1.0,
        unit: "km".to_owned(),
    };
    std::fs::write(
        root.join("campaign.ron"),
        ron::ser::to_string_pretty(&manifest, ron::ser::PrettyConfig::default()).unwrap(),
    )
    .unwrap();

    let opened = Campaign::open(&root).expect("open a manifest naming a terrain elsewhere");
    assert_eq!(opened.terrain_dir(), elsewhere);
    assert!(!opened.terrain_travels());

    assert!(
        !CampaignManifest {
            terrain: "../shared".to_owned(),
            ..manifest.clone()
        }
        .terrain_travels()
    );
    assert!(
        CampaignManifest {
            terrain: "terrain".to_owned(),
            ..manifest
        }
        .terrain_travels()
    );
}

// Creating into a directory that already has content adopts it, which is what a GM
// pointing at an existing folder expects; a root that is a file cannot be adopted.
#[test]
fn create_adopts_an_existing_directory_but_not_a_file() {
    let (_tmp, root) = root_with_terrain();
    std::fs::create_dir_all(root.join("notes")).unwrap();
    std::fs::write(root.join("notes/already-here.md"), "keep me").unwrap();

    create(&root, root.join("terrain")).expect("create into a populated directory");
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
        create(&file_root, &terrain),
        Err(CampaignError::LayoutUnwritable { .. })
    ));
}

// The acceptance observation itself: what `create` copies is a full, working
// duplicate that survives the source going away and the whole directory moving.
#[test]
fn create_copies_the_terrain_so_the_directory_travels() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("shared-terrain");
    write_loadable_terrain(&source);

    let root = tmp.path().join("a");
    create(&root, &source).expect("create");

    assert_eq!(tree(&root.join("terrain")), tree(&source));
    assert_eq!(CampaignManifest::read(&root).unwrap().terrain, "terrain");

    std::fs::remove_dir_all(&source).unwrap();
    let root2 = tmp.path().join("b");
    std::fs::rename(&root, &root2).unwrap();

    assert!(
        Campaign::open(&root2).is_ok(),
        "the moved campaign must still open"
    );
}

// A `terrain` directory already there and unrelated to the source is not silently
// merged into or replaced.
#[test]
fn create_refuses_a_terrain_directory_already_in_the_way() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("shared-terrain");
    write_loadable_terrain(&source);

    let root = tmp.path().join("a");
    std::fs::create_dir_all(root.join("terrain")).unwrap();
    std::fs::write(root.join("terrain/unrelated.txt"), "not the source").unwrap();

    assert!(matches!(
        create(&root, &source),
        Err(CampaignError::TerrainInTheWay(_))
    ));
    assert!(!root.join("campaign.ron").exists());
}

// The root sitting inside the source would have the copy walk into its own
// destination; refused rather than looping.
#[test]
fn create_refuses_a_root_nested_inside_the_source() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("shared-terrain");
    write_loadable_terrain(&source);
    let root = source.join("nested-campaign");

    assert!(matches!(
        create(&root, &source),
        Err(CampaignError::TerrainNotCopyable { .. })
    ));
}

// A second create over an existing campaign must not copy a different terrain over
// the one already there before it notices and refuses.
#[test]
fn create_over_an_existing_campaign_copies_nothing() {
    let (_tmp, root) = root_with_terrain();
    create(&root, root.join("terrain")).expect("create");
    let before = tree(&root.join("terrain"));

    let tmp2 = tempfile::tempdir().unwrap();
    let other_source = tmp2.path().join("other-terrain");
    write_loadable_terrain(&other_source);

    assert!(matches!(
        create(&root, &other_source),
        Err(CampaignError::AlreadyACampaign(_))
    ));
    assert_eq!(before, tree(&root.join("terrain")));
}

// A copy that meets something it cannot copy must not leave a half-written terrain
// directory behind.
#[test]
fn create_leaves_no_terrain_directory_when_the_copy_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("shared-terrain");
    write_loadable_terrain(&source);
    std::os::unix::fs::symlink(source.join("terrain.ron"), source.join("a-symlink")).unwrap();

    let root = tmp.path().join("a");
    match create(&root, &source) {
        Err(CampaignError::TerrainNotCopyable { .. }) => {}
        other => panic!("expected TerrainNotCopyable, got {other:?}"),
    }
    assert!(!root.join("terrain").exists());
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
    let missing_source = tmp.path().join("nowhere");
    match create(&root, &missing_source) {
        Err(CampaignError::TerrainUnreadable { source, .. }) => {
            assert!(matches!(source, IoError::Io(_)));
        }
        other => panic!("expected TerrainUnreadable, got {other:?}"),
    }
}

// The fourth acceptance criterion, at the level this crate can check it without a
// window: a campaign created with no `git` is still openable, and `create` reports the
// repository as refused rather than silently succeeding or refusing the campaign.
#[test]
fn create_with_no_git_is_still_an_openable_campaign() {
    let (_tmp, root) = root_with_terrain();

    let created = Campaign::create(&root, root.join("terrain"), &NoGit).expect("create");
    assert!(matches!(created.repository, Err(GitError::GitMissing)));

    let opened = Campaign::open(&root).expect("a campaign with no repository still opens");
    assert_eq!(opened.manifest(), created.campaign.manifest());
}

// The ignore file is what stops a committed campaign churning on its own queries and
// saves, so it must not depend on `git` being there to write it.
#[test]
fn the_ignore_file_is_written_even_when_git_is_missing() {
    let (_tmp, root) = root_with_terrain();
    Campaign::create(&root, root.join("terrain"), &NoGit).expect("create");

    let ignore = std::fs::read_to_string(root.join(".gitignore")).expect("read .gitignore");
    assert!(ignore.contains("notes/.zk/notebook.db*"), "{ignore}");
    assert!(ignore.contains("*.tmp"), "{ignore}");
}

// The GM's scale is what every figure on screen is derived from, so it has to survive the
// round trip through campaign.ron rather than being held only in memory.
#[test]
fn a_rescale_reaches_the_manifest_on_disk() {
    let (_tmp, root) = root_with_terrain();
    let mut campaign = create(&root, root.join("terrain")).expect("create");

    campaign.rescale(12.5, "miles").expect("a legal scale");
    assert_eq!(campaign.manifest().units_per_cell, 12.5);
    assert_eq!(campaign.manifest().unit, "miles");

    let reread = CampaignManifest::read(&root).expect("read it back");
    assert_eq!(reread.units_per_cell, 12.5);
    assert_eq!(reread.unit, "miles");
}

// `read` promises a later additive field stays readable, and re-serializing the parsed
// struct would quietly destroy exactly that.
#[test]
fn a_rescale_keeps_a_field_this_build_does_not_know() {
    let (_tmp, root) = root_with_terrain();
    create(&root, root.join("terrain")).expect("create");

    let path = root.join("campaign.ron");
    let text = std::fs::read_to_string(&path).expect("read");
    let widened = text.replace("version: 1,", "version: 1,\n    weather: \"rain\",");
    std::fs::write(&path, widened).expect("write");

    CampaignManifest::write_scale(&root, 3.0, "leagues").expect("a legal scale");

    let after = std::fs::read_to_string(&path).expect("read");
    assert!(after.contains("weather"), "{after}");
    assert!(after.contains("leagues"), "{after}");
}

// A refused scale must leave the file exactly as it was, not half written.
#[test]
fn a_refused_rescale_changes_nothing() {
    let (_tmp, root) = root_with_terrain();
    let mut campaign = create(&root, root.join("terrain")).expect("create");
    let before = std::fs::read_to_string(root.join("campaign.ron")).expect("read");

    let refusal = campaign.rescale(0.0, "km").expect_err("zero is not a scale");
    assert!(matches!(refusal, CampaignError::ManifestMalformed(_)));

    let after = std::fs::read_to_string(root.join("campaign.ron")).expect("read");
    assert_eq!(before, after);
    assert_eq!(campaign.manifest().units_per_cell, 1.0);
}

// The write goes through a temporary file, which must not be left behind.
#[test]
fn a_rescale_leaves_no_temporary_file() {
    let (_tmp, root) = root_with_terrain();
    let mut campaign = create(&root, root.join("terrain")).expect("create");
    campaign.rescale(2.0, "km").expect("a legal scale");

    let names: Vec<String> = std::fs::read_dir(&root)
        .expect("read dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!names.iter().any(|name| name.contains("writing")), "{names:?}");
}
