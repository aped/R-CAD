//! Draft of existing faces (fixed plane), exact against the prismatoid.
use super::*;

fn sides(e: FeatureId) -> PluginInput {
    PluginInput::Faces {
        faces: (1..=4)
            .map(|entity| FaceRef {
                feature: e,
                role: FaceRole::ExtrudeSide { entity },
            })
            .collect(),
    }
}

/// ∫ (a - 2tz)(b - 2tz) dz over [0, h] (Simpson is exact for the quadratic).
fn tapered_box(a: f64, b: f64, h: f64, t: f64) -> f64 {
    let area = |z: f64| (a - 2.0 * t * z) * (b - 2.0 * t * z);
    h / 6.0 * (area(0.0) + 4.0 * area(h / 2.0) + area(h))
}

#[test]
fn drafting_the_walls_of_a_box_makes_a_tapered_box() {
    let mut s = Session::new();
    ok(&mut s, Command::NewDocument);
    let e = plate(&mut s, SketchPlane::WorldXY, 40.0, 30.0, 20.0);
    let r = ok(
        &mut s,
        feature(
            "draft",
            &[
                ("faces", sides(e)),
                ("fixed", face_in(SketchPlane::Face(start_cap(e)))),
            ],
            &[("draft_deg", 3.0)],
            &[],
        ),
    );
    let t = 3f64.to_radians().tan();
    assert_close(
        vol(&s, BodyId(1)),
        tapered_box(40.0, 30.0, 20.0, t),
        1e-9,
        "drafted box",
    );
    // The top is inset by 20 tan 3° on each side; the bottom is unchanged.
    let inset = 20.0 * t;
    let pr = probe(
        &s,
        BodyId(1),
        &[
            [19.99, 0.0, 0.01],
            [20.0 - inset - 0.01, 0.0, 19.99],
            [20.0 - inset + 0.01, 0.0, 19.99],
        ],
    );
    assert_eq!(pr, [Some(true), Some(true), Some(false)]);
    // The angle is a bindable parameter; reverse leans the walls out.
    ok(
        &mut s,
        Command::SetParameter {
            feature: r.feature.unwrap(),
            name: "draft_deg".into(),
            value: 1.0,
        },
    );
    let t1 = 1f64.to_radians().tan();
    assert_close(
        vol(&s, BodyId(1)),
        tapered_box(40.0, 30.0, 20.0, t1),
        1e-9,
        "1 degree",
    );
}

#[test]
fn draft_follows_the_rule_and_refuses_parallel_faces() {
    let mut s = Session::new();
    ok(&mut s, Command::NewDocument);
    let e = plate(&mut s, SketchPlane::WorldXY, 40.0, 30.0, 20.0);
    ok(
        &mut s,
        Command::SetPluginData(rcad_commands::plugins::SetPluginDataCmd {
            plugin: "plastics".into(),
            data: serde_json::json!({"rules": [{"name": "ABS", "values": {"DA": "2"}}], "bodies": {"1": "ABS"}}),
        }),
    );
    ok(
        &mut s,
        feature(
            "draft",
            &[
                ("faces", sides(e)),
                ("fixed", face_in(SketchPlane::Face(start_cap(e)))),
            ],
            &[],
            &[],
        ),
    );
    let t = 2f64.to_radians().tan();
    assert_close(
        vol(&s, BodyId(1)),
        tapered_box(40.0, 30.0, 20.0, t),
        1e-9,
        "rule draft",
    );
    let top = PluginInput::Faces {
        faces: vec![FaceRef {
            feature: e,
            role: FaceRole::ExtrudeEndCap,
        }],
    };
    let m = err(
        &mut s,
        feature(
            "draft",
            &[
                ("faces", top),
                ("fixed", face_in(SketchPlane::Face(start_cap(e)))),
            ],
            &[],
            &[],
        ),
    );
    assert!(m.contains("parallel to the fixed plane"), "{m}");
}

#[test]
fn a_two_sided_draft_narrows_away_from_the_parting_plane() {
    let mut s = Session::new();
    ok(&mut s, Command::NewDocument);
    let e = plate(&mut s, SketchPlane::WorldXY, 40.0, 30.0, 20.0);
    // Parting plane 10 mm up (offset from the bottom, which faces -z: -10).
    let mid = SketchPlane::Offset {
        base: rcad_document::PlaneBase::Face(start_cap(e)),
        distance: -10.0,
        angle: 0.0,
    };
    ok(
        &mut s,
        feature(
            "draft",
            &[("faces", sides(e)), ("fixed", face_in(mid))],
            &[("draft_deg", 3.0)],
            &[("sides", "two_sides")],
        ),
    );
    let t = 3f64.to_radians().tan();
    assert_close(
        vol(&s, BodyId(1)),
        2.0 * tapered_box(40.0, 30.0, 10.0, t),
        1e-9,
        "two-sided draft",
    );
    // Widest at the parting plane, inset 10 tan 3° at top and bottom.
    let inset = 10.0 * t;
    let pr = probe(
        &s,
        BodyId(1),
        &[
            [19.99, 0.0, 10.0],
            [20.0 - inset + 0.01, 0.0, 0.01],
            [20.0 - inset + 0.01, 0.0, 19.99],
        ],
    );
    assert_eq!(pr, [Some(true), Some(false), Some(false)]);
}

/// Volume of a 40 × 30 × `h` box (z from 0) drafted by `a` about the
/// roof-shaped parting line z = 12 − 4|x|/30 swept along y, independent of
/// the kernel. Each front/back piece turns about its segment, so at a point
/// of the wall at in-plane distance d from the segment's line the wall is
/// inset by d·tan a; adjacent facets meet where those distances agree (the
/// bisector), which makes the inset max(d_left, d_right) above the line and
/// min(...) below it. The side walls hinge on the level line at the parting
/// height z_s = 28/3 there. The section at height z is
/// |x| ≤ 20 − tan a·|z − z_s|, |y| ≤ 15 − tan a·D(x, z): piecewise linear in
/// x (exact trapezoids between the kinks), quadratic in z between z_s, 12
/// and the ends (exact Simpson). `below`: draft below the line too.
fn roof_drafted_box(h: f64, a: f64, below: bool) -> f64 {
    let t = a.tan();
    let c = 1.0 / (1.0f64 + (4.0f64 / 30.0).powi(2)).sqrt();
    let zl = |x: f64| 12.0 + 4.0 * x / 30.0;
    let zr = |x: f64| 12.0 - 4.0 * x / 30.0;
    let zs = 28.0 / 3.0;
    let inset = |x: f64, z: f64| {
        let zp = 12.0 - 4.0 * x.abs() / 30.0;
        if z >= zp {
            ((z - zl(x)) * c).max((z - zr(x)) * c)
        } else if below {
            ((zl(x) - z) * c).min((zr(x) - z) * c)
        } else {
            0.0
        }
    };
    let area = |z: f64| {
        let side = if below || z >= zs {
            t * (z - zs).abs()
        } else {
            0.0
        };
        let xmax = 20.0 - side;
        let mut xs = vec![-xmax, 0.0, xmax];
        if (8.0..=12.0).contains(&z) {
            let xb = 30.0 * (12.0 - z) / 4.0;
            xs.extend([-xb, xb]);
        }
        xs.retain(|x| x.abs() <= xmax);
        xs.sort_by(f64::total_cmp);
        xs.windows(2)
            .map(|w| {
                let f = |x: f64| 30.0 - 2.0 * t * inset(x, z);
                (w[1] - w[0]) * (f(w[0]) + f(w[1])) / 2.0
            })
            .sum::<f64>()
    };
    let mut cuts = vec![0.0, h];
    cuts.extend([zs, 12.0].into_iter().filter(|z| *z < h));
    cuts.sort_by(f64::total_cmp);
    cuts.windows(2)
        .map(|w| {
            let n = 64;
            let dz = (w[1] - w[0]) / n as f64;
            (0..n)
                .map(|i| {
                    let z0 = w[0] + i as f64 * dz;
                    dz / 6.0 * (area(z0) + 4.0 * area(z0 + dz / 2.0) + area(z0 + dz))
                })
                .sum::<f64>()
        })
        .sum()
}

#[test]
fn a_broken_parting_line_drafts_each_segment_about_itself() {
    let mut s = Session::new();
    ok(&mut s, Command::NewDocument);
    let e = plate(&mut s, SketchPlane::WorldXY, 40.0, 30.0, 20.0);
    // Roof-shaped parting line on the XZ plane (sketch x = X, y = Z), past
    // the box on both sides: z 8 at x ±30, 12 at x 0.
    let roof = line_sketch(
        &mut s,
        SketchPlane::WorldXZ,
        &[([-30.0, 8.0], [0.0, 12.0]), ([0.0, 12.0], [30.0, 8.0])],
    );
    let r = ok(
        &mut s,
        feature(
            "draft",
            &[
                ("faces", sides(e)),
                ("fixed", face_in(SketchPlane::Face(start_cap(e)))),
                ("parting", sketch_in(roof)),
            ],
            &[("draft_deg", 3.0)],
            &[("sides", "two_sides")],
        ),
    );
    let a = 3f64.to_radians();
    assert_close(
        vol(&s, BodyId(1)),
        roof_drafted_box(20.0, a, true),
        1e-9,
        "roof draft",
    );
    // On the parting line the wall stays; above the peak it is inset by
    // (20 − 12)·tan 3° (facets meet over the peak along x = 0).
    let inset = 8.0 * a.tan();
    let pr = probe(
        &s,
        BodyId(1),
        &[
            [0.0, -14.99, 12.0],
            [0.0, -15.0 + inset - 0.01, 19.99],
            [0.0, -15.0 + inset + 0.01, 19.99],
        ],
    );
    assert_eq!(pr, [Some(true), Some(false), Some(true)]);
    // One side only: the faces below the line keep their planes.
    let f = r.feature.unwrap();
    let mut kind = s
        .model()
        .unwrap()
        .document()
        .feature(f)
        .unwrap()
        .kind
        .clone();
    if let rcad_document::FeatureKind::Plugin { choices, .. } = &mut kind {
        choices.insert("sides".into(), "one_side".into());
    }
    ok(
        &mut s,
        Command::RedefineFeature {
            feature: f,
            kind: Box::new(kind),
            name: None,
            expressions: None,
        },
    );
    assert_close(
        vol(&s, BodyId(1)),
        roof_drafted_box(20.0, a, false),
        1e-9,
        "one side",
    );
    // Upstream edit: a taller box regenerates against the same line.
    let ex = e;
    ok(
        &mut s,
        Command::SetParameter {
            feature: ex,
            name: "distance".into(),
            value: 24.0,
        },
    );
    assert_close(
        vol(&s, BodyId(1)),
        roof_drafted_box(24.0, a, false),
        1e-9,
        "taller box",
    );
    // A box lower than the line: no face crosses it; refused, the previous
    // state kept.
    let m = err(
        &mut s,
        Command::SetParameter {
            feature: ex,
            name: "distance".into(),
            value: 6.0,
        },
    );
    assert!(m.contains("does not cross the parting line"), "{m}");
    assert_close(
        vol(&s, BodyId(1)),
        roof_drafted_box(24.0, a, false),
        1e-9,
        "rolled back",
    );
}

#[test]
fn parting_lines_must_be_one_chain_of_lines_spanning_the_faces() {
    let mut s = Session::new();
    ok(&mut s, Command::NewDocument);
    let e = plate(&mut s, SketchPlane::WorldXY, 40.0, 30.0, 20.0);
    let draft = |s: &mut Session, sk: FeatureId| {
        err(
            s,
            feature(
                "draft",
                &[
                    ("faces", sides(e)),
                    ("fixed", face_in(SketchPlane::Face(start_cap(e)))),
                    ("parting", sketch_in(sk)),
                ],
                &[("draft_deg", 3.0)],
                &[("sides", "two_sides")],
            ),
        )
    };
    // Shorter than the box.
    let short = line_sketch(
        &mut s,
        SketchPlane::WorldXZ,
        &[([-10.0, 8.0], [10.0, 12.0])],
    );
    let m = draft(&mut s, short);
    assert!(m.contains("does not span"), "{m}");
    // Two separate pieces.
    let broken = line_sketch(
        &mut s,
        SketchPlane::WorldXZ,
        &[([-30.0, 8.0], [-1.0, 12.0]), ([1.0, 12.0], [30.0, 8.0])],
    );
    let m = draft(&mut s, broken);
    assert!(m.contains("one open chain"), "{m}");
    // Turning back on itself (two heights at one x).
    let zigzag = line_sketch(
        &mut s,
        SketchPlane::WorldXZ,
        &[
            ([-30.0, 8.0], [10.0, 10.0]),
            ([10.0, 10.0], [-5.0, 12.0]),
            ([-5.0, 12.0], [30.0, 12.0]),
        ],
    );
    let m = draft(&mut s, zigzag);
    assert!(m.contains("without turning back"), "{m}");
}
