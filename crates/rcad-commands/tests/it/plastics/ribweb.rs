//! Rib and Web inside an open box, against closed-form volumes.
use super::*;
use rcad_commands::Command;

fn body_in() -> PluginInput {
    PluginInput::Body { body: BodyId(1) }
}

#[test]
fn a_rib_fills_down_from_its_line_to_the_floor() {
    let (mut s, _) = open_box();
    assert_close(vol(&s, BodyId(1)), BOX, 1e-9, "box");
    // XZ plane: sketch x = X, y = Z. Line at Z = 15 into both walls.
    let sk = line_sketch(
        &mut s,
        SketchPlane::WorldXZ,
        &[([-29.0, 15.0], [29.0, 15.0])],
    );
    let r = ok(
        &mut s,
        feature(
            "rib",
            &[("sketch", sketch_in(sk)), ("body", body_in())],
            &[("thickness", 2.0), ("depth", 13.5)],
            &[("side", "right")],
        ),
    );
    assert_close(vol(&s, BodyId(1)), BOX + 56.0 * 2.0 * 13.0, 1e-6, "rib");
    let pr = probe(
        &s,
        BodyId(1),
        &[[0.0, 0.9, 8.0], [0.0, 1.1, 8.0], [0.0, 0.0, 15.5]],
    );
    assert_eq!(pr, [Some(true), Some(false), Some(false)]);
    // Drafted: the section narrows by tan(1°) per mm on each side.
    let f = r.feature.unwrap();
    ok(
        &mut s,
        Command::SetParameter {
            feature: f,
            name: "draft_deg".into(),
            value: 1.0,
        },
    );
    let a = 1.0f64.to_radians();
    assert_close(
        vol(&s, BodyId(1)),
        BOX + 56.0 * (26.0 - 169.0 * a.tan()),
        1e-6,
        "drafted rib",
    );
}

#[test]
fn a_web_network_joins_lines_and_reaches_the_next_face() {
    let (mut s, e) = open_box();
    // On the rim plane (the cap's plane, now the rim face): a cross of two
    // lines through the cavity, each into the walls; to the floor (z = 2).
    let sk = line_sketch(
        &mut s,
        end_cap(e),
        &[([-29.0, 0.0], [29.0, 0.0]), ([0.0, -19.0], [0.0, 19.0])],
    );
    ok(
        &mut s,
        feature(
            "web",
            &[("sketch", sketch_in(sk))],
            &[("thickness", 2.0)],
            &[("direction", "flip"), ("extent", "to_next")],
        ),
    );
    // Two walls 18 high in the cavity, crossing in a 2×2 column.
    let want = BOX + (56.0 * 2.0 + 36.0 * 2.0 - 4.0) * 18.0;
    assert_close(vol(&s, BodyId(1)), want, 1e-6, "web");
    let pr = probe(
        &s,
        BodyId(1),
        &[[10.0, 0.5, 10.0], [0.5, 10.0, 19.5], [10.0, 10.0, 10.0]],
    );
    assert_eq!(pr, [Some(true), Some(true), Some(false)]);
}

#[test]
fn a_drafted_web_narrows_away_from_its_sketch() {
    let (mut s, _) = open_box();
    let sk = line_sketch(&mut s, offset_xy(20.0), &[([-29.0, 0.0], [29.0, 0.0])]);
    ok(
        &mut s,
        feature(
            "web",
            &[("sketch", sketch_in(sk)), ("body", body_in())],
            &[("thickness", 2.0), ("depth", 18.5), ("draft_deg", 2.0)],
            &[("direction", "flip")],
        ),
    );
    let a = 2.0f64.to_radians();
    // Width 2 - 2 x tan a at x mm below z = 20, over the cavity x in 0..18.
    let area = 2.0 * 18.0 - 18.0 * 18.0 * a.tan();
    assert_close(vol(&s, BodyId(1)), BOX + 56.0 * area, 1e-6, "drafted web");
}

#[test]
fn unsupported_rib_input_is_refused() {
    let (mut s, e) = open_box();
    let mut sk = rcad_sketch::Sketch::default();
    let c = sk.add_point(0.0, 0.0);
    sk.add_circle(c, 5.0);
    let circle = ok(
        &mut s,
        Command::CreateSketch {
            name: None,
            plane: end_cap(e),
            template: SketchTemplate::Custom { sketch: sk },
        },
    )
    .feature
    .unwrap();
    let m = err(
        &mut s,
        feature("web", &[("sketch", sketch_in(circle))], &[], &[]),
    );
    assert!(m.contains("only straight sketch lines"), "{m}");
    let world = line_sketch(&mut s, offset_xy(30.0), &[([0.0, 0.0], [5.0, 0.0])]);
    let m = err(
        &mut s,
        feature("rib", &[("sketch", sketch_in(world))], &[], &[]),
    );
    assert!(m.contains("choose the body"), "{m}");
    let m = err(
        &mut s,
        feature(
            "rib",
            &[("sketch", sketch_in(world)), ("body", body_in())],
            &[("thickness", 2.0), ("depth", 40.0), ("draft_deg", 5.0)],
            &[],
        ),
    );
    assert!(m.contains("draft closes"), "{m}");
    assert_close(vol(&s, BodyId(1)), BOX, 1e-9, "unchanged");
}

#[test]
fn to_next_only_sees_faces_under_the_web() {
    let (mut s, e) = open_box();
    // A 6×6 block 5 high on the floor at x = 15 (its top, z = 7, faces up
    // but is not under the web at x = 0).
    let blk = ok(
        &mut s,
        Command::CreateSketch {
            name: None,
            plane: SketchPlane::Offset {
                base: rcad_document::PlaneBase::Face(FaceRef {
                    feature: e,
                    role: FaceRole::ExtrudeEndCap,
                }),
                distance: -18.0,
                angle: 0.0,
            },
            template: SketchTemplate::CenteredRectangle {
                width: 6.0,
                height: 6.0,
                center: Some([15.0, 0.0]),
            },
        },
    )
    .feature
    .unwrap();
    let mut c = Command::extrude(None, blk, 5.0);
    if let Command::Extrude { op, target, .. } = &mut c {
        *op = rcad_commands::ExtrudeOp::Join;
        *target = Some(BodyId(1));
    }
    ok(&mut s, c);
    let before = vol(&s, BodyId(1));
    assert_close(before, BOX + 180.0, 1e-6, "block");
    let sk = line_sketch(&mut s, end_cap(e), &[([0.0, -19.0], [0.0, 19.0])]);
    ok(
        &mut s,
        feature(
            "web",
            &[("sketch", sketch_in(sk))],
            &[("thickness", 2.0), ("draft_deg", 0.0)],
            &[("direction", "flip"), ("extent", "to_next")],
        ),
    );
    assert_close(
        vol(&s, BodyId(1)),
        before + 36.0 * 2.0 * 18.0,
        1e-6,
        "web to the floor",
    );
}

#[test]
fn a_rib_to_next_reaches_the_floor_along_its_line() {
    let (mut s, _) = open_box();
    // XZ plane, line at Z = 15 spanning the cavity (x ±28): the rib grows
    // down to the floor's top (z = 2), 13 deep, sunk 0.2 into it.
    let sk = line_sketch(
        &mut s,
        SketchPlane::WorldXZ,
        &[([-28.0, 15.0], [28.0, 15.0])],
    );
    ok(
        &mut s,
        feature(
            "rib",
            &[("sketch", sketch_in(sk)), ("body", body_in())],
            &[("thickness", 2.0), ("draft_deg", 0.0)],
            &[("side", "right"), ("extent", "to_next")],
        ),
    );
    assert_close(
        vol(&s, BodyId(1)),
        BOX + 56.0 * 2.0 * 13.0,
        1e-6,
        "rib to the floor",
    );
    // Growing up instead there is no face before the open top: refused.
    let sk2 = line_sketch(
        &mut s,
        SketchPlane::WorldXZ,
        &[([-10.0, 15.0], [10.0, 15.0])],
    );
    let m = err(
        &mut s,
        feature(
            "rib",
            &[("sketch", sketch_in(sk2)), ("body", body_in())],
            &[("draft_deg", 0.0)],
            &[("side", "left"), ("extent", "to_next")],
        ),
    );
    assert!(m.contains("no planar face"), "{m}");
}
