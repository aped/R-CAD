//! Draft (docs/PLASTICS.md "Draft"): Fusion's fixed-plane draft of existing
//! faces. Each selected planar face is turned about its line on the fixed
//! plane by `draft_deg` (relative to its current orientation), leaning in as
//! it goes along the pull direction (away from the fixed face, into the
//! part; `reverse`: the other way). Built with the kernel's exact face
//! replacement, which keeps topology (one operation per face, id = its
//! position in the selection).
use crate::util::{invalid, unsupported};
use crate::{choice, input, param};
use rcad_plugin::geom::{Curve, SketchGeom};
use rcad_plugin::geom::{add, cross, dot, norm, scale, sub};
use rcad_plugin::schema::FeatureSchema;
use rcad_plugin::{BuildRequest, InputKind, Op, ParamKind, PluginError};

const F: &str = "draft";

pub fn schema() -> FeatureSchema {
    FeatureSchema {
        id: F.into(),
        name: "Draft".into(),
        description: "Fixed-plane draft of planar faces (relative angle)".into(),
        inputs: vec![
            input("faces", "Faces to draft", InputKind::Faces, false),
            input("fixed", "Fixed plane (face)", InputKind::Face, false),
            input(
                "parting",
                "Parting line (sketch lines)",
                InputKind::Sketch,
                true,
            ),
        ],
        params: vec![param(
            "draft_deg",
            "Draft angle",
            ParamKind::Angle,
            1.0,
            Some((0.0, true)),
            Some(30.0),
            Some("DA"),
        )],
        choices: vec![
            choice("direction", "Direction", &["pull", "reverse"]),
            choice("sides", "Draft sides", &["one_side", "two_sides"]),
        ],
    }
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    scale(v, 1.0 / norm(v))
}

pub fn build(req: &BuildRequest) -> Result<Vec<Op>, PluginError> {
    let a = req.param("draft_deg")?.to_radians();
    let sign = if req.choice("direction")? == "reverse" {
        -1.0
    } else {
        1.0
    };
    let fixed = req.face("fixed")?;
    let faces = req.faces("faces")?;
    if faces.is_empty() {
        return Err(invalid(F, "select at least one face"));
    }
    let nf = fixed.frame.z;
    // Pull: away from the fixed face, into the part.
    let pull = scale(nf, -1.0);
    if req.has("parting") {
        // Parting line from a sketch: its lines, end to end, swept along
        // the sketch normal (the kernel's broken parting-line draft).
        if faces.iter().any(|f| f.body != fixed.body) {
            return Err(invalid(F, "the faces must be on the fixed plane's body"));
        }
        let sk = req.sketch("parting")?;
        let two = req.choice("sides")? == "two_sides";
        return Ok(vec![Op::DraftFaces {
            id: 0,
            body: "fixed".into(),
            faces: faces.iter().map(|f| [f.frame.origin, f.frame.z]).collect(),
            plane_point: fixed.frame.origin,
            plane_normal: nf,
            pull: scale(pull, sign),
            angle_deg: a.to_degrees(),
            two_sided: two,
            parting: polyline(sk)?
                .into_iter()
                .map(|p| sk.frame.point(p, 0.0))
                .collect(),
            parting_across: sk.frame.z,
            label: "parting-line draft".into(),
        }]);
    }
    if req.choice("sides")? == "two_sides" {
        // Parting line: faces crossing the plane are split there and each
        // side leans in going away from it (kernel polyhedral draft).
        if faces.iter().any(|f| f.body != fixed.body) {
            return Err(invalid(F, "the faces must be on the fixed plane's body"));
        }
        return Ok(vec![Op::DraftFaces {
            id: 0,
            body: "fixed".into(),
            faces: faces.iter().map(|f| [f.frame.origin, f.frame.z]).collect(),
            plane_point: fixed.frame.origin,
            plane_normal: nf,
            pull: scale(pull, sign),
            angle_deg: a.to_degrees(),
            two_sided: true,
            parting: Vec::new(),
            parting_across: [0.0; 3],
            label: "two-sided draft".into(),
        }]);
    }
    let mut ops = Vec::new();
    for (i, f) in faces.iter().enumerate() {
        if f.body != fixed.body {
            return Err(invalid(
                F,
                format!("face {} is on another body than the fixed plane", i + 1),
            ));
        }
        let nw = f.frame.z;
        let l = cross(nw, nf);
        if norm(l) < 1e-9 {
            return Err(invalid(
                F,
                format!("face {} is parallel to the fixed plane", i + 1),
            ));
        }
        let l = unit(l);
        // A point on both planes (the hinge line).
        let (d1, d2) = (dot(nw, f.frame.origin), dot(nf, fixed.frame.origin));
        let ll = dot(cross(nw, nf), cross(nw, nf));
        let p0 = scale(
            add(
                scale(cross(nf, cross(nw, nf)), d1),
                scale(cross(cross(nw, nf), nw), d2),
            ),
            1.0 / ll,
        );
        // Turn the normal about the hinge towards the pull (the face leans
        // in going along it).
        let mut t = cross(l, nw);
        if dot(t, pull) < 0.0 {
            t = scale(t, -1.0);
        }
        let n = unit(add(scale(nw, a.cos()), scale(t, sign * a.sin())));
        let _ = sub;
        ops.push(Op::ReplaceFace {
            id: i as u32,
            face: "faces".into(),
            index: i as u32,
            origin: p0,
            normal: n,
            label: format!("draft face {}", i + 1),
        });
    }
    Ok(ops)
}

/// The sketch's lines joined end to end into one open polyline.
fn polyline(sk: &SketchGeom) -> Result<Vec<[f64; 2]>, PluginError> {
    let mut segs = Vec::new();
    for c in &sk.curves {
        match c {
            Curve::Line { a, b } => segs.push((*a, *b)),
            _ => {
                return Err(unsupported(
                    F,
                    "parting lines of arcs or circles are not supported (straight lines only: \
                     the kernel drafts about planar strips)",
                ));
            }
        }
    }
    if segs.is_empty() {
        return Err(invalid(F, "the parting sketch has no lines"));
    }
    let close = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) <= 1e-6;
    // Ends touched by one segment only; joints by two.
    let touching = |p: [f64; 2]| {
        segs.iter()
            .filter(|(a, b)| close(*a, p) || close(*b, p))
            .count()
    };
    let start = segs
        .iter()
        .flat_map(|(a, b)| [*a, *b])
        .find(|p| touching(*p) == 1)
        .ok_or_else(|| invalid(F, "the parting line must be an open chain of lines"))?;
    let mut out = vec![start];
    let mut used = vec![false; segs.len()];
    while let Some(k) = {
        let last = *out.last().expect("start");
        (0..segs.len()).find(|&k| !used[k] && (close(segs[k].0, last) || close(segs[k].1, last)))
    } {
        used[k] = true;
        let last = *out.last().expect("start");
        let (a, b) = segs[k];
        out.push(if close(a, last) { b } else { a });
    }
    if used.iter().any(|u| !u)
        || segs
            .iter()
            .any(|(a, b)| touching(*a) > 2 || touching(*b) > 2)
    {
        return Err(invalid(
            F,
            "the parting line must be one open chain of lines, end to end",
        ));
    }
    Ok(out)
}
