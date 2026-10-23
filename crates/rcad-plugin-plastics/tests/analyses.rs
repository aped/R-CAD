//! Draft, Accessibility and Minimum Radius analyses on hand-built meshes with
//! analytic answers: grid boxes (every face a plane), a drafted frustum, a
//! faceted round boss labelled as a cone, a faceted prism labelled as a
//! cylinder and an icosphere with no exact surface.
use rcad_plugin::{
    AnalysisRequest, BodyGeom, BodyMesh, FaceGeom, FaceInfo, Frame, Plugin, PluginError, Resolved,
    Surface,
};
use rcad_plugin_plastics::Plastics;
use serde_json::Value;
use std::collections::HashMap;

const TOL: f64 = 1e-9;

/// Closed solid made of the `filled` cells of the grid `xs × ys × zs`: one
/// quad (two triangles) per exposed cell side, one plane face per
/// (axis, side, grid plane), vertices welded by grid index.
fn grid_solid(coords: [&[f64]; 3], filled: impl Fn(usize, usize, usize) -> bool) -> BodyMesh {
    let n = [
        coords[0].len() - 1,
        coords[1].len() - 1,
        coords[2].len() - 1,
    ];
    let is = |c: [isize; 3]| {
        (0..3).all(|a| c[a] >= 0 && (c[a] as usize) < n[a])
            && filled(c[0] as usize, c[1] as usize, c[2] as usize)
    };
    let mut vid: HashMap<[usize; 3], u32> = HashMap::new();
    let mut vertices = Vec::new();
    let mut fid: HashMap<(usize, bool, usize), u32> = HashMap::new();
    let mut faces: Vec<FaceInfo> = Vec::new();
    let (mut triangles, mut tri_face) = (Vec::new(), Vec::new());
    for i in 0..n[0] {
        for j in 0..n[1] {
            for k in 0..n[2] {
                if !filled(i, j, k) {
                    continue;
                }
                let cell = [i, j, k];
                for a in 0..3 {
                    for pos in [false, true] {
                        let mut nb = cell.map(|v| v as isize);
                        nb[a] += if pos { 1 } else { -1 };
                        if is(nb) {
                            continue;
                        }
                        let (b, c) = ((a + 1) % 3, (a + 2) % 3);
                        let plane = cell[a] + usize::from(pos);
                        let mut corners = [[0usize; 3]; 4];
                        for (q, (db, dc)) in [(0, 0), (1, 0), (1, 1), (0, 1)].iter().enumerate() {
                            corners[q][a] = plane;
                            corners[q][b] = cell[b] + db;
                            corners[q][c] = cell[c] + dc;
                        }
                        if !pos {
                            corners.reverse();
                        }
                        let ids = corners.map(|g| {
                            *vid.entry(g).or_insert_with(|| {
                                vertices.push([coords[0][g[0]], coords[1][g[1]], coords[2][g[2]]]);
                                (vertices.len() - 1) as u32
                            })
                        });
                        let f = *fid.entry((a, pos, plane)).or_insert_with(|| {
                            let mut normal = [0.0; 3];
                            normal[a] = if pos { 1.0 } else { -1.0 };
                            faces.push(FaceInfo {
                                surface: Surface::Plane { normal },
                                area: 0.0,
                            });
                            (faces.len() - 1) as u32
                        });
                        faces[f as usize].area += (coords[b][cell[b] + 1] - coords[b][cell[b]])
                            * (coords[c][cell[c] + 1] - coords[c][cell[c]]);
                        triangles.push([ids[0], ids[1], ids[2]]);
                        triangles.push([ids[0], ids[2], ids[3]]);
                        tri_face.extend([f, f]);
                    }
                }
            }
        }
    }
    BodyMesh {
        body: 7,
        chord: 0.01,
        faces,
        vertices,
        triangles,
        tri_face,
    }
}

fn block(x: f64, y: f64, z: f64) -> BodyMesh {
    grid_solid([&[0.0, x], &[0.0, y], &[0.0, z]], |_, _, _| true)
}

/// 40×20×10 block with a slot through y in its +x wall: x 36..40, z 4..6.
fn slotted_block() -> BodyMesh {
    grid_solid(
        [&[0.0, 36.0, 40.0], &[0.0, 20.0], &[0.0, 4.0, 6.0, 10.0]],
        |i, _, k| (i, k) != (1, 1),
    )
}

/// Square frustum: base 2a at z = 0, top 2b at z = h, walls drafted by `deg`.
fn frustum(a: f64, h: f64, deg: f64) -> BodyMesh {
    let th = deg.to_radians();
    let b = a - h * th.tan();
    let vertices = vec![
        [-a, -a, 0.0],
        [a, -a, 0.0],
        [a, a, 0.0],
        [-a, a, 0.0],
        [-b, -b, h],
        [b, -b, h],
        [b, b, h],
        [-b, b, h],
    ];
    let (c, s) = (th.cos(), th.sin());
    let side = (a + b) * h / c;
    let plane = |normal: [f64; 3], area: f64| FaceInfo {
        surface: Surface::Plane { normal },
        area,
    };
    let faces = vec![
        plane([0.0, 0.0, -1.0], 4.0 * a * a),
        plane([0.0, 0.0, 1.0], 4.0 * b * b),
        plane([0.0, -c, s], side),
        plane([c, 0.0, s], side),
        plane([0.0, c, s], side),
        plane([-c, 0.0, s], side),
    ];
    let triangles = vec![
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    BodyMesh {
        body: 7,
        chord: 0.0,
        faces,
        vertices,
        triangles,
        tri_face: vec![0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5],
    }
}

/// Faceted solid of revolution about z: an `n`-gon of circumradius `r0` at
/// z = 0 and `r1` at z = h, side vertices on the exact surface; faces bottom
/// (plane), top (plane), side (`side`).
fn revolved(n: u32, r0: f64, r1: f64, h: f64, side: Surface) -> BodyMesh {
    let mut vertices = Vec::new();
    for (z, r) in [(0.0, r0), (h, r1)] {
        for k in 0..n {
            let a = std::f64::consts::TAU * f64::from(k) / f64::from(n);
            vertices.push([r * a.cos(), r * a.sin(), z]);
        }
    }
    vertices.push([0.0, 0.0, 0.0]);
    vertices.push([0.0, 0.0, h]);
    let (c0, c1) = (2 * n, 2 * n + 1);
    let (mut triangles, mut tri_face) = (Vec::new(), Vec::new());
    for k in 0..n {
        let k1 = (k + 1) % n;
        triangles.push([c0, k1, k]);
        tri_face.push(0);
        triangles.push([c1, n + k, n + k1]);
        tri_face.push(1);
        triangles.push([k, k1, n + k1]);
        triangles.push([k, n + k1, n + k]);
        tri_face.extend([2, 2]);
    }
    let plane = |z: f64| FaceInfo {
        surface: Surface::Plane {
            normal: [0.0, 0.0, z],
        },
        area: 0.0,
    };
    BodyMesh {
        body: 7,
        chord: 0.1,
        faces: vec![
            plane(-1.0),
            plane(1.0),
            FaceInfo {
                surface: side,
                area: 0.0,
            },
        ],
        vertices,
        triangles,
        tri_face,
    }
}

fn cylinder(r: f64, outward: bool) -> Surface {
    Surface::Cylinder {
        point: [0.0; 3],
        axis: [0.0, 0.0, 1.0],
        radius: r,
        outward,
    }
}

/// Icosphere of radius `r` (`levels` subdivisions, vertices on the sphere),
/// one face with no exact surface.
fn icosphere(r: f64, levels: usize) -> BodyMesh {
    let t = (1.0 + 5f64.sqrt()) / 2.0;
    let mut v: Vec<[f64; 3]> = vec![
        [-1.0, t, 0.0],
        [1.0, t, 0.0],
        [-1.0, -t, 0.0],
        [1.0, -t, 0.0],
        [0.0, -1.0, t],
        [0.0, 1.0, t],
        [0.0, -1.0, -t],
        [0.0, 1.0, -t],
        [t, 0.0, -1.0],
        [t, 0.0, 1.0],
        [-t, 0.0, -1.0],
        [-t, 0.0, 1.0],
    ];
    let mut f: Vec<[u32; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    let proj = |p: [f64; 3]| {
        let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        p.map(|c| c * r / l)
    };
    v = v.into_iter().map(proj).collect();
    for _ in 0..levels {
        let mut mid: HashMap<(u32, u32), u32> = HashMap::new();
        let mut nf = Vec::new();
        for [a, b, c] in f {
            let mut m = |x: u32, y: u32| {
                *mid.entry((x.min(y), x.max(y))).or_insert_with(|| {
                    let (p, q) = (v[x as usize], v[y as usize]);
                    v.push(proj([p[0] + q[0], p[1] + q[1], p[2] + q[2]]));
                    (v.len() - 1) as u32
                })
            };
            let (ab, bc, ca) = (m(a, b), m(b, c), m(c, a));
            nf.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        f = nf;
    }
    let n = f.len();
    BodyMesh {
        body: 7,
        chord: 0.2,
        faces: vec![FaceInfo {
            surface: Surface::Other,
            area: 0.0,
        }],
        vertices: v,
        triangles: f,
        tri_face: vec![0; n],
    }
}

fn request(
    analysis: &str,
    mesh: BodyMesh,
    params: &[(&str, f64)],
    choices: &[(&str, &str)],
) -> AnalysisRequest {
    AnalysisRequest {
        analysis: analysis.into(),
        params: params.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        choices: choices
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        inputs: [(
            "body".to_string(),
            Resolved::Body(BodyGeom {
                body: mesh.body,
                bounds: [[0.0; 3], [1.0; 3]],
            }),
        )]
        .into(),
        data: Value::Null,
        bodies: vec![mesh],
    }
}

fn draft(mesh: BodyMesh, min: f64, axis: &str) -> AnalysisRequest {
    request(
        "draft_analysis",
        mesh,
        &[("min_draft_deg", min), ("max_draft_deg", 90.0)],
        &[("pull_axis", axis)],
    )
}

fn access(mesh: BodyMesh, axis: &str, dirs: &str) -> AnalysisRequest {
    request(
        "accessibility",
        mesh,
        &[],
        &[("direction_axis", axis), ("both_directions", dirs)],
    )
}

fn radius(mesh: BodyMesh, min: f64) -> AnalysisRequest {
    request("minimum_radius", mesh, &[("min_radius", min)], &[])
}

fn run(req: &AnalysisRequest) -> Result<Value, PluginError> {
    Plastics.analyze(req)
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} != {b} (tol {tol})");
}

/// The face index of the plane with this outward normal containing `p`.
fn face_of(m: &BodyMesh, normal: [f64; 3], p: [f64; 3]) -> usize {
    (0..m.faces.len())
        .find(|&fi| {
            m.faces[fi].surface == Surface::Plane { normal }
                && m.tri_face.iter().zip(&m.triangles).any(|(&tf, t)| {
                    tf as usize == fi && {
                        let v = m.vertices[t[0] as usize];
                        (0..3).all(|a| normal[a] == 0.0 || v[a] == p[a])
                    }
                })
        })
        .expect("face")
}

fn face_entry(r: &Value, fi: usize) -> &Value {
    r["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["face"] == fi)
        .unwrap_or_else(|| panic!("no entry for face {fi}"))
}

#[test]
fn manifest_lists_the_inspect_analyses() {
    let m = Plastics.manifest();
    m.validate().unwrap();
    let ids: Vec<&str> = m.analyses.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "design_advice",
            "draft_analysis",
            "accessibility",
            "minimum_radius"
        ]
    );
    let da = m.analysis("draft_analysis").unwrap();
    assert_eq!(da.params[0].rule.as_deref(), Some("DA"));
    let nr = m.analysis("minimum_radius").unwrap();
    assert_eq!(nr.params[0].rule.as_deref(), Some("NR"));
}

#[test]
fn drafted_frustum_walls_are_positive_and_flip_with_the_pull() {
    let m = frustum(10.0, 10.0, 2.0);
    let side = m.faces[2].area;
    let (bottom, top) = (m.faces[0].area, m.faces[1].area);
    let r = run(&draft(m.clone(), 1.0, "+z")).unwrap();
    assert_eq!(r["pull"], serde_json::json!([0.0, 0.0, 1.0]));
    assert_eq!(r["range_deg"], serde_json::json!([-90.0, 90.0]));
    for fi in 2..6 {
        let e = face_entry(&r, fi);
        assert_eq!(e["exact"], true);
        assert_eq!(e["class"], "positive");
        close(f(&e["draft_deg"]), 2.0, 1e-9);
        close(f(&e["area"]["positive"]), side, 1e-9);
    }
    close(f(&face_entry(&r, 0)["draft_deg"]), -90.0, TOL);
    assert_eq!(face_entry(&r, 0)["class"], "negative");
    close(f(&face_entry(&r, 1)["draft_deg"]), 90.0, TOL);
    let s = &r["summary"];
    close(f(&s["positive_area"]), 4.0 * side + top, 1e-9);
    close(f(&s["negative_area"]), bottom, 1e-9);
    assert_eq!(f(&s["insufficient_area"]), 0.0);
    assert_eq!(f(&s["vertical_area"]), 0.0);
    assert_eq!(s["faces"]["positive"], 5);
    assert_eq!(s["faces"]["negative"], 1);
    // Per triangle: the face's value and class, in triangle order.
    let td = r["triangles"]["draft_deg"].as_array().unwrap();
    assert_eq!(td.len(), 12);
    close(f(&td[0]), -90.0, TOL);
    close(f(&td[4]), 2.0, 1e-9);
    assert_eq!(r["triangles"]["class"][4], "positive");

    // Pulled along -z every sign flips: the walls are negative.
    let rn = run(&draft(m.clone(), 1.0, "-z")).unwrap();
    for fi in 2..6 {
        let e = face_entry(&rn, fi);
        assert_eq!(e["class"], "negative");
        close(f(&e["draft_deg"]), -2.0, 1e-9);
    }
    close(f(&rn["summary"]["negative_area"]), 4.0 * side + top, 1e-9);
    close(f(&rn["summary"]["positive_area"]), bottom, 1e-9);

    // Below a 3° minimum the walls are insufficient.
    let r3 = run(&draft(m, 3.0, "+z")).unwrap();
    assert_eq!(face_entry(&r3, 3)["class"], "insufficient");
    close(f(&r3["summary"]["insufficient_area"]), 4.0 * side, 1e-9);
    assert_eq!(r3["summary"]["faces"]["insufficient"], 4);
}

#[test]
fn block_walls_are_vertical_and_insufficient() {
    let m = block(40.0, 20.0, 10.0);
    let r = run(&draft(m, 1.0, "+x")).unwrap();
    let s = &r["summary"];
    // Pulled along x: the y and z faces are vertical, 2·400 + 2·800.
    close(f(&s["vertical_area"]), 2400.0, TOL);
    close(f(&s["insufficient_area"]), 2400.0, TOL);
    close(f(&s["positive_area"]), 200.0, TOL);
    close(f(&s["negative_area"]), 200.0, TOL);
    close(f(&s["total_area"]), 2800.0, TOL);
    close(f(&s["min_deg"]), -90.0, TOL);
    close(f(&s["max_deg"]), 90.0, TOL);
}

#[test]
fn cone_draft_is_exact_coaxially_with_its_sign_from_the_mesh() {
    // Round boss r 10 → 9 over 20 high, labelled as the exact cone.
    let (r0, r1, h) = (10.0f64, 9.0, 20.0);
    let half = ((r0 - r1) / h).atan();
    let cone = Surface::Cone {
        point: [0.0, 0.0, r0 / ((r0 - r1) / h)],
        axis: [0.0, 0.0, 1.0],
        half_angle: half,
    };
    let m = revolved(48, r0, r1, h, cone);
    let r = run(&draft(m.clone(), 1.0, "+z")).unwrap();
    let e = face_entry(&r, 2);
    assert_eq!(e["surface"], "cone");
    assert_eq!(e["exact"], true);
    close(f(&e["draft_deg"]), half.to_degrees(), 1e-12);
    assert_eq!(e["class"], "positive");
    let rn = run(&draft(m.clone(), 1.0, "-z")).unwrap();
    close(
        f(&face_entry(&rn, 2)["draft_deg"]),
        -half.to_degrees(),
        1e-12,
    );
    // Across the axis the cone is meshed: every class appears.
    let rx = run(&draft(m, 1.0, "+x")).unwrap();
    let e = face_entry(&rx, 2);
    assert_eq!(e["exact"], false);
    assert_eq!(e["draft_deg"], Value::Null);
    assert_eq!(e["class"], "mixed");
    assert!(f(&e["min_deg"]) < -80.0 && f(&e["max_deg"]) > 80.0);
}

#[test]
fn coaxial_cylinder_is_exactly_vertical() {
    let m = revolved(16, 10.0, 10.0, 5.0, cylinder(10.0, true));
    let r = run(&draft(m, 1.0, "+z")).unwrap();
    let e = face_entry(&r, 2);
    assert_eq!(e["exact"], true);
    assert_eq!(f(&e["draft_deg"]), 0.0);
    assert_eq!(e["class"], "insufficient");
    close(
        f(&r["summary"]["vertical_area"]),
        f(&e["area"]["insufficient"]),
        TOL,
    );
}

#[test]
fn block_accessibility_from_one_and_both_directions() {
    let m = block(40.0, 20.0, 10.0);
    let r = run(&access(m.clone(), "+z", "one")).unwrap();
    let s = &r["summary"];
    close(f(&s["accessible_area"]), 800.0, TOL);
    close(f(&s["away_area"]), 800.0, TOL);
    close(f(&s["vertical_area"]), 1200.0, TOL);
    assert_eq!(f(&s["blocked_area"]), 0.0);
    close(f(&s["inaccessible_area"]), 2000.0, TOL);
    assert_eq!(s["faces"]["accessible"], 1);
    assert_eq!(s["faces"]["inaccessible"], 5);
    let rb = run(&access(m, "+z", "both")).unwrap();
    close(f(&rb["summary"]["accessible_area"]), 1600.0, TOL);
    assert_eq!(f(&rb["summary"]["away_area"]), 0.0);
    assert_eq!(rb["directions"], "both");
    let acc = rb["triangles"]["accessible"].as_array().unwrap();
    assert_eq!(acc.len(), 12);
    assert_eq!(acc.iter().filter(|a| **a == true).count(), 4);
}

#[test]
fn slot_faces_are_the_blocked_area() {
    let m = slotted_block();
    let floor = face_of(&m, [0.0, 0.0, 1.0], [0.0, 0.0, 4.0]);
    let roof = face_of(&m, [0.0, 0.0, -1.0], [0.0, 0.0, 6.0]);
    let back = face_of(&m, [1.0, 0.0, 0.0], [36.0, 0.0, 0.0]);
    // Two-plate mould along z: the slot's floor and roof (4·20 each) face a
    // half but the slot's other side is in the way.
    let r = run(&access(m.clone(), "+z", "both")).unwrap();
    let s = &r["summary"];
    close(f(&s["blocked_area"]), 160.0, TOL);
    close(f(&s["accessible_area"]), 1600.0, TOL);
    // Walls along z, the slot's back wall included: x=0 200, x=40 160,
    // back 40, y walls 2·(400 − 8).
    close(f(&s["vertical_area"]), 200.0 + 160.0 + 40.0 + 784.0, TOL);
    for fi in [floor, roof] {
        let e = face_entry(&r, fi);
        assert_eq!(e["status"], "inaccessible");
        close(f(&e["blocked_area"]), 80.0, TOL);
    }
    let t = m
        .tri_face
        .iter()
        .position(|&tf| tf as usize == floor)
        .unwrap();
    assert_eq!(r["triangles"]["status"][t], "blocked");
    assert_eq!(r["triangles"]["accessible"][t], false);
    // From +z alone only the floor is blocked; the roof faces away.
    let r1 = run(&access(m.clone(), "+z", "one")).unwrap();
    close(f(&r1["summary"]["blocked_area"]), 80.0, TOL);
    close(f(&face_entry(&r1, roof)["away_area"]), 80.0, TOL);
    // Along +x the slot opens towards the tool: its back wall is reachable
    // and nothing is blocked.
    let rx = run(&access(m, "+x", "one")).unwrap();
    assert_eq!(f(&rx["summary"]["blocked_area"]), 0.0);
    assert_eq!(face_entry(&rx, back)["status"], "accessible");
}

#[test]
fn enclosed_void_is_blocked_from_everywhere() {
    let m = grid_solid(
        [
            &[0.0, 2.0, 38.0, 40.0],
            &[0.0, 2.0, 18.0, 20.0],
            &[0.0, 2.0, 8.0, 10.0],
        ],
        |i, j, k| (i, j, k) != (1, 1, 1),
    );
    let r = run(&access(m, "+z", "both")).unwrap();
    // The void's floor and roof, 2·36·16.
    close(f(&r["summary"]["blocked_area"]), 2.0 * 36.0 * 16.0, TOL);
    close(f(&r["summary"]["accessible_area"]), 1600.0, TOL);
}

#[test]
fn direction_from_a_face_normal() {
    let m = slotted_block();
    let mut req = access(m, "+z", "one");
    req.inputs.insert(
        "direction".into(),
        Resolved::Face(FaceGeom {
            frame: Frame::from_normal([40.0, 0.0, 0.0], [3.0, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap(),
            body: 7,
            loops: vec![],
            body_span: [-40.0, 0.0],
            parallel: vec![],
        }),
    );
    let r = run(&req).unwrap();
    assert_eq!(r["direction_source"], "face");
    assert_eq!(r["direction"], serde_json::json!([1.0, 0.0, 0.0]));
    assert_eq!(f(&r["summary"]["blocked_area"]), 0.0);
}

#[test]
fn cylinder_radius_is_exact_and_holes_are_concave() {
    let m = revolved(8, 10.0, 10.0, 5.0, cylinder(10.0, true));
    let r = run(&radius(m.clone(), 12.0)).unwrap();
    let e = face_entry(&r, 2);
    assert_eq!(e["method"], "cylinder");
    assert_eq!(e["exact"], true);
    assert_eq!(f(&e["radius"]), 10.0);
    assert_eq!(e["concave"], false);
    assert_eq!(e["below"], true);
    let wall: f64 = (0..m.triangles.len())
        .filter(|&t| m.tri_face[t] == 2)
        .map(|t| {
            let [a, b, c] = m.triangles[t].map(|v| m.vertices[v as usize]);
            let (u, v) = (
                [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
            );
            let x = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            0.5 * (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt()
        })
        .sum();
    close(f(&e["area_below"]), wall, 1e-9);
    // Planes have no radius.
    assert_eq!(face_entry(&r, 0)["radius"], Value::Null);
    assert_eq!(face_entry(&r, 0)["method"], "plane");
    let s = &r["summary"];
    assert_eq!(f(&s["min_radius"]), 10.0);
    assert_eq!(s["flagged_faces"], 1);
    assert_eq!(s["exact_faces"], 3);
    let tr = r["triangles"]["radius"].as_array().unwrap();
    assert_eq!(tr[0], Value::Null);
    assert_eq!(f(&tr[2]), 10.0);
    // Below a 5 mm minimum nothing is flagged.
    let r5 = run(&radius(m.clone(), 5.0)).unwrap();
    assert_eq!(r5["summary"]["flagged_faces"], 0);
    assert_eq!(face_entry(&r5, 2)["below"], false);
    // A hole surface (material outside the radius) is concave.
    let mut hole = m;
    hole.faces[2].surface = cylinder(10.0, false);
    let rh = run(&radius(hole, 5.0)).unwrap();
    assert_eq!(face_entry(&rh, 2)["concave"], true);
}

#[test]
fn cone_radius_at_its_narrow_end() {
    let (r0, r1, h) = (10.0f64, 6.0, 20.0);
    let half = ((r0 - r1) / h).atan();
    let cone = Surface::Cone {
        point: [0.0, 0.0, r0 / ((r0 - r1) / h)],
        axis: [0.0, 0.0, 1.0],
        half_angle: half,
    };
    let r = run(&radius(revolved(32, r0, r1, h, cone), 1.0)).unwrap();
    let e = face_entry(&r, 2);
    assert_eq!(e["method"], "cone");
    assert_eq!(e["exact"], false);
    assert_eq!(e["concave"], false);
    // Circumferential radius ρ / cos(α) at the top ring ρ = 6.
    close(f(&e["radius"]), r1 / half.cos(), 1e-9);
    close(f(&e["location"][2]), h, TOL);
}

#[test]
fn faceted_sphere_radius_is_estimated_within_one_percent() {
    let rad = 10.0;
    let m = icosphere(rad, 3);
    assert_eq!(m.triangles.len(), 1280);
    let r = run(&radius(m, 11.0)).unwrap();
    assert_eq!(r["mesh"]["closed"], true);
    let e = face_entry(&r, 0);
    assert_eq!(e["method"], "mesh");
    assert_eq!(e["exact"], false);
    assert_eq!(e["concave"], false);
    let est = f(&e["radius"]);
    // Level 3 facets span β ≈ 0.16 rad: β²/6 under 0.5 % low (observed
    // 0.33-0.38 %); the minimum over edges stays within 1 %.
    assert!(est <= rad && est > 0.99 * rad, "estimate {est}");
    // Every triangle has an estimate, all within the same bound.
    for v in r["triangles"]["radius"].as_array().unwrap() {
        let v = f(v);
        assert!(v > 0.99 * rad && v <= rad, "triangle estimate {v}");
    }
    assert_eq!(e["below"], true);
    close(
        f(&r["summary"]["flagged_area"]),
        f(&r["mesh"]["area"]),
        1e-9,
    );
    assert_eq!(r["summary"]["approximate_faces"], 1);
    assert!(
        r["approximation"]["note"]
            .as_str()
            .unwrap()
            .contains("0.2 mm")
    );
}

#[test]
fn faceted_cylinder_without_a_surface_is_estimated() {
    // 64 segments labelled Other: across the quads θ = 2π/64 over a width of
    // 2r·sin(π/64), so the estimate is r·sin(π/64)/(π/64).
    let rad = 10.0f64;
    let n = 64.0;
    let m = revolved(64, rad, rad, 5.0, Surface::Other);
    let r = run(&radius(m, 1.0)).unwrap();
    let e = face_entry(&r, 2);
    let x = std::f64::consts::PI / n;
    close(f(&e["radius"]), rad * x.sin() / x, 1e-9);
    assert_eq!(e["below"], false);
}

fn input_error(r: Result<Value, PluginError>) -> String {
    match r {
        Err(PluginError::Input(m)) => m,
        other => panic!("expected an input error, got {other:?}"),
    }
}

#[test]
fn invalid_requests_are_refused() {
    let good = block(4.0, 2.0, 1.0);
    // Bad parameters.
    assert!(input_error(run(&draft(good.clone(), 0.0, "+z"))).contains("minimum draft"));
    let mut req = draft(good.clone(), 5.0, "+z");
    req.params.insert("max_draft_deg".into(), 4.0);
    assert!(input_error(run(&req)).contains("colour range"));
    let mut req = draft(good.clone(), 1.0, "+z");
    req.params.remove("min_draft_deg");
    assert!(input_error(run(&req)).contains("min_draft_deg"));
    assert!(input_error(run(&radius(good.clone(), 0.0))).contains("positive"));
    assert!(input_error(run(&radius(good.clone(), f64::NAN))).contains("min_radius"));
    // Bad choices.
    assert!(input_error(run(&draft(good.clone(), 1.0, "up"))).contains("pull axis"));
    assert!(input_error(run(&access(good.clone(), "sideways", "one"))).contains("direction axis"));
    assert!(input_error(run(&access(good.clone(), "+z", "three"))).contains("directions"));
    let mut req = access(good.clone(), "+z", "one");
    req.choices.remove("both_directions");
    assert!(input_error(run(&req)).contains("both_directions"));
    // Missing body or mesh.
    let mut req = radius(good.clone(), 1.0);
    req.inputs.clear();
    assert!(input_error(run(&req)).contains("missing"));
    let mut req = access(good.clone(), "+z", "one");
    req.bodies[0].body = 8;
    assert!(input_error(run(&req)).contains("no mesh"));
    // Bad meshes, with the analysis named in the message.
    let mut m = good.clone();
    for t in &mut m.triangles {
        t.swap(1, 2);
    }
    let e = input_error(run(&draft(m, 1.0, "+z")));
    assert!(
        e.starts_with("draft analysis:") && e.contains("outward"),
        "{e}"
    );
    let mut m = good.clone();
    m.tri_face[0] = 99;
    let e = input_error(run(&access(m, "+z", "one")));
    assert!(
        e.starts_with("accessibility:") && e.contains("missing face"),
        "{e}"
    );
    let mut m = good.clone();
    m.vertices[0][0] = f64::INFINITY;
    let e = input_error(run(&radius(m, 1.0)));
    assert!(
        e.starts_with("minimum radius:") && e.contains("non-finite"),
        "{e}"
    );
    // Invalid exact surfaces.
    let mut m = revolved(8, 10.0, 10.0, 5.0, cylinder(-1.0, true));
    assert!(input_error(run(&radius(m.clone(), 1.0))).contains("cylinder"));
    m.faces[2].surface = Surface::Cone {
        point: [0.0; 3],
        axis: [0.0, 0.0, 1.0],
        half_angle: 2.0,
    };
    assert!(input_error(run(&draft(m.clone(), 1.0, "+z"))).contains("half-angle"));
    assert!(input_error(run(&radius(m, 1.0))).contains("half-angle"));
}

/// ~20k triangles: a 40×20×10 block on a 60×40×25 cell grid.
fn fine_block() -> BodyMesh {
    let axis = |len: f64, n: usize| {
        (0..=n)
            .map(|i| len * i as f64 / n as f64)
            .collect::<Vec<_>>()
    };
    let (xs, ys, zs) = (axis(40.0, 60), axis(20.0, 40), axis(10.0, 25));
    grid_solid([&xs, &ys, &zs], |_, _, _| true)
}

#[test]
fn twenty_thousand_triangles_analyse_quickly() {
    let m = fine_block();
    assert_eq!(m.triangles.len(), 19_600);
    let mut sphere = icosphere(10.0, 5);
    assert_eq!(sphere.triangles.len(), 20_480);
    sphere.body = 7;
    for req in [
        draft(m.clone(), 1.0, "+z"),
        access(m.clone(), "+z", "both"),
        radius(sphere, 1.0),
    ] {
        let start = std::time::Instant::now();
        let r = run(&req).unwrap();
        let took = start.elapsed();
        assert!(r["mesh"]["closed"] == true);
        // Unoptimised (dev profile) bound, as for Design Advice.
        assert!(took.as_secs_f64() < 2.0, "{}: took {took:?}", req.analysis);
    }
    let r = run(&access(m, "+z", "both")).unwrap();
    close(f(&r["summary"]["accessible_area"]), 1600.0, 1e-9);
}
