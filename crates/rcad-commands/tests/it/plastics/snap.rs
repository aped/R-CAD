//! Snap Fit: hook sections, stand-off hooks, grooves and loops.
use super::*;

const PLATE: f64 = 60.0 * 40.0 * 4.0;

fn setup() -> (Session, FeatureId) {
    let mut s = Session::new();
    ok(&mut s, Command::NewDocument);
    let e = plate(&mut s, SketchPlane::WorldXY, 60.0, 40.0, 4.0);
    (s, e)
}

/// Section area above the root of the default hook (thickness 1, length 8,
/// catch 0.7 deep and 2 high, insertion 30°, retention 90°).
fn hook_area(beam_above: f64) -> f64 {
    let v2 = 8.0 - 0.7 / 30f64.to_radians().tan();
    beam_above * 1.0 + 0.7 * (v2 - 6.0) + 0.35 * (8.0 - v2)
}

const HOOK: [(&str, f64); 6] = [
    ("thickness", 1.0),
    ("tip_thickness", 1.0),
    ("catch_depth", 0.7),
    ("catch_height", 2.0),
    ("length", 8.0),
    ("width", 5.0),
];

#[test]
fn a_perpendicular_hook_has_the_section_area_times_its_width() {
    let (mut s, e) = setup();
    let sk = points_sketch(&mut s, end_cap(e), &[[0.0, 0.0], [20.0, 10.0]]);
    ok(
        &mut s,
        feature("snap_fit", &[("sketch", sketch_in(sk))], &HOOK, &[]),
    );
    assert_close(
        vol(&s, BodyId(1)),
        PLATE + 2.0 * 5.0 * hook_area(8.0),
        1e-6,
        "volume",
    );
    // Beam, catch ledge (z = 4 + 6), above the ledge, outside the catch.
    let pr = probe(
        &s,
        BodyId(1),
        &[
            [0.5, 0.0, 8.0],
            [1.6, 0.0, 10.1],
            [1.6, 0.0, 9.9],
            [1.8, 0.0, 10.1],
        ],
    );
    assert_eq!(pr, [Some(true), Some(true), Some(false), Some(false)]);
}

#[test]
fn a_parallel_hook_stands_off_the_next_face() {
    let (mut s, e) = setup();
    let plane = SketchPlane::Offset {
        base: rcad_document::PlaneBase::Face(FaceRef {
            feature: e,
            role: FaceRole::ExtrudeEndCap,
        }),
        distance: 3.0,
        angle: 0.0,
    };
    let sk = points_sketch(&mut s, plane, &[[0.0, 0.0]]);
    ok(
        &mut s,
        feature(
            "snap_fit",
            &[("sketch", sketch_in(sk))],
            &HOOK,
            &[("orientation", "parallel")],
        ),
    );
    // The beam continues 3 mm down to the plate.
    assert_close(
        vol(&s, BodyId(1)),
        PLATE + 5.0 * hook_area(11.0),
        1e-6,
        "volume",
    );
}

#[test]
fn grooves_and_loops_go_on_the_mating_body() {
    // Groove: a block beside the hook (x 1.5..5, z 6..14) is cut by the
    // catch envelope + 0.1 mm: x 1.5..1.8, z 9.9..12.1, y ±2.6.
    let (mut s, e) = setup();
    let sk = points_sketch(&mut s, end_cap(e), &[[0.0, 0.0]]);
    let blk = ok(
        &mut s,
        Command::CreateSketch {
            name: None,
            plane: offset_xy(6.0),
            template: SketchTemplate::CenteredRectangle {
                width: 3.5,
                height: 20.0,
                center: Some([3.25, 0.0]),
            },
        },
    )
    .feature
    .unwrap();
    ok(&mut s, Command::extrude(None, blk, 8.0));
    let mate = PluginInput::Body { body: BodyId(2) };
    ok(
        &mut s,
        feature(
            "snap_fit",
            &[("sketch", sketch_in(sk)), ("mate", mate.clone())],
            &HOOK,
            &[("type", "hook_and_groove")],
        ),
    );
    assert_close(
        vol(&s, BodyId(2)),
        3.5 * 20.0 * 8.0 - 0.3 * 2.2 * 5.2,
        1e-6,
        "grooved block",
    );

    // Loop: a bar under the ledge (z 8.4..9.9) from x 1.1 to 4.7, y ±4.1,
    // merging into a block at x 3..6.
    let (mut s, e) = setup();
    let sk = points_sketch(&mut s, end_cap(e), &[[0.0, 0.0]]);
    let blk = ok(
        &mut s,
        Command::CreateSketch {
            name: None,
            plane: offset_xy(4.5),
            template: SketchTemplate::CenteredRectangle {
                width: 3.0,
                height: 20.0,
                center: Some([4.5, 0.0]),
            },
        },
    )
    .feature
    .unwrap();
    ok(&mut s, Command::extrude(None, blk, 9.5));
    ok(
        &mut s,
        feature(
            "snap_fit",
            &[("sketch", sketch_in(sk)), ("mate", mate)],
            &HOOK,
            &[("type", "hook_and_loop")],
        ),
    );
    assert_close(
        vol(&s, BodyId(2)),
        3.0 * 20.0 * 9.5 + 1.9 * 1.5 * 8.2,
        1e-6,
        "looped block",
    );
}

#[test]
fn impossible_catches_are_refused() {
    let (mut s, e) = setup();
    let sk = points_sketch(&mut s, end_cap(e), &[[0.0, 0.0]]);
    let mut p = HOOK.to_vec();
    p.push(("catch_height", 0.5));
    p.retain(|(n, v)| *n != "catch_height" || *v == 0.5);
    let m = err(
        &mut s,
        feature(
            "snap_fit",
            &[("sketch", sketch_in(sk))],
            &p,
            &[("type", "hook_and_groove")],
        ),
    );
    assert!(m.contains("too short for its angles"), "{m}");
    let m = err(
        &mut s,
        feature(
            "snap_fit",
            &[("sketch", sketch_in(sk))],
            &[("catch_height", 9.0)],
            &[],
        ),
    );
    assert!(m.contains("shorter than the beam"), "{m}");
    assert_close(vol(&s, BodyId(1)), PLATE, 1e-12, "unchanged");
}

#[test]
fn outward_hooks_point_away_from_their_centroid() {
    let (mut s, e) = setup();
    let sk = points_sketch(
        &mut s,
        end_cap(e),
        &[[-20.0, 0.0], [20.0, 0.0], [0.0, 15.0]],
    );
    ok(
        &mut s,
        feature(
            "snap_fit",
            &[("sketch", sketch_in(sk))],
            &HOOK,
            &[("orient", "outward")],
        ),
    );
    assert_close(
        vol(&s, BodyId(1)),
        PLATE + 3.0 * 5.0 * hook_area(8.0),
        1e-6,
        "three hooks",
    );
    // Centroid (0, 5): the catch at (20, 0) points along (20, -5)/|.|; the one
    // at (0, 15) along +y. Probe just inside each catch tip (1.6 out, z 10.1).
    let d = (20.0f64).hypot(5.0);
    let (ux, uy) = (20.0 / d, -5.0 / d);
    let pr = probe(
        &s,
        BodyId(1),
        &[
            [20.0 + 1.6 * ux, 1.6 * uy, 10.1],
            [20.0 - 1.6 * ux, -1.6 * uy, 10.1],
            [0.0, 15.0 + 1.6, 10.1],
            [0.0, 15.0 - 1.6, 10.1],
        ],
    );
    assert_eq!(pr, [Some(true), Some(false), Some(true), Some(false)]);
}

#[test]
fn a_base_fillet_rounds_both_sides_of_the_hook_root() {
    let (mut s, e) = setup();
    let sk = points_sketch(&mut s, end_cap(e), &[[0.0, 0.0]]);
    let mut p = HOOK.to_vec();
    p.push(("base_fillet", 0.5));
    ok(
        &mut s,
        feature("snap_fit", &[("sketch", sketch_in(sk))], &p, &[]),
    );
    let spandrel = 0.25 * (1.0 - std::f64::consts::PI / 4.0);
    assert_close(
        vol(&s, BodyId(1)),
        PLATE + 5.0 * hook_area(8.0) + 2.0 * 5.0 * spandrel,
        1e-6,
        "volume",
    );
}
