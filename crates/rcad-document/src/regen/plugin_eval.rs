//! Regeneration of plugin features ([`FeatureKind::Plugin`], docs/PLUGINS.md):
//! resolve the inputs, ask the plugin for its operations, check them and
//! apply each through the same checked tool path as an Extrude Join/Cut
//! (ADR 0005 result gate; tapered tools through `tapered_apply`).
use super::*;
use crate::PluginInput;
use rcad_plugin as rp;

fn to_plugin_frame(f: &Frame) -> rp::Frame {
    rp::Frame {
        origin: f.origin,
        x: f.x,
        y: f.y,
        z: f.z,
    }
}

fn from_plugin_frame(f: &rp::Frame) -> Frame {
    Frame {
        origin: f.origin,
        x: f.x,
        y: f.y,
        z: f.z,
    }
}

fn plugin_profile(p: &SketchProfile) -> Option<rp::Profile> {
    rp::Profile::from_sketch(p)
}

/// Planar faces of `body` parallel to `frame` (exact heights along its z,
/// footprints in its plane coordinates).
fn parallel_planes(id: FeatureId, body: &Body, frame: &Frame) -> Result<Vec<rp::ParallelPlane>> {
    Ok(k::parallel_faces(body, frame)
        .map_err(kernel(id))?
        .into_iter()
        .map(|f| rp::ParallelPlane {
            height: f.height,
            facing: f.facing,
            footprint: Some([f.min, f.max]),
        })
        .collect())
}

/// Every planar face of `body` with its sampled boundary.
fn planar_faces(id: FeatureId, body: &Body) -> Result<Vec<rp::PlanarFace>> {
    Ok(k::planar_face_outlines(body)
        .map_err(kernel(id))?
        .into_iter()
        .map(|f| rp::PlanarFace {
            point: f.point,
            normal: f.normal,
            loops: f.loops,
        })
        .collect())
}

/// A resolved placement face.
fn face_geom(id: FeatureId, frame: &Frame, body: BodyId, st: &Snapshot) -> Result<rp::FaceGeom> {
    let t = brep_target(st, id, body)?;
    let (lo, hi) = k::extent_along(&t, frame).map_err(kernel(id))?;
    // Boundary loops as exact lines/arcs; empty when the face has other
    // curves (the plugin then works without them).
    let loops = k::face_boundary_curves(&t, frame)
        .ok()
        .and_then(|ls| {
            ls.iter()
                .map(|l| curves_to_profile(l).ok().and_then(|p| plugin_profile(&p)))
                .collect::<Option<Vec<_>>>()
        })
        .unwrap_or_default();
    Ok(rp::FaceGeom {
        frame: to_plugin_frame(frame),
        body: body.0,
        loops,
        body_span: [lo, hi],
        parallel: parallel_planes(id, &t, frame)?,
    })
}

fn sketch_geom(id: FeatureId, sketch: FeatureId, st: &Snapshot) -> Result<rp::SketchGeom> {
    let so = st.sketches.get(&sketch).ok_or(not_a_sketch(id, sketch))?;
    let s = &so.solved;
    let pt = |p: rcad_sketch::PointId| -> Result<[f64; 2]> {
        s.points
            .iter()
            .find(|q| q.id == p)
            .map(|q| [q.x, q.y])
            .ok_or_else(|| {
                DocError::Validation(format!("{id}: sketch {sketch} has a dangling point"))
            })
    };
    let mut curves = Vec::new();
    let mut used = BTreeSet::new();
    for e in &s.entities {
        let eid = e.id();
        for p in e.points() {
            used.insert(p);
        }
        if s.construction.contains(&eid) {
            continue;
        }
        match e {
            rcad_sketch::Entity::Line { a, b, .. } => curves.push(rp::Curve::Line {
                a: pt(*a)?,
                b: pt(*b)?,
            }),
            rcad_sketch::Entity::Circle { center, radius, .. } => {
                curves.push(rp::Curve::Circle {
                    center: pt(*center)?,
                    radius: *radius,
                });
            }
            rcad_sketch::Entity::Arc {
                center, start, end, ..
            } => {
                let (c, a, b) = (pt(*center)?, pt(*start)?, pt(*end)?);
                curves.push(rp::Curve::Arc {
                    center: c,
                    radius: (a[0] - c[0]).hypot(a[1] - c[1]),
                    start: (a[1] - c[1]).atan2(a[0] - c[0]),
                    end: (b[1] - c[1]).atan2(b[0] - c[0]),
                });
            }
            _ => {
                return Err(DocError::ProductLimit(format!(
                    "{id}: sketch {sketch} has an ellipse or spline; plugin features read lines, \
                     arcs, circles and points"
                )));
            }
        }
    }
    // Standalone points (no entity uses them), not the sketch origin: the
    // placement points of point-based features (Fusion's sketch points).
    let points = s
        .points
        .iter()
        .filter(|p| !used.contains(&p.id) && Some(p.id) != s.origin)
        .map(|p| [p.x, p.y])
        .collect();
    let body = so_face_body(st, sketch);
    let (parallel, faces) = match body {
        Some(b) => {
            let t = brep_target(st, id, BodyId(b))?;
            (parallel_planes(id, &t, &so.frame)?, planar_faces(id, &t)?)
        }
        None => (vec![], vec![]),
    };
    Ok(rp::SketchGeom {
        frame: to_plugin_frame(&so.frame),
        curves,
        points,
        body,
        parallel,
        faces,
    })
}

/// The body whose face the sketch lies on, if any.
fn so_face_body(st: &Snapshot, sketch: FeatureId) -> Option<u32> {
    let i = *st.index.get(&sketch)?;
    st.results.get(i)?.resolved_face.as_ref().map(|r| r.body.0)
}

/// Resolve every declared input of `schema` from `inputs`.
fn resolve_inputs(
    id: FeatureId,
    schema: &[rp::InputSchema],
    inputs: &BTreeMap<String, PluginInput>,
    st: &Snapshot,
    r: &mut FeatureResult,
) -> Result<BTreeMap<String, rp::Resolved>> {
    for n in inputs.keys() {
        if !schema.iter().any(|s| &s.name == n) {
            return Err(DocError::Validation(format!("{id}: unknown input '{n}'")));
        }
    }
    let mut out = BTreeMap::new();
    for s in schema {
        let Some(i) = inputs.get(&s.name) else {
            if s.optional {
                continue;
            }
            return Err(DocError::Validation(format!(
                "{id}: input '{}' ({}) is required",
                s.name, s.label
            )));
        };
        let wrong =
            || DocError::Validation(format!("{id}: input '{}' must be {:?}", s.name, s.kind));
        let v = match (s.kind, i) {
            (rp::InputKind::Face, PluginInput::Face { plane }) => {
                let (frame, rf) = plane_frame(id, plane, st)?;
                let Some(rf) = rf else {
                    return Err(DocError::Validation(format!(
                        "{id}: input '{}' must be a face of a body (or a plane offset from one)",
                        s.name
                    )));
                };
                if r.resolved_face.is_none() {
                    r.resolved_face = Some(rf.clone());
                    r.frame = Some(frame);
                }
                rp::Resolved::Face(face_geom(id, &frame, rf.body, st)?)
            }
            (rp::InputKind::Faces, PluginInput::Faces { faces }) => {
                let mut v = Vec::new();
                for f in faces {
                    let rf = resolve_face(id, f, st)?;
                    v.push(face_geom(id, &rf.frame, rf.body, st)?);
                }
                rp::Resolved::Faces { faces: v }
            }
            (rp::InputKind::Body, PluginInput::Body { body }) => {
                let t = brep_target(st, id, *body)?;
                let b = k::bounds(&t).map_err(kernel(id))?;
                rp::Resolved::Body(rp::BodyGeom {
                    body: body.0,
                    bounds: [b.min, b.max],
                    faces: planar_faces(id, &t)?,
                })
            }
            (rp::InputKind::Sketch, PluginInput::Sketch { sketch }) => {
                rp::Resolved::Sketch(sketch_geom(id, *sketch, st)?)
            }
            (rp::InputKind::Points, PluginInput::Points { points }) => {
                if points.iter().flatten().any(|v| !v.is_finite()) {
                    return Err(DocError::Validation(format!(
                        "{id}: input '{}': points must be finite",
                        s.name
                    )));
                }
                rp::Resolved::Points {
                    points: points.clone(),
                }
            }
            _ => return Err(wrong()),
        };
        out.insert(s.name.clone(), v);
    }
    Ok(out)
}

fn plugin_error(id: FeatureId, e: rp::PluginError) -> DocError {
    match e {
        rp::PluginError::Input(m) | rp::PluginError::Manifest(m) | rp::PluginError::NotFound(m) => {
            DocError::Validation(format!("{id}: {m}"))
        }
        rp::PluginError::Unsupported(m) => DocError::ProductLimit(format!("{id}: {m}")),
        rp::PluginError::Failed(m) => DocError::Kernel {
            feature: id,
            detail: format!("plugin failure: {m}"),
        },
    }
}

/// The installed plugin and feature schema of a plugin feature.
pub(crate) fn schema_of(
    id: FeatureId,
    plugin: &str,
    feature: &str,
) -> Result<(Arc<dyn rp::Plugin>, rp::FeatureSchema)> {
    let p = rp::find(plugin).ok_or_else(|| {
        DocError::ProductLimit(format!(
            "{id}: plugin '{plugin}' is not installed; the feature keeps its definition and \
             regenerates once the plugin is installed"
        ))
    })?;
    let s = p.manifest().feature(feature).cloned().ok_or_else(|| {
        DocError::ProductLimit(format!(
            "{id}: plugin '{plugin}' ({}) has no feature '{feature}'",
            p.manifest().version
        ))
    })?;
    Ok((p, s))
}

/// Evaluate a plugin feature.
#[allow(clippy::too_many_arguments)]
pub(super) fn eval_plugin(
    doc: &Document,
    id: FeatureId,
    plugin: &str,
    feature: &str,
    inputs: &BTreeMap<String, PluginInput>,
    params: &BTreeMap<String, crate::PluginParam>,
    choices: &BTreeMap<String, String>,
    st: &mut Snapshot,
    r: &mut FeatureResult,
) -> Result<()> {
    let (p, schema) = schema_of(id, plugin, feature)?;
    let given: BTreeMap<String, f64> = params.iter().map(|(n, v)| (n.clone(), v.value)).collect();
    let (params, choices) =
        rp::schema::complete(&schema.id, &schema.params, &schema.choices, &given, choices)
            .map_err(|e| plugin_error(id, e))?;
    let inputs = resolve_inputs(id, &schema.inputs, inputs, st, r)?;
    let req = rp::BuildRequest {
        feature: schema.id.clone(),
        params,
        choices,
        inputs,
        data: doc
            .plugin_data
            .get(plugin)
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    };
    let t = std::time::Instant::now();
    let ops = p.build(&req).map_err(|e| plugin_error(id, e))?;
    trace_regen(id, "plugin build", t);
    rp::check_ops(&req, &ops).map_err(|e| plugin_error(id, e))?;
    if ops.is_empty() {
        return Err(DocError::ProductLimit(format!(
            "{id}: {plugin}.{feature} produced no geometry for these inputs"
        )));
    }
    let mut tools = Vec::with_capacity(ops.len());
    for op in &ops {
        if let rp::Op::DraftFaces {
            body,
            faces,
            plane_point,
            plane_normal,
            pull,
            angle_deg,
            two_sided,
            parting,
            parting_across,
            label,
            ..
        } = op
        {
            let target =
                BodyId(rp::target_body(&req, body).ok_or_else(|| {
                    DocError::Validation(format!("{id}: {label}: no target body"))
                })?);
            let t = brep_target(st, id, target)?;
            let kinds = k::classify_faces(&t).map_err(kernel(id))?;
            let mut idx = Vec::new();
            for fp in faces {
                let (o, n) = (fp[0], fp[1]);
                let hits: Vec<usize> = kinds
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| match f {
                        k::DirectFace::Plane { origin, normal }
                            if dot(*normal, n) > 1.0 - 1e-9
                                && dot(sub(*origin, o), n).abs() <= k::tol::FACE_MATCH =>
                        {
                            Some(i)
                        }
                        _ => None,
                    })
                    .collect();
                let [fi] = hits.as_slice() else {
                    return Err(DocError::BrokenReference {
                        from: id,
                        detail: format!(
                            "{label}: {} faces of {target} lie in a face's plane; refusing to guess",
                            hits.len()
                        ),
                    });
                };
                idx.push(*fi);
            }
            let out = if parting.is_empty() {
                k::draft_faces(
                    &t,
                    &idx,
                    *plane_point,
                    *plane_normal,
                    *pull,
                    angle_deg.to_radians(),
                    *two_sided,
                )
            } else {
                k::draft_faces_parting(
                    &t,
                    &idx,
                    parting,
                    *parting_across,
                    *pull,
                    angle_deg.to_radians(),
                    *two_sided,
                )
            }
            .map_err(|e| match e {
                k::KernelError::InvalidInput(m) => {
                    DocError::ProductLimit(format!("{id}: {label}: {m}"))
                }
                other => DocError::Kernel {
                    feature: id,
                    detail: format!("{label}: {other}"),
                },
            })?;
            replace_body(st, id, target, out);
            if r.body.is_none() {
                r.body = Some(target);
            }
            continue;
        }
        if let rp::Op::EdgeFillet {
            body,
            a,
            b,
            radius,
            label,
            ..
        } = op
        {
            let target =
                BodyId(rp::target_body(&req, body).ok_or_else(|| {
                    DocError::Validation(format!("{id}: {label}: no target body"))
                })?);
            let t = brep_target(st, id, target)?;
            let plane = |p: &[[f64; 3]; 2]| {
                rp::Frame::from_normal(p[0], p[1], [1.0, 0.0, 0.0])
                    .map(|f| from_plugin_frame(&f))
                    .ok_or_else(|| DocError::Validation(format!("{id}: {label}: degenerate plane")))
            };
            let out = k::fillet_edge(&t, &plane(a)?, &plane(b)?, *radius).map_err(|e| match e {
                k::KernelError::InvalidInput(m) => {
                    DocError::ProductLimit(format!("{id}: {label}: {m}"))
                }
                other => DocError::Kernel {
                    feature: id,
                    detail: format!("{label}: {other}"),
                },
            })?;
            replace_body(st, id, target, out);
            continue;
        }
        if let rp::Op::RootRound {
            body,
            at,
            axis,
            boss_diameter,
            radius,
            draft_deg,
            label,
            ..
        } = op
        {
            let target =
                BodyId(rp::target_body(&req, body).ok_or_else(|| {
                    DocError::Validation(format!("{id}: {label}: no target body"))
                })?);
            let t = brep_target(st, id, target)?;
            let out = k::root_round(
                &t,
                *at,
                *axis,
                *boss_diameter,
                *radius,
                draft_deg.to_radians(),
            )
            .map_err(|e| match e {
                k::KernelError::InvalidInput(m) => {
                    DocError::ProductLimit(format!("{id}: {label}: {m}"))
                }
                other => DocError::Kernel {
                    feature: id,
                    detail: format!("{label}: {other}"),
                },
            })?;
            replace_body(st, id, target, out);
            continue;
        }
        if let rp::Op::Revolve {
            body,
            frame,
            profile,
            angle_deg,
            mode,
            label,
            ..
        } = op
        {
            let target =
                BodyId(rp::target_body(&req, body).ok_or_else(|| {
                    DocError::Validation(format!("{id}: {label}: no target body"))
                })?);
            let t = brep_target(st, id, target)?;
            let frame = from_plugin_frame(frame);
            let prof = profile
                .to_sketch()
                .map_err(|m| DocError::Validation(format!("{id}: {label}: {m}")))?;
            let angle = angle_deg.to_radians();
            let tool =
                k::revolve(&to_kernel(&prof), &frame, angle).map_err(|e| DocError::Kernel {
                    feature: id,
                    detail: format!("{label}: revolve: {e}"),
                })?;
            let op = match mode {
                rp::Mode::Join => k::CombineOp::Join,
                rp::Mode::Cut => k::CombineOp::Cut,
            };
            // As for revolved Join/Cut features: a full turn is the same
            // solid whatever its seam, so retry with the seam turned.
            let full = angle >= 2.0 * std::f64::consts::PI - 1e-12;
            let turns: &[f64] = if full {
                &[0.0, 0.37, 1.13, 2.71]
            } else {
                &[0.0]
            };
            let mut last = None;
            let mut done = None;
            for &a in turns {
                let tl = if a == 0.0 {
                    tool.clone()
                } else {
                    let rot =
                        k::Rigid::new([0.0; 3], frame.origin, frame.y, a).map_err(kernel(id))?;
                    k::transform_body(&tool, &rot).map_err(kernel(id))?
                };
                match k::combine(&t, std::slice::from_ref(&tl), op) {
                    Ok(x) => {
                        done = Some(x);
                        break;
                    }
                    Err(e) => last = Some(e),
                }
            }
            let (out, rep) = done.ok_or_else(|| DocError::Kernel {
                feature: id,
                detail: format!(
                    "{label}: {}",
                    last.map(|e| e.to_string()).unwrap_or_default()
                ),
            })?;
            if !rep.passed() {
                return Err(DocError::GeometryCheck {
                    feature: id,
                    detail: format!("{label}: {}; refused", rep.failure().unwrap_or_default()),
                });
            }
            replace_body(st, id, target, out);
            if r.body.is_none() {
                r.body = Some(target);
            }
            continue;
        }
        if let rp::Op::Countersink {
            body,
            at,
            axis,
            hole_diameter,
            diameter,
            angle_deg,
            label,
            ..
        } = op
        {
            let target =
                BodyId(rp::target_body(&req, body).ok_or_else(|| {
                    DocError::Validation(format!("{id}: {label}: no target body"))
                })?);
            let t = brep_target(st, id, target)?;
            let out = k::countersink(
                &t,
                *at,
                *axis,
                *hole_diameter,
                *diameter,
                angle_deg.to_radians(),
            )
            .map_err(|e| match e {
                k::KernelError::InvalidInput(m) => {
                    DocError::ProductLimit(format!("{id}: {label}: {m}"))
                }
                other => DocError::Kernel {
                    feature: id,
                    detail: format!("{label}: {other}"),
                },
            })?;
            replace_body(st, id, target, out);
            continue;
        }
        if let rp::Op::ReplaceFace {
            id: op_id,
            face,
            index,
            origin,
            normal,
            label,
        } = op
        {
            let what = if label.is_empty() {
                format!("operation {op_id}")
            } else {
                label.clone()
            };
            let fg = match &req.inputs[face] {
                rp::Resolved::Face(f) => f.clone(),
                rp::Resolved::Faces { faces } => faces[*index as usize].clone(),
                _ => unreachable!("checked by check_ops"),
            };
            let target = BodyId(fg.body);
            let t = brep_target(st, id, target)?;
            // The B-rep face is the one in the input's (original) plane with
            // its normal: face replacement keeps topology and face order, so
            // the other inputs' faces still match their planes.
            let (o, n) = (fg.frame.origin, fg.frame.z);
            let kinds = k::classify_faces(&t).map_err(kernel(id))?;
            let hits: Vec<usize> = kinds
                .iter()
                .enumerate()
                .filter_map(|(i, f)| match f {
                    k::DirectFace::Plane { origin, normal }
                        if dot(*normal, n) > 1.0 - 1e-9
                            && dot(sub(*origin, o), n).abs() <= k::tol::FACE_MATCH =>
                    {
                        Some(i)
                    }
                    _ => None,
                })
                .collect();
            let [fi] = hits.as_slice() else {
                return Err(DocError::BrokenReference {
                    from: id,
                    detail: format!(
                        "{what}: {} faces of {target} lie in the face's plane; refusing to guess",
                        hits.len()
                    ),
                });
            };
            let out = k::replace_face(&t, *fi, *origin, *normal).map_err(|e| DocError::Kernel {
                feature: id,
                detail: format!("{what}: {e}"),
            })?;
            replace_body(st, id, target, out.body);
            if r.body.is_none() {
                r.body = Some(target);
            }
            continue;
        }
        if let rp::Op::EdgeLoop {
            id: op_id,
            of,
            end,
            blend,
            size,
            holes,
            label,
        } = op
        {
            let what = if label.is_empty() {
                format!("operation {op_id}")
            } else {
                label.clone()
            };
            let tool = &tools
                .iter()
                .find(|(i, _)| i == of)
                .ok_or_else(|| {
                    DocError::Validation(format!("{id}: {what}: operation {of} was not built"))
                })?
                .1;
            let role = if *end {
                FaceRole::ExtrudeEndCap
            } else {
                FaceRole::ExtrudeStartCap
            };
            let frame = extrude_face_frame(tool, &FaceRef { feature: id, role }, id)?;
            let target = tool.body;
            let t = brep_target(st, id, target)?;
            let out = match (blend, holes) {
                (rp::Blend::Fillet, false) => k::fillet_cap_loop(&t, &frame, *size),
                (rp::Blend::Fillet, true) => k::fillet_cap_face_edges(&t, &frame, *size),
                (rp::Blend::Chamfer, false) => k::chamfer_cap_loop(&t, &frame, *size),
                (rp::Blend::Chamfer, true) => k::chamfer_cap_face_edges(&t, &frame, *size),
            }
            .map_err(|e| DocError::Kernel {
                feature: id,
                detail: format!("{what}: {e}"),
            })?;
            replace_body(st, id, target, out);
            continue;
        }
        let rp::Op::Prism {
            id: op_id,
            body,
            frame,
            profile,
            holes,
            from,
            to,
            taper_deg,
            mode,
            label,
        } = op
        else {
            continue;
        };
        let target = BodyId(rp::target_body(&req, body).ok_or_else(|| {
            DocError::Validation(format!("{id}: operation {op_id}: no target body"))
        })?);
        let what = if label.is_empty() {
            format!("operation {op_id}")
        } else {
            label.clone()
        };
        let frame = from_plugin_frame(frame);
        let bad = |m: String| DocError::Validation(format!("{id}: {what}: {m}"));
        let prof = profile.to_sketch().map_err(bad)?;
        let holes: Vec<(SketchProfile, Vec<EntityId>)> = holes
            .iter()
            .map(|h| h.to_sketch().map(|p| (p, vec![])).map_err(bad))
            .collect::<Result<_>>()?;
        let n_edges = match &prof {
            SketchProfile::Polygon(v) => v.len(),
            SketchProfile::Loop(s) => s.len(),
            SketchProfile::Circle { .. } => 1,
        };
        let t = brep_target(st, id, target)?;
        let removes = *mode == rp::Mode::Cut;
        // Outward side of a target face in the operation's plane (height
        // 0). A Join starting there is joined like an extrusion sketched on
        // that face. A Cut's span is explicit (plugins overshoot into air
        // themselves), so it gets no start face; its direction points into
        // the material, so only its far end is moved off a coplanar face.
        let plane_side = k::coplanar_face_side(&t, &frame).map_err(kernel(id))?;
        // A Join counts as on a face only when its profile lies inside a
        // face of that plane (not when an unrelated face merely shares it).
        let (side, dir) = if removes {
            (None, plane_side.map_or(1.0, |s| -s))
        } else {
            let on = plane_side.is_some()
                && k::profile_within_face(&t, &frame, &to_kernel(&prof)).map_err(kernel(id))?;
            (plane_side.filter(|_| on), 1.0)
        };
        let tool = ExtrudeOut {
            start: frame,
            distance: to - from,
            dir,
            extent: Extent::Distance,
            profile: prof,
            // Synthetic entity ids: edge i of the normalised profile, so
            // `ExtrudeSide { entity: i }` names its side face.
            entities: (0..n_edges as u32).map(EntityId).collect(),
            holes,
            span: (*from, *to),
            nominal: (*from, *to),
            side,
            removes,
            joined: !removes,
            intersect: false,
            plain: *from == 0.0 && *taper_deg == 0.0,
            multi: false,
            taper: taper_deg.to_radians(),
            pieces: false,
            body: target,
        };
        let out = if *taper_deg != 0.0 {
            if !tool.holes.is_empty() {
                return Err(DocError::ProductLimit(format!(
                    "{id}: {what}: a drafted tool cannot have inner loops"
                )));
            }
            let op = if removes {
                k::PrismOp::Cut
            } else {
                k::PrismOp::Join
            };
            let shrink = tool.taper;
            tapered_apply(id, &t, &tool, (*from, *to), 1.0, shrink, op)
                .map_err(|e| prefix_error(e, &what))?
        } else {
            // The tool frame is the operation frame; spans are along its z.
            apply_tool(&t, &tool, &frame, Some(&what), id, None)?.0
        };
        replace_body(st, id, target, out);
        if r.body.is_none() {
            r.body = Some(target);
        }
        tools.push((*op_id, tool));
    }
    r.notes.push(format!(
        "{plugin} {} ({}): {} operation(s)",
        schema.name,
        p.manifest().version,
        ops.len()
    ));
    st.plugin_tools.insert(id, tools);
    Ok(())
}

fn prefix_error(e: DocError, what: &str) -> DocError {
    match e {
        DocError::ProductLimit(m) => DocError::ProductLimit(format!("{what}: {m}")),
        DocError::Kernel { feature, detail } => DocError::Kernel {
            feature,
            detail: format!("{what}: {detail}"),
        },
        other => other,
    }
}

/// The tool of operation `op` of plugin feature `feature` ([`FaceRole::Plugin`]).
pub(super) fn plugin_tool(
    st: &Snapshot,
    from: FeatureId,
    feature: FeatureId,
    op: u32,
) -> Result<&ExtrudeOut> {
    let tools = st
        .plugin_tools
        .get(&feature)
        .ok_or(DocError::BrokenReference {
            from,
            detail: format!("{feature} is not an evaluated plugin feature"),
        })?;
    tools
        .iter()
        .find(|(i, _)| *i == op)
        .map(|(_, e)| e)
        .ok_or_else(|| DocError::BrokenReference {
            from,
            detail: format!(
                "operation {op} of {feature} no longer exists (the plugin feature now makes {} \
                 operation(s)); re-attach to an existing face",
                tools.len()
            ),
        })
}
