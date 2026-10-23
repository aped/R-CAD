//! Lip and groove along the rim of an open box.
use super::*;

#[test]
fn inner_and_outer_lips_are_bands_of_the_rim() {
    let (mut s, e) = open_box();
    let rim = face_in(end_cap(e));
    let r = ok(
        &mut s,
        feature(
            "lip",
            &[("face", rim.clone())],
            &[("width", 1.0), ("height", 2.0)],
            &[],
        ),
    );
    assert_close(
        vol(&s, BodyId(1)),
        BOX + (58.0 * 38.0 - 56.0 * 36.0) * 2.0,
        1e-6,
        "inner lip",
    );
    let pr = probe(&s, BodyId(1), &[[28.5, 0.0, 21.0], [29.5, 0.0, 21.0]]);
    assert_eq!(pr, [Some(true), Some(false)]);
    ok(&mut s, Command::Undo);
    let _ = r;
    ok(
        &mut s,
        feature(
            "lip",
            &[("face", rim)],
            &[("width", 1.0), ("height", 2.0)],
            &[("position", "outer")],
        ),
    );
    assert_close(
        vol(&s, BodyId(1)),
        BOX + (60.0 * 40.0 - 58.0 * 38.0) * 2.0,
        1e-6,
        "outer lip",
    );
}

#[test]
fn a_lid_gets_the_groove_matching_the_lip() {
    let (mut s, e) = open_box();
    // Lid: z 20..30 shelled open at its bottom (rim at z = 20 facing down).
    let lid = plate(&mut s, offset_xy(20.0), 60.0, 40.0, 10.0);
    ok(
        &mut s,
        Command::ShellFace {
            name: None,
            face: start_cap(lid),
            thickness: 2.0,
        },
    );
    let lid_v = vol(&s, BodyId(2));
    ok(
        &mut s,
        feature(
            "lip",
            &[
                ("face", face_in(end_cap(e))),
                ("mate", face_in(SketchPlane::Face(start_cap(lid)))),
            ],
            &[("width", 1.0), ("height", 2.0), ("clearance", 0.1)],
            &[("type", "lip_and_groove")],
        ),
    );
    assert_close(vol(&s, BodyId(1)), BOX + 188.0 * 2.0, 1e-6, "lip");
    // Groove: the band grown 0.1 mm, cut 2.1 deep; only the wall part has material.
    assert_close(
        vol(&s, BodyId(2)),
        lid_v - (58.2 * 38.2 - 56.0 * 36.0) * 2.1,
        1e-6,
        "groove",
    );
    // The lip fits the groove with clearance: no lid material where the lip is.
    let pr = probe(
        &s,
        BodyId(2),
        &[[28.5, 0.0, 21.0], [28.5, 0.0, 22.2], [29.5, 0.0, 21.0]],
    );
    assert_eq!(pr, [Some(false), Some(true), Some(true)]);
}

#[test]
fn lips_wider_than_the_wall_or_off_a_rim_are_refused() {
    let (mut s, e) = open_box();
    let m = err(
        &mut s,
        feature(
            "lip",
            &[("face", face_in(end_cap(e)))],
            &[("width", 2.5)],
            &[],
        ),
    );
    assert!(m.contains("not narrower than the wall"), "{m}");
    // A plate's top has one boundary loop: not a rim.
    let p = plate(&mut s, offset_xy(40.0), 10.0, 10.0, 2.0);
    let m = err(
        &mut s,
        feature("lip", &[("face", face_in(end_cap(p)))], &[], &[]),
    );
    assert!(m.contains("must be a rim"), "{m}");
    assert_close(vol(&s, BodyId(1)), BOX, 1e-9, "unchanged");
}
