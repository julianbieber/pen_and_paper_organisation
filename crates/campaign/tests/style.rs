//! The styling table: that it answers for everything, and that the constraints the
//! renderer relies on hold for every entry.

use std::collections::BTreeSet;

use campaign::feature::{FeatureKind, Rank};
use campaign::style::{self, Dash, Stroke};

// The table is the only thing standing between a new FeatureKind and a feature that draws
// as nothing at all. It must answer for every kind at every rank, including no rank.
#[test]
fn every_kind_and_rank_has_a_style() {
    for kind in FeatureKind::all() {
        for rank in [None].into_iter().chain(Rank::all().map(Some)) {
            let style = style::of(kind, rank);
            assert!(
                style.alpha > 0.0,
                "{kind:?} at {rank:?} is styled invisible, which is indistinguishable from not being drawn"
            );
            for channel in [style.red, style.green, style.blue, style.alpha] {
                assert!(
                    (0.0..=1.0).contains(&channel),
                    "{kind:?} at {rank:?} has a colour channel of {channel} outside zero to one"
                );
            }
        }
    }
}

// FeatureKind::all() is what the property panel offers. A kind missing from it can be
// drawn but never chosen, which is worse than one that fails to compile.
#[test]
fn every_kind_is_offered_exactly_once() {
    let offered: BTreeSet<String> = FeatureKind::all()
        .iter()
        .map(|kind| format!("{kind:?}"))
        .collect();
    assert_eq!(
        offered.len(),
        FeatureKind::all().len(),
        "a kind appears twice in the chooser"
    );
    assert!(offered.contains("Settlement") && offered.contains("Poi"));
}

// A dash period is a multiple of the pen's own width, and the river's width is rewritten
// from the zoom every frame — so a dashed river would change its dash length as the camera
// moved. This is the invariant that makes that unrepresentable.
#[test]
fn the_river_pen_is_never_dashed() {
    assert_eq!(Stroke::River.dash(), Dash::Solid);
}

// Stroke::all() is the paint order the renderer registers its configuration groups in. A
// pen missing from it is a pen that is never registered, and drawing through an
// unregistered group is a panic rather than a missing line.
#[test]
fn every_pen_appears_in_the_paint_order_exactly_once() {
    let order = Stroke::all();
    let unique: BTreeSet<Stroke> = order.iter().copied().collect();
    assert_eq!(unique.len(), order.len(), "a pen appears twice in the paint order");

    for stroke in order {
        assert!(
            stroke.width_pixels() > 0.0,
            "{stroke:?} has no width, so it draws nothing"
        );
    }
}

// A settlement's icon and label priority are the whole of what its rank buys it, so a
// bigger settlement must actually outrank a smaller one at both.
#[test]
fn a_larger_settlement_outranks_a_smaller_one() {
    let hamlet = style::of(FeatureKind::Settlement, Some(Rank::Hamlet));
    let town = style::of(FeatureKind::Settlement, Some(Rank::Town));
    let city = style::of(FeatureKind::Settlement, Some(Rank::City));

    assert!(hamlet.label_priority < town.label_priority);
    assert!(town.label_priority < city.label_priority);

    assert!(style::icon_pixels(Some(Rank::Hamlet)) < style::icon_pixels(Some(Rank::Town)));
    assert!(style::icon_pixels(Some(Rank::Town)) < style::icon_pixels(Some(Rank::City)));
}

// A rank on a road has no meaning, and the decision was to ignore it rather than refuse
// it — so the table must answer identically with and without one, or a stray rank would
// silently restyle a feature.
#[test]
fn a_rank_changes_nothing_for_a_kind_that_is_not_a_settlement() {
    for kind in FeatureKind::all() {
        if kind == FeatureKind::Settlement {
            continue;
        }
        let plain = style::of(kind, None);
        for rank in Rank::all() {
            assert_eq!(
                style::of(kind, Some(rank)),
                plain,
                "{kind:?} was restyled by a {rank:?} it has no use for"
            );
        }
    }
}

// Only a point is drawn as an icon, and only a polygon can carry a fill — but the table
// does not know a feature's geometry, so what it must guarantee is that the two kinds that
// are always polygons are the only ones asking for a fill.
#[test]
fn only_the_region_kinds_ask_for_a_fill() {
    for kind in FeatureKind::all() {
        let fill = style::of(kind, None).fill;
        let expected = matches!(kind, FeatureKind::Landcover);
        assert_eq!(
            fill.is_some(),
            expected,
            "{kind:?} asks for a fill it should not, or lacks one it should have"
        );
        if let Some(fill) = fill {
            assert!(fill.spacing_pixels > 0.0, "{kind:?} hatches at zero spacing");
            assert!(fill.angle.is_finite(), "{kind:?} hatches at a non-finite angle");
        }
    }
}
