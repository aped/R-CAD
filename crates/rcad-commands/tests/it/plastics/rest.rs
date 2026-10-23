//! Rest: uniform-wall platforms opened through the floor.
use super::*;

fn rest_sketch(s: &mut Session, e: FeatureId) -> FeatureId {
    // 10×10 square on a plane 8 below the rim (z = 12), 10 above the floor.
    let plane = SketchPlane::Offset {
        base: rcad_document::PlaneBase::Face(FaceRef {
            feature: e,
            role: FaceRole::ExtrudeEndCap,
        }),
        distance: -8.0,
        angle: 0.0,
    };
    ok(
        s,
        Command::CreateSketch {
            name: None,
            plane,
            template: SketchTemplate::CenteredRectangle {
                width: 10.0,
                height: 10.0,
                center: None,
            },
        },
    )
    .feature
    .unwrap()
}

/// ∫ (c - k z)² dz over [a, b].
fn sq(c: f64, k: f64, a: f64, b: f64) -> f64 {
    let f = |z: f64| -(c - k * z).powi(3) / (3.0 * k);
    if k == 0.0 {
        c * c * (b - a)
    } else {
        f(b) - f(a)
    }
}

#[test]
fn a_rest_is_a_cap_on_walls_open_through_the_floor() {
    let (mut s, e) = open_box();
    let sk = rest_sketch(&mut s, e);
    let r = ok(
        &mut s,
        feature(
            "rest",
            &[("sketch", sketch_in(sk))],
            &[("thickness", 2.0), ("draft_deg", 0.0)],
            &[],
        ),
    );
    assert_close(vol(&s, BodyId(1)), BOX + 1000.0 - 36.0 * 10.0, 1e-6, "rest");
    let pr = probe(
        &s,
        BodyId(1),
        &[
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 11.0],
            [4.0, 0.0, 5.0],
            [0.0, 0.0, 5.0],
        ],
    );
    assert_eq!(pr, [Some(false), Some(true), Some(true), Some(false)]);
    // Drafted 2°: exact at the top, walls and inside both drafted.
    ok(
        &mut s,
        Command::SetParameter {
            feature: r.feature.unwrap(),
            name: "draft_deg".into(),
            value: 2.0,
        },
    );
    let t = 2f64.to_radians().tan();
    // Solid side 10 + 2t(12 - z) over z 2..12; inside side 6 + 24t - 2tz over z 0..10.
    let added = sq(10.0 + 24.0 * t, 2.0 * t, 2.0, 12.0);
    let removed = sq(6.0 + 24.0 * t, 2.0 * t, 0.0, 10.0);
    assert_close(
        vol(&s, BodyId(1)),
        BOX + added - removed,
        1e-6,
        "drafted rest",
    );
}

#[test]
fn a_rest_too_thick_for_its_profile_is_refused() {
    let (mut s, e) = open_box();
    let sk = rest_sketch(&mut s, e);
    let m = err(
        &mut s,
        feature(
            "rest",
            &[("sketch", sketch_in(sk))],
            &[("thickness", 6.0)],
            &[],
        ),
    );
    assert!(m.contains("does not fit"), "{m}");
    let m = err(
        &mut s,
        feature(
            "rest",
            &[("sketch", sketch_in(sk))],
            &[("offset", -9.0)],
            &[],
        ),
    );
    assert!(m.contains("more than the thickness"), "{m}");
    assert_close(vol(&s, BodyId(1)), BOX, 1e-9, "unchanged");
}

#[test]
fn a_rounded_rest_follows_its_arcs() {
    use std::f64::consts::PI;
    let (mut s, e) = open_box();
    // A 16 x 10 stadium (straight 6, end radius 5) 10 above the floor.
    let mut sk = rcad_sketch::Sketch::default();
    let pts = [
        [-3.0, -5.0],
        [3.0, -5.0],
        [3.0, 5.0],
        [-3.0, 5.0],
        [3.0, 0.0],
        [-3.0, 0.0],
    ];
    let id: Vec<_> = pts.iter().map(|p| sk.add_point(p[0], p[1])).collect();
    for (i, p) in id.iter().zip(&pts) {
        sk.constraints.push(rcad_sketch::Constraint::FixedPoint {
            p: *i,
            x: p[0],
            y: p[1],
        });
    }
    sk.add_line(id[0], id[1]);
    sk.add_line(id[2], id[3]);
    sk.entities.push(rcad_sketch::Entity::Arc {
        id: rcad_sketch::EntityId(3),
        center: id[4],
        start: id[1],
        end: id[2],
    });
    sk.entities.push(rcad_sketch::Entity::Arc {
        id: rcad_sketch::EntityId(4),
        center: id[5],
        start: id[3],
        end: id[0],
    });
    let plane = SketchPlane::Offset {
        base: rcad_document::PlaneBase::Face(FaceRef {
            feature: e,
            role: FaceRole::ExtrudeEndCap,
        }),
        distance: -8.0,
        angle: 0.0,
    };
    let f = ok(
        &mut s,
        Command::CreateSketch {
            name: None,
            plane,
            template: SketchTemplate::Custom { sketch: sk },
        },
    )
    .feature
    .unwrap();
    ok(
        &mut s,
        feature(
            "rest",
            &[("sketch", sketch_in(f))],
            &[("thickness", 2.0), ("draft_deg", 0.0)],
            &[],
        ),
    );
    // Solid 10 high minus its inside (offset 2 in: straight 6, r 3) over
    // the floor (2) and the walls (8).
    let (outer, inner) = (10.0 * 6.0 + 25.0 * PI, 6.0 * 6.0 + 9.0 * PI);
    assert_close(
        vol(&s, BodyId(1)),
        BOX + outer * 10.0 - inner * 10.0,
        1e-6,
        "rounded rest",
    );
}
