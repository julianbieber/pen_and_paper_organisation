//! What an image backdrop refuses, where it puts a picture, and the one property the
//! whole feature rests on: after a calibration, the two places the GM marked really are
//! the distance apart they said.

use campaign::feature::{CellPoint, dungeon_name_refusal};
use campaign::image::{self, ImageBackdrop, ImageProblem, ImportError};
use campaign::layout;
use campaign::world::World;

fn backdrop(cells_per_pixel: f32) -> ImageBackdrop {
    ImageBackdrop::new(
        "plan.png",
        CellPoint::new(3.0, -4.0),
        cells_per_pixel,
        1.0,
    )
    .expect("a legal declaration")
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    ::image::DynamicImage::ImageRgba8(::image::RgbaImage::new(width, height))
        .write_to(&mut bytes, ::image::ImageFormat::Png)
        .expect("encode a png");
    bytes.into_inner()
}

// A cursor lands on a pixel and a pixel lands back on the cursor, or a calibration mark
// cannot be turned into the image coordinate the scale is computed from.
#[test]
fn a_pixel_survives_the_trip_through_the_document() {
    let placed = backdrop(0.25);
    for (x, y) in [(0.0, 0.0), (17.0, 3.0), (-9.5, 512.25)] {
        let cell = placed.cell_of_pixel(x, y);
        let (back_x, back_y) = placed.pixel_of_cell(cell);
        assert!(
            (back_x - x).abs() < 0.001 && (back_y - y).abs() < 0.001,
            "({x}, {y}) came back as ({back_x}, {back_y})"
        );
    }
}

// The headline invariant, on the world map where a cell is one unit: after calibrating,
// the two marks are exactly the distance apart the GM gave.
#[test]
fn calibration_puts_the_two_marks_the_given_distance_apart() {
    let placed = backdrop(1.0);
    let first = (100.0, 100.0);
    let second = (500.0, 400.0);

    let scale = image::calibrate(first, second, 50.0, 1.0).expect("a legal calibration");
    let calibrated = placed.placed(placed.origin(), scale).expect("a legal scale");

    let a = calibrated.cell_of_pixel(first.0, first.1);
    let b = calibrated.cell_of_pixel(second.0, second.1);
    let apart = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
    assert!(
        (apart - 50.0).abs() < 0.001,
        "the marks came out {apart} cells apart, not 50"
    );
}

// And in a dungeon, where a cell is worth metres rather than one: the same distance must
// give a different scale, or every dungeon backdrop is out by the grid's own factor.
#[test]
fn calibration_measures_in_the_documents_own_unit() {
    let first = (0.0, 0.0);
    let second = (100.0, 0.0);

    let on_the_map = image::calibrate(first, second, 30.0, 1.0).expect("a legal calibration");
    let in_a_dungeon = image::calibrate(first, second, 30.0, 1.5).expect("a legal calibration");

    assert!(
        (on_the_map / in_a_dungeon - 1.5).abs() < 0.001,
        "a cell worth 1.5 must give a scale 1.5x smaller, got {on_the_map} and {in_a_dungeon}"
    );
}

// A rescale has to hold one point still, or the landmarks the GM lined up slide out from
// under the features drawn on them.
#[test]
fn an_anchored_rescale_leaves_its_pixel_where_it_was() {
    let placed = backdrop(0.5);
    let pixel = (64.0, 32.0);
    let was = placed.cell_of_pixel(pixel.0, pixel.1);

    let origin = placed.anchored(pixel.0, pixel.1, was, 0.125);
    let rescaled = placed.placed(origin, 0.125).expect("a legal scale");

    let now = rescaled.cell_of_pixel(pixel.0, pixel.1);
    assert!(
        (now.x - was.x).abs() < 0.001 && (now.y - was.y).abs() < 0.001,
        "the anchor moved from {was} to {now}"
    );
}

// Two marks on the same pixel fix no scale at all, and dividing by their separation would
// hand back an infinity that fails inside the renderer instead of here.
#[test]
fn a_calibration_between_two_identical_marks_is_refused() {
    let refusal = image::calibrate((10.0, 10.0), (10.0, 10.0), 5.0, 1.0);
    assert!(matches!(refusal, Err(ImageProblem::MarksCoincide)), "{refusal:?}");
}

// A legal distance over a separation small enough still produces a scale no document may
// hold; the check has to be on the answer, not only on the inputs.
#[test]
fn a_calibration_whose_answer_overflows_is_refused() {
    let refusal = image::calibrate((0.0, 0.0), (1.0e-5, 0.0), 1.0e30, 1.0);
    assert!(matches!(refusal, Err(ImageProblem::BadScale { .. })), "{refusal:?}");
}

// Two marks close enough that the square of their separation underflows to zero read as
// the same point rather than as an infinite scale, which is the answer that would reach
// the renderer.
#[test]
fn two_marks_too_close_to_square_are_refused_as_coincident() {
    let refusal = image::calibrate((0.0, 0.0), (1.0e-30, 0.0), 5.0, 1.0);
    assert!(matches!(refusal, Err(ImageProblem::MarksCoincide)), "{refusal:?}");
}

// The distance is the one number the GM types, so it is the one that arrives as anything
// at all; each of these divides into a scale the renderer could not use.
#[test]
fn a_distance_that_is_not_a_finite_positive_number_is_refused() {
    for distance in [0.0, -3.0, f32::NAN, f32::INFINITY] {
        let refusal = image::calibrate((0.0, 0.0), (10.0, 0.0), distance, 1.0);
        assert!(
            matches!(refusal, Err(ImageProblem::BadDistance { .. })),
            "a distance of {distance} was accepted"
        );
    }
}

// The name is the trust boundary: it is joined onto a directory this tool writes into, so
// every one of these has to be refused where it is stored rather than where it is opened.
#[test]
fn a_file_name_a_document_may_not_carry_is_refused() {
    for name in [
        "",
        "..",
        ".",
        "-rf.png",
        "over/there.png",
        "over\\there.png",
        "data:plan.png",
        "plan.png#label",
        "plan",
        "plan.ron",
        "plan.exe",
    ] {
        assert!(
            image::name_refusal(name).is_some(),
            "`{name}` was accepted as an image file name"
        );
    }
    for name in ["plan.png", "scan-2.jpg", "a.jpeg", "Plan.PNG"] {
        assert!(
            image::name_refusal(name).is_none(),
            "`{name}` was refused: {:?}",
            image::name_refusal(name)
        );
    }
}

// The shared rule now backs both names, so the dungeon's own refusals must be exactly what
// they were — this is the regression that extraction could silently cause.
#[test]
fn a_dungeon_name_is_refused_for_everything_it_was_before() {
    for name in ["", "..", ".", "-x.ron", "a/b.ron", "a\\b.ron", "plan", "plan.png"] {
        assert!(
            dungeon_name_refusal(name).is_some(),
            "`{name}` was accepted as a dungeon name"
        );
    }
    assert!(dungeon_name_refusal("riverford-a1b2.ron").is_none());
}

// The four fields a declaration carries, each at the value that would make the picture
// undrawable — this is the rule `World::check` and `Edit::apply` both enforce, checked at
// the one place it is written.
#[test]
fn a_declaration_that_could_not_be_drawn_is_refused() {
    let at = CellPoint::new(0.0, 0.0);
    assert!(ImageBackdrop::new("plan.png", at, 0.0, 1.0).is_err());
    assert!(ImageBackdrop::new("plan.png", at, -1.0, 1.0).is_err());
    assert!(ImageBackdrop::new("plan.png", at, f32::NAN, 1.0).is_err());
    assert!(ImageBackdrop::new("plan.png", at, 1.0e30, 1.0).is_err());
    assert!(ImageBackdrop::new("plan.png", at, 1.0, 1.5).is_err());
    assert!(ImageBackdrop::new("plan.png", at, 1.0, -0.1).is_err());
    assert!(ImageBackdrop::new("plan.png", CellPoint::new(f32::NAN, 0.0), 1.0, 1.0).is_err());
    assert!(ImageBackdrop::new("plan.png", at, 1.0, 0.0).is_ok());
}

// A picture arrives visible, centred and whole, or the GM has to hunt for it at whatever
// scale it happened to land at.
#[test]
fn a_fitted_picture_is_centred_and_inside_the_extent() {
    let (origin, scale) = ImageBackdrop::fit_to(64, 64, (200, 100));
    let placed = ImageBackdrop::new("plan.png", origin, scale, 1.0).expect("a legal fit");
    let far = placed.far_corner((200, 100));

    assert!(origin.x >= -0.001 && origin.y >= -0.001, "{origin} starts outside");
    assert!(far.x <= 64.001 && far.y <= 64.001, "{far} reaches outside");
    assert!(
        (origin.x - (64.0 - far.x)).abs() < 0.001,
        "the margins across are {} and {}",
        origin.x,
        64.0 - far.x
    );
    assert!(
        (origin.y - (64.0 - far.y)).abs() < 0.001,
        "the margins down are {} and {}",
        origin.y,
        64.0 - far.y
    );
}

// A hand-edited document naming a picture that could never be drawn must be refused with
// the same sentence an edit would give, not opened and drawn wrong.
#[test]
fn a_document_carrying_an_undrawable_declaration_is_refused() {
    let text = r#"(
        version: 1,
        next_id: 0,
        features: {},
        image: Some((
            file: "../secrets.png",
            origin: (x: 0.0, y: 0.0),
            cells_per_pixel: 1.0,
            opacity: 1.0,
        )),
    )"#;
    let refused = World::from_ron(text, std::path::Path::new("world.ron"));
    assert!(refused.is_err(), "an escaping file name was accepted");
}

// The file's readability is a different question from the declaration's legality, and the
// acceptance criterion turns on it: a picture nobody can find must still leave a document
// that opens with its features intact.
#[test]
fn a_document_naming_a_picture_that_is_not_there_still_opens() {
    let text = r#"(
        version: 1,
        next_id: 0,
        features: {},
        image: Some((
            file: "nowhere.png",
            origin: (x: 1.0, y: 2.0),
            cells_per_pixel: 0.5,
            opacity: 0.8,
        )),
    )"#;
    let world = World::from_ron(text, std::path::Path::new("world.ron")).expect("it should open");
    assert_eq!(world.image().expect("the declaration").file(), "nowhere.png");
}

// A document written before backdrops existed still has to read, because WORLD_VERSION
// stays 1 and there is no migration path.
#[test]
fn a_document_written_before_backdrops_reads_with_none() {
    let text = "(version: 1, next_id: 0, features: {})";
    let world = World::from_ron(text, std::path::Path::new("world.ron")).expect("it should open");
    assert!(world.image().is_none());
    assert!(!world.to_ron().expect("serialize").contains("image"));
}

// A world this build writes is one it reads back, which the save-size refusal exists to
// guarantee — a declaration that serialized to something `from_ron` rejects would break it
// on the first reopen rather than at the save.
#[test]
fn a_document_carrying_a_picture_round_trips() {
    let mut world = World::default();
    campaign::edit::Edit::SetImage {
        image: Some(backdrop(0.25)),
    }
    .apply(&mut world)
    .expect("declaring an image");

    let text = world.to_ron().expect("serialize");
    let back = World::from_ron(&text, std::path::Path::new("world.ron")).expect("read back");
    assert_eq!(back, world);
}

// Import is what makes the campaign directory portable, so the file has to actually land
// inside it under a name the document may carry.
#[test]
fn importing_puts_the_file_in_the_campaign_and_names_it() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let source = root.path().join("My Scan!.png");
    std::fs::write(&source, png(8, 8)).expect("write the source");

    let name = image::import(root.path(), &source).expect("the import").name;

    assert_eq!(name, "my-scan.png");
    assert!(image::name_refusal(&name).is_none(), "the name it chose is not storable");
    assert!(layout::image(root.path(), &name).is_file(), "the file did not land");
}

// Two imports of the same picture must not silently become one file, or re-importing
// after an undo would overwrite the picture the earlier document still names.
#[test]
fn a_second_import_takes_a_name_of_its_own() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let source = root.path().join("scan.png");
    std::fs::write(&source, png(4, 4)).expect("write the source");

    let first = image::import(root.path(), &source).expect("the first import").name;
    let second = image::import(root.path(), &source).expect("the second import").name;

    assert_eq!(first, "scan.png");
    assert_eq!(second, "scan-2.png");
    assert!(layout::image(root.path(), &first).is_file());
    assert!(layout::image(root.path(), &second).is_file());
}

// The format is decided by the bytes, so a picture misnamed as something else still
// imports, and a text file called .png does not.
#[test]
fn the_extension_comes_from_the_bytes_and_not_from_the_name() {
    let root = tempfile::tempdir().expect("a temporary directory");

    let misnamed = root.path().join("scan.txt");
    std::fs::write(&misnamed, png(2, 2)).expect("write the source");
    assert_eq!(
        image::import(root.path(), &misnamed).expect("the import").name,
        "scan.png"
    );

    let lying = root.path().join("notes.png");
    std::fs::write(&lying, b"this is not a picture").expect("write the source");
    assert!(matches!(
        image::import(root.path(), &lying),
        Err(ImportError::NotAnImage { .. })
    ));
}

// Reading a FIFO would hang for the life of the process, which is why world.rs refuses
// anything that is not a regular file and why this must too.
#[test]
fn importing_something_that_is_not_a_file_is_refused() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let directory = root.path().join("a-directory");
    std::fs::create_dir(&directory).expect("make it");

    assert!(matches!(
        image::import(root.path(), &directory),
        Err(ImportError::NotAFile { .. })
    ));
    assert!(matches!(
        image::import(root.path(), &root.path().join("nothing.png")),
        Err(ImportError::NotAFile { .. })
    ));
}

// A refused import must leave nothing behind, or a document could later name a file that
// was only half written.
#[test]
fn a_refused_import_leaves_no_file_in_the_campaign() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let lying = root.path().join("notes.png");
    std::fs::write(&lying, b"not a picture").expect("write the source");

    let _ = image::import(root.path(), &lying);

    let images = layout::images(root.path());
    let left = images
        .read_dir()
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(left, 0, "the refused import left something behind");
}
