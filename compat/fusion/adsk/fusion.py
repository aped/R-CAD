"""``adsk.fusion`` subset: design, component, construction planes, sketches,
extrude / shell / fillet, user parameters and the plastic features and
rules (docs/FUSION_SCRIPTS.md). Each supported call appends R-CAD commands
through the recorder; anything else raises NotSupported."""

import math

from adsk import core
from adsk._recorder import (
    ANGLE,
    LENGTH,
    RCAD_LENGTH_UNITS,
    UNITLESS,
    Base as _CoreBase,
    Collection as _CoreCollection,
    Evaluator,
    NotSupported,
    fmt,
    not_in_subset,
    plain_quantity,
    rcad_expression,
    recorder,
    tokenize,
)

# Tolerance for merging coincident curve end points, in mm.
_MERGE_TOL = 1e-9


def __getattr__(name):
    if name.startswith("__"):
        raise AttributeError(name)
    raise not_in_subset("adsk.fusion.%s" % name)


class Base(_CoreBase):
    _module = "adsk.fusion"


class Collection(_CoreCollection):
    _module = "adsk.fusion"

    def itemByName(self, name):
        for o in self._items:
            if getattr(o, "name", None) == name:
                return o
        return None


def _rec():
    return recorder()


def _items(entities, what):
    """Objects of an ObjectCollection, list/tuple or a single object."""
    if isinstance(entities, (core.ObjectCollection, list, tuple)):
        return list(entities)
    if entities is None:
        raise NotSupported("%s: nothing given" % what)
    return [entities]


# ---------------------------------------------------------------------------
# Enumerations


class FeatureOperations:
    JoinFeatureOperation = 0
    CutFeatureOperation = 1
    IntersectFeatureOperation = 2
    NewBodyFeatureOperation = 3
    NewComponentFeatureOperation = 4


class ExtentDirections:
    PositiveExtentDirection = 0
    NegativeExtentDirection = 1
    SymmetricExtentDirection = 2


class DesignTypes:
    DirectDesignType = 0
    ParametricDesignType = 1


class BossHoleExtentTypes:
    BossHoleThrough = 0
    BossBlindFull = 1
    BossBlindDepth = 2


class BossRibShapeTypes:
    BossRibShapeNone = 0
    BossRibShapeChamfer = 1
    BossRibShapeFillet = 2


class BossAlignmentTypes:
    BossAlignFlat = 0
    BossAlignStepOut = 1
    BossAlignStepIn = 2


_OPS = {
    FeatureOperations.JoinFeatureOperation: "join",
    FeatureOperations.CutFeatureOperation: "cut",
    FeatureOperations.IntersectFeatureOperation: "intersect",
    FeatureOperations.NewBodyFeatureOperation: None,
}


# ---------------------------------------------------------------------------
# Design and component


class Design(Base):
    def __init__(self):
        self._root = Component(self)
        self._params = UserParameters()
        self._plastics = _Plastics(self._root)
        _rec().plastics = self._plastics

    @property
    def rootComponent(self):
        return self._root

    @property
    def userParameters(self):
        return self._params

    @property
    def designPlasticRules(self):
        return self._plastics.design

    @property
    def libraryPlasticRules(self):
        return self._plastics.library

    @property
    def designType(self):
        return DesignTypes.ParametricDesignType

    @designType.setter
    def designType(self, value):
        if value != DesignTypes.ParametricDesignType:
            raise NotSupported("R-CAD designs are parametric (DesignTypes.DirectDesignType)")

    @property
    def unitsManager(self):
        return FusionUnitsManager()

    @property
    def fusionUnitsManager(self):
        return FusionUnitsManager()


class FusionUnitsManager(core.UnitsManager):
    _module = "adsk.fusion"


def _reset():
    pass


class Component(Base):
    def __init__(self, design):
        self._design = design
        self._name = "Root"
        self._xy = ConstructionPlane("WorldXY", 0.0, None, None, "XY")
        self._xz = ConstructionPlane("WorldXZ", 0.0, None, None, "XZ")
        self._yz = ConstructionPlane("WorldYZ", 0.0, None, None, "YZ")
        self._planes = ConstructionPlanes()
        self._sketches = Sketches(self)
        self._features = Features(self)
        self._bodies = BRepBodies()
        self._rule = None

    @property
    def name(self):
        return self._name

    @property
    def parentDesign(self):
        return self._design

    @property
    def xYConstructionPlane(self):
        return self._xy

    @property
    def xZConstructionPlane(self):
        return self._xz

    @property
    def yZConstructionPlane(self):
        return self._yz

    @property
    def constructionPlanes(self):
        return self._planes

    @property
    def sketches(self):
        return self._sketches

    @property
    def features(self):
        return self._features

    @property
    def bRepBodies(self):
        return self._bodies

    @property
    def extrudeFeatures(self):
        return self._features.extrudeFeatures

    @property
    def activePlasticRule(self):
        """R-CAD extension (by analogy with ``activeSheetMetalRule``): the
        plastic rule of the component's bodies."""
        return self._rule

    @activePlasticRule.setter
    def activePlasticRule(self, rule):
        if rule is not None and (not isinstance(rule, PlasticRule) or rule._library):
            raise NotSupported(
                "Component.activePlasticRule takes a design rule "
                "(copy a library rule with designPlasticRules.addByCopy first)"
            )
        self._rule = rule


# ---------------------------------------------------------------------------
# Planes and faces


class _Planar(Base):
    """Something a sketch or an offset plane can be placed on."""

    def _offset_base(self):
        """(PlaneBase JSON, offset mm, offset expression or None, host body)."""
        raise NotImplementedError

    def _plane(self):
        """The R-CAD SketchPlane JSON."""
        base, d, expr, _ = self._offset_base()
        if d == 0.0 and expr is None:
            return base
        return {"Offset": {"base": base, "distance": d}}


class ConstructionPlane(_Planar):
    def __init__(self, base, distance, expr, body, name):
        self._base = base  # PlaneBase JSON
        self._distance = distance
        self._expr = expr
        self._body = body
        self._name = name

    @property
    def name(self):
        return self._name

    @name.setter
    def name(self, value):
        self._name = str(value)

    def _offset_base(self):
        return self._base, self._distance, self._expr, self._body


class ConstructionPlaneInput(Base):
    def __init__(self):
        self._def = None

    def setByOffset(self, planarEntity, offset):
        if not isinstance(planarEntity, _Planar):
            raise NotSupported(
                "ConstructionPlaneInput.setByOffset takes a construction plane or a planar "
                "face of an extrusion"
            )
        d, expr = _rec().value(offset, LENGTH, "the plane offset")
        self._def = (planarEntity, d, expr)
        return True


class ConstructionPlanes(Collection):
    def createInput(self, occurrenceForCreation=None):
        if occurrenceForCreation is not None:
            raise NotSupported("ConstructionPlanes.createInput: occurrences are not supported")
        return ConstructionPlaneInput()

    def add(self, input):
        if not isinstance(input, ConstructionPlaneInput) or input._def is None:
            raise NotSupported("ConstructionPlanes.add needs an input set with setByOffset")
        ent, d, expr = input._def
        base, d0, e0, body = ent._offset_base()
        if expr is None and e0 is None:
            total_expr = None
        else:
            total_expr = "(%s) + (%s)" % (e0 or fmt(d0), expr or fmt(d))
            if e0 is None and d0 == 0.0:
                total_expr = expr
        p = ConstructionPlane(base, d0 + d, total_expr, body, "Plane%d" % (self.count + 1))
        self._items.append(p)
        return p


class BRepFace(_Planar):
    def __init__(self, feature, role, body):
        self._feature = feature
        self._role = role  # FaceRole JSON
        self._body = body

    def _ref(self):
        return {"feature": self._feature, "role": self._role}

    def _offset_base(self):
        return {"Face": self._ref()}, 0.0, None, self._body.id if self._body else None

    @property
    def body(self):
        return self._body

    @property
    def edges(self):
        return BRepEdges(self)


class BRepFaces(Collection):
    pass


class BRepEdges(Base):
    """The edges of a face. Only the whole set can be used (a fillet of
    every edge of the face); single edges are not addressable."""

    def __init__(self, face):
        self._face = face

    def _single(self, *_):
        raise NotSupported(
            "single B-rep edges are not addressable in R-CAD's Fusion API subset; "
            "pass the face itself (all of its edges) to the fillet"
        )

    item = _single
    __iter__ = _single

    @property
    def count(self):
        self._single()


class BRepBody(Base):
    def __init__(self, bid, name, component):
        self._id = bid
        self._name = name
        self._component = component

    @property
    def id(self):
        return self._id

    @property
    def name(self):
        return self._name

    @name.setter
    def name(self, value):
        self._name = str(value)
        _rec().add({"cmd": "rename_body", "body": self._id, "name": self._name})

    @property
    def parentComponent(self):
        return self._component


class BRepBodies(Collection):
    pass


# ---------------------------------------------------------------------------
# Sketches


class SketchPoint(Base):
    def __init__(self, sketch, pid, x, y):
        self._sketch = sketch
        self._id = pid
        self._x, self._y = x, y  # mm

    @property
    def geometry(self):
        return core.Point3D(self._x / 10.0, self._y / 10.0, 0.0)

    @property
    def parentSketch(self):
        return self._sketch


class SketchCurve(Base):
    def __init__(self, sketch, eid):
        self._sketch = sketch
        self._id = eid

    @property
    def parentSketch(self):
        return self._sketch

    @property
    def isConstruction(self):
        return self._id in self._sketch._construction

    @isConstruction.setter
    def isConstruction(self, value):
        if value:
            self._sketch._construction.add(self._id)
        else:
            self._sketch._construction.discard(self._id)
        self._sketch._dirty = True


class SketchLine(SketchCurve):
    def __init__(self, sketch, eid, a, b):
        super().__init__(sketch, eid)
        self._a, self._b = a, b

    @property
    def startSketchPoint(self):
        return self._a

    @property
    def endSketchPoint(self):
        return self._b


class SketchCircle(SketchCurve):
    def __init__(self, sketch, eid, center, radius):
        super().__init__(sketch, eid)
        self._center = center
        self._radius = radius  # mm

    @property
    def centerSketchPoint(self):
        return self._center

    @property
    def radius(self):
        return self._radius / 10.0


class SketchLineList(Collection):
    pass


class SketchLines(Collection):
    def __init__(self, sketch):
        super().__init__()
        self._sketch = sketch

    def addByTwoPoints(self, startPoint, endPoint):
        s = self._sketch
        a = s._point(startPoint, curve=True)
        b = s._point(endPoint, curve=True)
        if a is b:
            raise NotSupported("SketchLines.addByTwoPoints: the two points coincide")
        line = SketchLine(s, s._new_entity(), a, b)
        s._entities.append(("line", line))
        self._items.append(line)
        s._dirty = True
        return line

    def _poly(self, corners):
        pts = [self._sketch._point(c, curve=True) for c in corners]
        out = SketchLineList()
        for i in range(len(pts)):
            out._items.append(self.addByTwoPoints(pts[i], pts[(i + 1) % len(pts)]))
        return out

    def addTwoPointRectangle(self, pointOne, pointTwo):
        p, q = _sketch_xy(pointOne), _sketch_xy(pointTwo)
        if p[0] == q[0] or p[1] == q[1]:
            raise NotSupported("SketchLines.addTwoPointRectangle: zero width or height")
        return self._poly([(p[0], p[1]), (q[0], p[1]), (q[0], q[1]), (p[0], q[1])])

    def addCenterPointRectangle(self, centerPoint, cornerPoint):
        c, k = _sketch_xy(centerPoint), _sketch_xy(cornerPoint)
        dx, dy = abs(k[0] - c[0]), abs(k[1] - c[1])
        if dx == 0 or dy == 0:
            raise NotSupported("SketchLines.addCenterPointRectangle: zero width or height")
        return self._poly(
            [
                (c[0] - dx, c[1] - dy),
                (c[0] + dx, c[1] - dy),
                (c[0] + dx, c[1] + dy),
                (c[0] - dx, c[1] + dy),
            ]
        )


class SketchCircles(Collection):
    def __init__(self, sketch):
        super().__init__()
        self._sketch = sketch

    def addByCenterRadius(self, centerPoint, radius):
        s = self._sketch
        if not isinstance(radius, (int, float)) or isinstance(radius, bool):
            raise NotSupported("SketchCircles.addByCenterRadius: the radius is a number (cm)")
        r = float(radius) * 10.0
        if not (math.isfinite(r) and r > 0):
            raise NotSupported("SketchCircles.addByCenterRadius: the radius must be positive")
        c = s._point(centerPoint, curve=True)
        circle = SketchCircle(s, s._new_entity(), c, r)
        s._entities.append(("circle", circle))
        self._items.append(circle)
        s._dirty = True
        return circle


class SketchCurves(Base):
    def __init__(self, sketch):
        self._lines = SketchLines(sketch)
        self._circles = SketchCircles(sketch)

    @property
    def sketchLines(self):
        return self._lines

    @property
    def sketchCircles(self):
        return self._circles

    @property
    def count(self):
        return self._lines.count + self._circles.count


class SketchPoints(Collection):
    def __init__(self, sketch):
        super().__init__()
        self._sketch = sketch

    def add(self, point):
        s = self._sketch
        x, y = _sketch_xy(point)
        p = s._make_point(x, y)
        s._standalone.append(p)
        s._dirty = True
        return p


def _sketch_xy(p):
    """Sketch-space Point3D (cm) -> (x, y) mm."""
    if isinstance(p, SketchPoint):
        return p._x, p._y
    if not isinstance(p, core.Point3D):
        raise NotSupported("sketch geometry takes Point3D or SketchPoint objects")
    if abs(p.z) > 1e-12:
        raise NotSupported("sketch points must lie on the sketch plane (z = 0)")
    return p.x * 10.0, p.y * 10.0


class Profile(Base):
    def __init__(self, sketch):
        self._sketch = sketch

    @property
    def parentSketch(self):
        return self._sketch


class Profiles(Collection):
    pass


class Sketch(Base):
    def __init__(self, component, plane, body, name):
        self._component = component
        self._plane_obj = plane
        self._body = body  # host body id or None
        self._name = name
        self._points = []  # SketchPoint, in id order
        self._standalone = []
        self._entities = []  # (kind, curve)
        self._construction = set()
        self._next_entity = 1
        self._feature = None
        self._dirty = True
        self._curves = SketchCurves(self)
        self._sketch_points = SketchPoints(self)
        self._sketch_points._items = self._points
        self._compute_deferred = False

    # -- API ---------------------------------------------------------------
    @property
    def name(self):
        return self._name

    @name.setter
    def name(self, value):
        self._name = str(value)
        if self._feature is not None:
            _rec().add({"cmd": "rename_feature", "feature": self._feature, "name": self._name})

    @property
    def sketchCurves(self):
        return self._curves

    @property
    def sketchPoints(self):
        return self._sketch_points

    @property
    def profiles(self):
        live = [c for _, c in self._entities if c._id not in self._construction]
        return Profiles([Profile(self)] if live else [])

    @property
    def referencePlane(self):
        return self._plane_obj

    @property
    def parentComponent(self):
        return self._component

    @property
    def isComputeDeferred(self):
        return self._compute_deferred

    @isComputeDeferred.setter
    def isComputeDeferred(self, value):
        self._compute_deferred = bool(value)

    # -- building ----------------------------------------------------------
    def _new_entity(self):
        e = self._next_entity
        self._next_entity += 1
        return e

    def _make_point(self, x, y):
        p = SketchPoint(self, len(self._points), x, y)
        self._points.append(p)
        return p

    def _point(self, p, curve):
        if isinstance(p, SketchPoint):
            if p._sketch is not self:
                raise NotSupported("a SketchPoint of another sketch was given")
            return p
        x, y = p if isinstance(p, tuple) else _sketch_xy(p)
        standalone = set(id(q) for q in self._standalone)
        for q in self._points:
            if id(q) in standalone:
                continue
            if abs(q._x - x) <= _MERGE_TOL and abs(q._y - y) <= _MERGE_TOL:
                return q
        return self._make_point(x, y)

    def _json(self):
        pts = [{"id": p._id, "x": p._x, "y": p._y} for p in self._points]
        ents, cons = [], []
        for kind, c in self._entities:
            if kind == "line":
                ents.append({"Line": {"id": c._id, "a": c._a._id, "b": c._b._id}})
            else:
                ents.append({"Circle": {"id": c._id, "center": c._center._id, "radius": c._radius}})
                cons.append(
                    {"Radius": {"entity": c._id, "dim": {"name": "r%d" % c._id, "value": c._radius}}}
                )
        cons = [{"FixedPoint": {"p": p._id, "x": p._x, "y": p._y}} for p in self._points] + cons
        out = {"points": pts, "entities": ents, "constraints": cons}
        if self._construction:
            out["construction"] = sorted(self._construction)
        return out

    def _flush(self):
        """Emit the sketch (or its changes) before a feature uses it."""
        rec = _rec()
        if self._feature is None:
            if not self._points:
                return
            cmd = {"cmd": "create_sketch"}
            if self._name:
                cmd["name"] = self._name
            cmd["plane"] = self._plane_obj._plane()
            cmd["template"] = "custom"
            cmd["sketch"] = self._json()
            self._feature, _ = rec.add(cmd, feature=True)
            expr = getattr(self._plane_obj, "_expr", None)
            if expr is not None:
                rec.add(
                    {
                        "cmd": "set_expression",
                        "feature": self._feature,
                        "name": "plane_offset",
                        "expression": expr,
                    }
                )
        elif self._dirty:
            rec.add({"cmd": "edit_sketch", "feature": self._feature, "sketch": self._json()})
        self._dirty = False

    def _use(self, what):
        self._flush()
        if self._feature is None:
            raise NotSupported("%s: the sketch is empty" % what)
        return self._feature

    def _helper(self, points=(), lines=()):
        """A new sketch on the same plane holding only `points` (standalone)
        and `lines`: R-CAD plastic features take every point/line of a
        sketch, so a selection of some of them gets its own sketch."""
        h = Sketch(self._component, self._plane_obj, self._body, (self._name or "Sketch") + " selection")
        for p in points:
            q = h._make_point(p._x, p._y)
            h._standalone.append(q)
        for ln in lines:
            h._curves.sketchLines.addByTwoPoints((ln._a._x, ln._a._y), (ln._b._x, ln._b._y))
        return h._use("selection")


class Sketches(Collection):
    def __init__(self, component):
        super().__init__()
        self._component = component

    def add(self, planarEntity, occurrenceForCreation=None):
        if occurrenceForCreation is not None:
            raise NotSupported("Sketches.add: occurrences are not supported")
        if not isinstance(planarEntity, _Planar):
            raise NotSupported(
                "Sketches.add takes a construction plane or a planar face of an extrusion"
            )
        _, _, _, body = planarEntity._offset_base()
        s = Sketch(self._component, planarEntity, body, "Sketch%d" % (self.count + 1))
        self._items.append(s)
        _rec().sketches.append(s)
        return s


# ---------------------------------------------------------------------------
# Features


class Feature(Base):
    def __init__(self, fid):
        self._id = fid
        self._name = None
        _rec().api_features.append(self)

    @property
    def name(self):
        return self._name

    @name.setter
    def name(self, value):
        self._name = str(value)
        _rec().add({"cmd": "rename_feature", "feature": self._id, "name": self._name})


class Features(Base):
    def __init__(self, component):
        self._extrude = ExtrudeFeatures(component)
        self._shell = ShellFeatures()
        self._fillet = FilletFeatures()
        self._boss = BossFeatures()
        self._rib = RibFeatures(web=False)
        self._web = RibFeatures(web=True)

    @property
    def extrudeFeatures(self):
        return self._extrude

    @property
    def shellFeatures(self):
        return self._shell

    @property
    def filletFeatures(self):
        return self._fillet

    @property
    def bossFeatures(self):
        return self._boss

    @property
    def ribFeatures(self):
        return self._rib

    @property
    def webFeatures(self):
        return self._web


def _negate(expr):
    """``-(expr)``, or ``name`` for ``- name``."""
    toks = tokenize(expr)
    if len(toks) == 2 and toks[0] == ("op", "-") and toks[1][0] == "name":
        return toks[1][1]
    return "-(%s)" % expr


def _bind(rec, fid, name, expr):
    if expr is not None:
        rec.add({"cmd": "set_expression", "feature": fid, "name": name, "expression": expr})


# -- Extrude -----------------------------------------------------------------


class DistanceExtentDefinition(Base):
    def __init__(self, distance):
        self._distance = distance

    @staticmethod
    def create(distance):
        return DistanceExtentDefinition(distance)

    @property
    def distance(self):
        return self._distance


class ThroughAllExtentDefinition(Base):
    @staticmethod
    def create():
        return ThroughAllExtentDefinition()


class ExtrudeFeatureInput(Base):
    def __init__(self, profile, operation):
        self._profile = profile
        self._operation = operation
        self._extent = None  # ("distance", ValueInput, negate) | ("through_all", reverse)
        self._participants = None

    @property
    def profile(self):
        return self._profile

    @property
    def operation(self):
        return self._operation

    @operation.setter
    def operation(self, value):
        self._operation = value

    @property
    def participantBodies(self):
        return list(self._participants or [])

    @participantBodies.setter
    def participantBodies(self, bodies):
        self._participants = list(bodies)

    def setDistanceExtent(self, isSymmetric, distance):
        if isSymmetric:
            raise NotSupported("ExtrudeFeatureInput.setDistanceExtent: symmetric extents are not mapped")
        self._extent = ("distance", distance, False)
        return True

    def setOneSideExtent(self, extent, direction, taperAngle=None):
        if taperAngle is not None:
            raise NotSupported("ExtrudeFeatureInput.setOneSideExtent: taperAngle is not mapped")
        if direction == ExtentDirections.SymmetricExtentDirection:
            raise NotSupported("ExtrudeFeatureInput.setOneSideExtent: symmetric direction is not mapped")
        neg = direction == ExtentDirections.NegativeExtentDirection
        if isinstance(extent, DistanceExtentDefinition):
            self._extent = ("distance", extent._distance, neg)
        elif isinstance(extent, ThroughAllExtentDefinition):
            self._extent = ("through_all", neg)
        else:
            raise NotSupported(
                "ExtrudeFeatureInput.setOneSideExtent: only DistanceExtentDefinition and "
                "ThroughAllExtentDefinition are supported"
            )
        return True

    def setAllExtent(self, direction):
        if direction == ExtentDirections.SymmetricExtentDirection:
            raise NotSupported("ExtrudeFeatureInput.setAllExtent: symmetric direction is not mapped")
        self._extent = ("through_all", direction == ExtentDirections.NegativeExtentDirection)
        return True


class ExtrudeFeature(Feature):
    def __init__(self, fid, sketch, body, lines):
        super().__init__(fid)
        self._body = body
        self._start = BRepFaces([BRepFace(fid, "ExtrudeStartCap", body)])
        self._end = BRepFaces([BRepFace(fid, "ExtrudeEndCap", body)])
        self._side = BRepFaces([BRepFace(fid, {"ExtrudeSide": {"entity": e}}, body) for e in lines])

    @property
    def startFaces(self):
        return self._start

    @property
    def endFaces(self):
        return self._end

    @property
    def sideFaces(self):
        return self._side

    @property
    def faces(self):
        return BRepFaces(list(self._start) + list(self._end) + list(self._side))

    @property
    def bodies(self):
        return BRepBodies([self._body] if self._body else [])


class ExtrudeFeatures(Collection):
    def __init__(self, component):
        super().__init__()
        self._component = component

    def createInput(self, profile, operation):
        if isinstance(profile, (core.ObjectCollection, list, tuple)):
            items = list(profile)
            if len(items) != 1:
                raise NotSupported("ExtrudeFeatures.createInput: exactly one profile is supported")
            profile = items[0]
        if not isinstance(profile, Profile):
            raise NotSupported("ExtrudeFeatures.createInput takes a Profile (sketch.profiles.item(0))")
        if operation not in _OPS:
            raise NotSupported("FeatureOperations value %r is not mapped (new components)" % (operation,))
        return ExtrudeFeatureInput(profile, operation)

    def addSimple(self, profile, distance, operation):
        inp = self.createInput(profile, operation)
        inp.setDistanceExtent(False, distance)
        return self.add(inp)

    def add(self, input):
        if not isinstance(input, ExtrudeFeatureInput) or input._extent is None:
            raise NotSupported("ExtrudeFeatures.add needs an input with an extent")
        rec = _rec()
        sketch = input._profile._sketch
        sid = sketch._use("extrude")
        op = _OPS[input._operation]
        cmd = {"cmd": "extrude", "sketch": sid}
        expr = None
        if input._extent[0] == "distance":
            raw, e = rec.value(input._extent[1], LENGTH, "the extrude distance")
            d = -raw if input._extent[2] else raw
            if not d:
                raise NotSupported("the extrude distance is zero")
            reverse = d < 0
            on_face = isinstance(sketch._plane_obj, BRepFace)
            if on_face and op == "cut":
                # From a face R-CAD cuts into the material by default (against
                # the face normal): the API's negative distance. A positive
                # distance points away from it and is refused, as in the API.
                reverse = d > 0
            elif on_face and op == "join":
                # A join from a face always grows away from the material.
                reverse = False
            cmd["distance"] = abs(d)
            # R-CAD's distance is |raw|; bind it to the matching expression.
            if e is not None:
                expr = e if raw > 0 else _negate(e)
        else:
            if op != "cut":
                raise NotSupported("through-all extents are supported for cuts only")
            reverse = input._extent[1]
            cmd["through_all"] = True
        if op is not None:
            cmd["op"] = op
            target = None
            if input._participants:
                if len(input._participants) != 1:
                    raise NotSupported("participantBodies: exactly one body is supported")
                target = input._participants[0]._id
            elif sketch._body is not None:
                target = sketch._body
            elif len(rec.bodies) > 1:
                raise NotSupported(
                    "cannot tell which body to %s: set participantBodies (one body)" % op
                )
            if target is not None:
                cmd["target"] = target
        if reverse:
            cmd["reverse"] = True
        new = op is None
        fid, bid = rec.add(cmd, feature=True, body=new)
        _bind(rec, fid, "distance", expr)
        if new:
            body = BRepBody(bid, "Body%d" % bid, self._component)
            rec.bodies.append(body)
            self._component._bodies._items.append(body)
        else:
            tid = cmd.get("target")
            body = next((b for b in rec.bodies if b._id == tid), None)
            if body is None and len(rec.bodies) == 1:
                body = rec.bodies[0]
        lines = [c._id for k, c in sketch._entities if k == "line" and c._id not in sketch._construction]
        f = ExtrudeFeature(fid, sketch, body, lines)
        self._items.append(f)
        return f


# -- Shell -------------------------------------------------------------------


class ShellFeatureInput(Base):
    def __init__(self, entities):
        self._entities = entities
        self._inside = None
        self._outside = None
        self._tangent = True

    @property
    def insideThickness(self):
        return self._inside

    @insideThickness.setter
    def insideThickness(self, value):
        self._inside = value

    @property
    def outsideThickness(self):
        return self._outside

    @outsideThickness.setter
    def outsideThickness(self, value):
        self._outside = value

    @property
    def isTangentChain(self):
        return self._tangent

    @isTangentChain.setter
    def isTangentChain(self, value):
        self._tangent = bool(value)


class ShellFeatures(Collection):
    def createInput(self, inputEntities, isTangentChain=True):
        items = _items(inputEntities, "ShellFeatures.createInput")
        if len(items) != 1 or not isinstance(items[0], BRepFace):
            raise NotSupported(
                "ShellFeatures.createInput: exactly one face to remove is supported "
                "(R-CAD's shell_face); bodies and several faces are not mapped"
            )
        return ShellFeatureInput(items[0])

    def add(self, input):
        rec = _rec()
        if input._inside is None:
            raise NotSupported("ShellFeatureInput.insideThickness is required")
        if input._outside is not None and rec.number(input._outside, LENGTH, "outsideThickness") != 0:
            raise NotSupported("ShellFeatureInput.outsideThickness is not mapped (inside shells only)")
        t, expr = rec.value(input._inside, LENGTH, "the shell thickness")
        fid, _ = rec.add({"cmd": "shell_face", "face": input._entities._ref(), "thickness": t}, feature=True)
        _bind(rec, fid, "thickness", expr)
        f = Feature(fid)
        self._items.append(f)
        return f


# -- Fillet ------------------------------------------------------------------


class FilletEdgeSetInputs(Collection):
    def addConstantRadiusEdgeSet(self, entities, radius, isTangentChain=True):
        items = _items(entities, "addConstantRadiusEdgeSet")
        faces = []
        for o in items:
            if isinstance(o, BRepFace):
                faces.append(o)
            elif isinstance(o, BRepEdges):
                faces.append(o._face)
            else:
                raise NotSupported(
                    "fillet edge sets: only a whole face (every edge of it) maps to R-CAD's "
                    "fillet_loop; %s is not supported" % type(o).__name__
                )
        if len(faces) != 1:
            raise NotSupported("fillet edge sets: exactly one face per fillet is supported")
        es = ConstantRadiusFilletEdgeSetInput(faces[0], radius)
        self._items.append(es)
        return es


class ConstantRadiusFilletEdgeSetInput(Base):
    def __init__(self, face, radius):
        self._face = face
        self._radius = radius


class FilletFeatureInput(Base):
    def __init__(self):
        self._sets = FilletEdgeSetInputs()
        self._rolling = True
        self._g2 = False
        self._tangent = True

    @property
    def edgeSetInputs(self):
        return self._sets

    def addConstantRadiusEdgeSet(self, edges, radius, isTangentChain=True):
        self._sets.addConstantRadiusEdgeSet(edges, radius, isTangentChain)
        return True

    @property
    def isRollingBallCorner(self):
        return self._rolling

    @isRollingBallCorner.setter
    def isRollingBallCorner(self, value):
        self._rolling = bool(value)

    @property
    def isG2(self):
        return self._g2

    @isG2.setter
    def isG2(self, value):
        if value:
            raise NotSupported("FilletFeatureInput.isG2 (curvature-continuous fillets) is not mapped")

    @property
    def isTangentChain(self):
        return self._tangent

    @isTangentChain.setter
    def isTangentChain(self, value):
        self._tangent = bool(value)


class FilletFeatures(Collection):
    def createInput(self):
        return FilletFeatureInput()

    def add(self, input):
        rec = _rec()
        if input._sets.count != 1:
            raise NotSupported("FilletFeatures.add: exactly one edge set is supported")
        es = input._sets.item(0)
        r, expr = rec.value(es._radius, LENGTH, "the fillet radius")
        fid, _ = rec.add(
            {"cmd": "fillet_loop", "face": es._face._ref(), "radius": r, "holes": True}, feature=True
        )
        _bind(rec, fid, "radius", expr)
        f = Feature(fid)
        self._items.append(f)
        return f


# ---------------------------------------------------------------------------
# User parameters

_PARAM_UNITS = {
    "mm": ("mm", LENGTH),
    "cm": ("mm", LENGTH),
    "m": ("mm", LENGTH),
    "in": ("mm", LENGTH),
    "deg": ("deg", ANGLE),
    "rad": ("deg", ANGLE),
    "": ("unitless", UNITLESS),
}


class UserParameter(Base):
    def __init__(self, name, units, comment):
        self._name = name
        self._units = units
        self._comment = comment
        self._expression = None

    def _set(self, vi):
        rec = _rec()
        unit, kind = _PARAM_UNITS[self._units]
        if vi._real is not None:
            v, _ = rec.value(vi, kind, "parameter %s" % self._name)
            expr = fmt(v) + (" mm" if kind == LENGTH else "")
        else:
            text = vi._string
            toks = tokenize(text)
            if len(toks) == 1 and toks[0][0] == "num":
                # A plain number is in the parameter's own units.
                if kind == LENGTH:
                    text = "%s %s" % (text.strip(), self._units)
                elif self._units == "rad":
                    text = fmt(math.degrees(float(toks[0][1])))
            q = plain_quantity(text, kind)
            if q is not None:
                v = q
                expr = fmt(q) + (" mm" if kind == LENGTH else "")
            else:
                v = Evaluator(rec.params).eval(text)
                expr = rcad_expression(text, kind)
        rec.params[self._name] = v
        self._expression = expr
        cmd = {"cmd": "set_user_parameter", "name": self._name, "expression": expr, "unit": unit}
        if self._comment:
            cmd["comment"] = self._comment
        rec.add(cmd)

    @property
    def name(self):
        return self._name

    @property
    def expression(self):
        return self._expression

    @expression.setter
    def expression(self, text):
        self._set(core.ValueInput.createByString(str(text)))

    @property
    def value(self):
        v = _rec().params[self._name]
        kind = _PARAM_UNITS[self._units][1]
        if kind == LENGTH:
            return v / 10.0
        if kind == ANGLE:
            return math.radians(v)
        return v

    @value.setter
    def value(self, v):
        self._set(core.ValueInput.createByReal(v))

    @property
    def unit(self):
        return self._units

    @property
    def comment(self):
        return self._comment


class UserParameters(Collection):
    def add(self, name, value, units, comment=""):
        if not isinstance(name, str) or not name:
            raise NotSupported("UserParameters.add: a parameter name is required")
        if units not in _PARAM_UNITS:
            raise NotSupported(
                "UserParameters.add: unit %r is not supported (mm, cm, m, in, deg, rad or '')"
                % (units,)
            )
        if self.itemByName(name) is not None:
            raise NotSupported("UserParameters.add: parameter %r already exists" % name)
        if not isinstance(value, core.ValueInput):
            raise NotSupported("UserParameters.add: the value must be a ValueInput")
        p = UserParameter(name, units, comment)
        p._set(value)
        self._items.append(p)
        return p


# ---------------------------------------------------------------------------
# Plastic features


def _hole_choice(side):
    return side._v.get("_hole", "pilot")


class BossFeatureSideInput(Base):
    """One side of a boss. Supported members map to the plastics boss
    parameters; members R-CAD does not model raise NotSupported (radius and
    rib-draft members accept 0)."""

    # member -> (plastics parameter, kind)
    _MAP = {
        "diameter": ("diameter", LENGTH),
        "holeDiameter": ("hole_diameter", LENGTH),
        "draftAngle": ("draft_deg", ANGLE),
        "holeDraftAngle": ("hole_draft_deg", ANGLE),
        "holeDepth": ("hole_depth", LENGTH),
        "ribCount": ("rib_count", UNITLESS),
        "ribLength": ("rib_length", LENGTH),
        "ribThickness": ("rib_thickness", LENGTH),
        "ribRotation": ("rib_angle_deg", ANGLE),
        "ribTotalAngle": ("rib_total_deg", ANGLE),
        "ribOffset": (None, LENGTH),
        "alignmentDiameter": ("step_diameter", LENGTH),
        "alignmentDepth": ("step_depth", LENGTH),
        "tipRadius": ("tip_radius", LENGTH),
        "rootRadius": ("root_radius", LENGTH),
    }
    _ENUMS = ("holeExtentType", "ribType", "alignmentType")
    _ZERO_ONLY = {
        "holeStartRadius": LENGTH,
        "holeEndRadius": LENGTH,
        "ribRootRadius": LENGTH,
        "ribTipRadius": LENGTH,
        "ribDraftAngle": ANGLE,
        "ribOuterDraftAngle": ANGLE,
    }
    _UNMAPPED = (
        "alignmentDraftAngle",
        "alignmentRootRadius",
        "alignmentTipRadius",
        "holeMajorDiameter",
        "holeMajorDepth",
        "holeMajorDraftAngle",
        "holeMajorRootRadius",
        "holeMajorTipRadius",
        "holeCountersinkAngle",
        "offsetClearance",
        "ribChamferAngle",
        "ribCutSize",
    )

    def __init__(self):
        object.__setattr__(self, "_v", {})

    def __setattr__(self, name, value):
        if name.startswith("_"):
            object.__setattr__(self, name, value)
        elif name in self._MAP or name in self._ENUMS:
            self._v[name] = value
        elif name in self._ZERO_ONLY:
            if _rec().number(value, self._ZERO_ONLY[name], name) != 0:
                raise NotSupported(
                    "adsk.fusion.BossFeatureSideInput.%s is not modelled by R-CAD's boss "
                    "(only 0 is accepted)" % name
                )
        elif name in self._UNMAPPED:
            raise NotSupported(
                "adsk.fusion.BossFeatureSideInput.%s is not in R-CAD's Fusion API subset "
                "(drafted alignment steps, counterbores/countersinks on side 1 and rib cut "
                "profiles are not modelled)" % name
            )
        else:
            Base.__setattr__(self, name, value)

    def __getattr__(self, name):
        if name in self._MAP or name in self._ENUMS:
            return self._v.get(name)
        return Base.__getattr__(self, name)

    def setSimple(self, diameter, holeDiameter):
        self._v["diameter"] = diameter
        self._v["holeDiameter"] = holeDiameter
        self._v["_hole"] = "pilot"
        return True

    def setBlank(self, diameter):
        self._v["diameter"] = diameter
        self._v["_hole"] = "none"
        return True

    def _unmapped(name):
        def f(self, *args, **kwargs):
            raise NotSupported(
                "adsk.fusion.BossFeatureSideInput.%s is not in R-CAD's Fusion API subset" % name
            )

        return f

    setCounterbore = _unmapped("setCounterbore")
    setCountersink = _unmapped("setCountersink")
    setRibExtent = _unmapped("setRibExtent")
    clearRibExtent = _unmapped("clearRibExtent")
    del _unmapped


class BossFeatureInput(Base):
    def __init__(self):
        self._points = None
        self._side1 = None
        self._side2 = None
        self._offset = None
        self._default_dir = True
        self._participants = None

    def createSideInput(self):
        return BossFeatureSideInput()

    def setPositionBySketchPoints(self, sketchPoints):
        pts = _items(sketchPoints, "setPositionBySketchPoints")
        if not pts or not all(isinstance(p, SketchPoint) for p in pts):
            raise NotSupported("setPositionBySketchPoints takes SketchPoint objects")
        if len(set(id(p._sketch) for p in pts)) != 1:
            raise NotSupported("setPositionBySketchPoints: all points must be in one sketch")
        self._points = pts
        return True

    @property
    def side1(self):
        return self._side1

    @side1.setter
    def side1(self, value):
        if not isinstance(value, BossFeatureSideInput):
            raise NotSupported("BossFeatureInput.side1 takes a BossFeatureSideInput")
        self._side1 = value

    @property
    def side2(self):
        return self._side2

    @side2.setter
    def side2(self, value):
        if value is not None:
            raise NotSupported(
                "adsk.fusion.BossFeatureInput.side2 is not in R-CAD's Fusion API subset "
                "(the mating boss needs the mate face; use R-CAD's plastics boss 'mate' input)"
            )

    @property
    def offset(self):
        return self._offset

    @offset.setter
    def offset(self, value):
        self._offset = value

    @property
    def isDefaultDirection(self):
        return self._default_dir

    @isDefaultDirection.setter
    def isDefaultDirection(self, value):
        if not value:
            raise NotSupported("BossFeatureInput.isDefaultDirection = False is not mapped")

    @property
    def participantBodies(self):
        return list(self._participants or [])

    @participantBodies.setter
    def participantBodies(self, bodies):
        self._participants = list(bodies)

    @property
    def creationOccurrence(self):
        return None

    @creationOccurrence.setter
    def creationOccurrence(self, value):
        if value is not None:
            raise NotSupported("BossFeatureInput.creationOccurrence: occurrences are not supported")


class PlasticFeature(Feature):
    pass


def _plugin_feature(rec, feature, name, inputs, params, exprs, choices, resolve=None):
    rec.flush_plastics()
    cmd = {"cmd": "plugin_feature", "plugin": "plastics", "feature": feature}
    if name:
        cmd["name"] = name
    cmd["inputs"] = inputs
    cmd["params"] = params
    if choices:
        cmd["choices"] = choices
    if exprs:
        cmd["expressions"] = exprs
    fid, _ = rec.add(cmd, feature=True, resolve=resolve)
    return fid


class BossFeatures(Collection):
    def createInput(self):
        return BossFeatureInput()

    def add(self, input):
        rec = _rec()
        if not isinstance(input, BossFeatureInput) or input._points is None:
            raise NotSupported("BossFeatures.add needs an input positioned with setPositionBySketchPoints")
        side = input._side1 or BossFeatureSideInput()
        sketch = input._points[0]._sketch
        body = None
        if input._participants:
            if len(input._participants) != 1:
                raise NotSupported("BossFeatureInput.participantBodies: exactly one body is supported")
            body = input._participants[0]._id
        elif sketch._body is not None:
            body = sketch._body
        if body is None:
            raise NotSupported(
                "the boss sketch must lie on a face of a body or on a plane offset from one"
            )
        # rootRadius is accepted only as 0: say so, since R-CAD's boss otherwise takes
        # its root fillet from the plastic rule's nominal radius (NR).
        params, exprs = {"root_radius": 0.0}, {}
        for member, (slot, kind) in BossFeatureSideInput._MAP.items():
            vi = side._v.get(member)
            if vi is None or slot is None:
                continue
            v, e = rec.value(vi, kind, member)
            if slot == "rib_count":
                if v != int(v) or v < 0:
                    raise NotSupported("ribCount must be a whole number")
                v = int(v)
            params[slot] = v
            if e is not None:
                exprs[slot] = e
        choices = {}
        hole = _hole_choice(side)
        ext = side._v.get("holeExtentType")
        full = False
        if hole != "none":
            if ext == BossHoleExtentTypes.BossHoleThrough:
                hole = "through"
            elif ext == BossHoleExtentTypes.BossBlindFull or (ext is None and "hole_depth" not in params):
                full = True
            elif ext not in (None, BossHoleExtentTypes.BossBlindDepth):
                raise NotSupported("BossFeatureSideInput.holeExtentType %r is not mapped" % (ext,))
            choices["hole"] = hole
        else:
            choices["hole"] = "none"
        align = side._v.get("alignmentType")
        if align in (None, BossAlignmentTypes.BossAlignFlat):
            pass
        elif align == BossAlignmentTypes.BossAlignStepOut:
            choices["step"] = "out"
        elif align == BossAlignmentTypes.BossAlignStepIn:
            choices["step"] = "in"
        else:
            raise NotSupported("BossFeatureSideInput.alignmentType %r is not mapped" % (align,))
        rib = side._v.get("ribType")
        if rib == BossRibShapeTypes.BossRibShapeNone:
            params["rib_count"] = 0
        elif rib not in (None, BossRibShapeTypes.BossRibShapeChamfer):
            raise NotSupported(
                "BossFeatureSideInput.ribType: only BossRibShapeChamfer and BossRibShapeNone are mapped"
            )
        offset = 0.0 if input._offset is None else rec.number(input._offset, LENGTH, "the boss offset")
        rib_offset = None
        if side._v.get("ribOffset") is not None:
            rib_offset = rec.number(side._v["ribOffset"], LENGTH, "ribOffset")
        pts = input._points
        sel = set(id(p) for p in pts)
        if sel == set(id(p) for p in sketch._standalone):
            sid = sketch._use("boss")
        else:
            sid = sketch._helper(points=pts)
        resolve = {
            "boss": {
                "sketch": sid,
                "body": body,
                "points": [[p._x, p._y] for p in pts],
                "offset": offset,
                "hole_full": full,
                "rib_offset": rib_offset,
            }
        }
        fid = _plugin_feature(
            rec, "boss", None, {"sketch": {"kind": "sketch", "sketch": sid}}, params, exprs, choices, resolve
        )
        f = PlasticFeature(fid)
        self._items.append(f)
        return f


class RibFeatureInput(Base):
    def __init__(self, web, curves, thickness, body):
        self._web = web
        self._curves = curves
        self._thickness = thickness
        self._body = body
        self._depth = None
        self._to_next = False
        self._draft = None
        self._flipped = False
        self._symmetric = True

    def _member(self):
        return "WebFeatureInput" if self._web else "RibFeatureInput"

    @property
    def curves(self):
        return list(self._curves)

    @property
    def thickness(self):
        return self._thickness

    @thickness.setter
    def thickness(self, value):
        self._thickness = value

    @property
    def depth(self):
        return self._depth

    @property
    def draftAngle(self):
        return self._draft

    @draftAngle.setter
    def draftAngle(self, value):
        self._draft = value

    @property
    def isFlipped(self):
        return self._flipped

    @isFlipped.setter
    def isFlipped(self, value):
        self._flipped = bool(value)

    @property
    def isSymmetric(self):
        return self._symmetric

    @isSymmetric.setter
    def isSymmetric(self, value):
        self._symmetric = bool(value)

    @property
    def isThicknessFromTop(self):
        return False

    @isThicknessFromTop.setter
    def isThicknessFromTop(self, value):
        if value:
            raise NotSupported("adsk.fusion.%s.isThicknessFromTop is not in R-CAD's Fusion API subset" % self._member())

    @property
    def isExtended(self):
        return False

    @isExtended.setter
    def isExtended(self, value):
        if not self._web:
            raise not_in_subset("adsk.fusion.RibFeatureInput.isExtended")
        if value:
            raise NotSupported("adsk.fusion.WebFeatureInput.isExtended (extend curves) is not modelled")

    @property
    def filletRadius(self):
        return None

    @filletRadius.setter
    def filletRadius(self, value):
        if _rec().number(value, LENGTH, "filletRadius") != 0:
            raise NotSupported("adsk.fusion.%s.filletRadius is not modelled (only 0)" % self._member())

    @property
    def targetBody(self):
        return self._body

    @targetBody.setter
    def targetBody(self, body):
        self._body = body

    @property
    def creationOccurrence(self):
        return None

    @creationOccurrence.setter
    def creationOccurrence(self, value):
        if value is not None:
            raise NotSupported("%s.creationOccurrence: occurrences are not supported" % self._member())

    def setDistanceExtent(self, depth):
        self._depth = depth
        self._to_next = False
        return True

    def setToNextExtent(self):
        self._to_next = True
        self._depth = None
        return True


class RibFeatures(Collection):
    def __init__(self, web):
        super().__init__()
        self._web = web

    def createInput(self, curves, thickness, targetBody=None):
        kind = "web" if self._web else "rib"
        if isinstance(curves, Sketch):
            items = [c for k, c in curves._entities if c._id not in curves._construction]
        else:
            items = _items(curves, "%sFeatures.createInput" % kind)
        if not items or not all(isinstance(c, SketchLine) for c in items):
            raise NotSupported("%s curves: only sketch lines are supported by this subset" % kind)
        if len(set(id(c._sketch) for c in items)) != 1:
            raise NotSupported("%s curves: all curves must be in one sketch" % kind)
        if targetBody is not None and not isinstance(targetBody, BRepBody):
            raise NotSupported("%s targetBody must be a BRepBody" % kind)
        return RibFeatureInput(self._web, items, thickness, targetBody)

    def add(self, input):
        rec = _rec()
        kind = "web" if self._web else "rib"
        if not isinstance(input, RibFeatureInput) or input._web != self._web:
            raise NotSupported("%sFeatures.add needs an input from its own createInput" % kind)
        sketch = input._curves[0]._sketch
        if not input._to_next and input._depth is None:
            raise NotSupported("%s: set an extent (setDistanceExtent, setToNextExtent)" % kind)
        params, exprs, choices = {}, {}, {}
        for slot, vi, k in (
            ("thickness", input._thickness, LENGTH),
            ("depth", input._depth, LENGTH),
            ("draft_deg", input._draft, ANGLE),
        ):
            if vi is None:
                continue
            v, e = rec.value(vi, k, slot)
            params[slot] = v
            if e is not None:
                exprs[slot] = e
        choices["thickness_side"] = "symmetric" if input._symmetric else "one_side"
        if self._web:
            choices["direction"] = "flip" if input._flipped else "normal"
            choices["extent"] = "to_next" if input._to_next else "distance"
        else:
            # Default: tangent x normal (R-CAD "right"); flipped: "left".
            choices["side"] = "left" if input._flipped else "right"
            choices["extent"] = "to_next" if input._to_next else "distance"
        all_lines = [c for k, c in sketch._entities if c._id not in sketch._construction]
        if set(id(c) for c in input._curves) == set(id(c) for c in all_lines):
            sid = sketch._use(kind)
        else:
            sid = sketch._helper(lines=input._curves)
        inputs = {"sketch": {"kind": "sketch", "sketch": sid}}
        if input._body is not None:
            inputs["body"] = {"kind": "body", "body": input._body._id}
        fid = _plugin_feature(rec, kind, None, inputs, params, exprs, choices)
        f = PlasticFeature(fid)
        self._items.append(f)
        return f


# ---------------------------------------------------------------------------
# Plastic rules

# Rule symbol defaults (the plastics plugin's rule fields).
_RULE_DEFAULTS = {
    "T": "2 mm",
    "DA": "1",
    "NR": "0.5*T",
    "CL": "0.1 mm",
    "RH": "T",
    "TMIN": "0.5*T",
    "TMAX": "1.5*T",
    "TV": "0.15",
    "MDA": "0.5",
    "KE": "0.5*T",
}
_PRESETS = [
    ("ABS", "abs", "2 mm", "1"),
    ("ASA", "asa", "2 mm", "1"),
    ("PC", "pc", "2 mm", "1"),
    ("PC_ABS", "pc_abs", "2 mm", "1"),
    ("PP", "pp", "1.5 mm", "1"),
    ("PA66", "pa66", "1.5 mm", "0.5"),
    ("POM", "pom", "1.5 mm", "0.5"),
    ("HDPE", "hdpe", "2 mm", "1.5"),
]
# PlasticRule property -> (rule symbol, kind)
_RULE_PROPS = {
    "thickness": ("T", LENGTH),
    "draftAngle": ("DA", ANGLE),
    "nominalRadius": ("NR", LENGTH),
    "clearance": ("CL", LENGTH),
    "revealHeight": ("RH", LENGTH),
    "minimumThickness": ("TMIN", LENGTH),
    "maximumThickness": ("TMAX", LENGTH),
    "minimumDraftAngle": ("MDA", ANGLE),
    "knifeEdgeThreshold": ("KE", LENGTH),
}


def _valid_rule_name(s):
    return (
        isinstance(s, str)
        and 0 < len(s) <= 32
        and s[0].isascii()
        and s[0].isalpha()
        and all(c.isascii() and (c.isalnum() or c == "_") for c in s)
    )


class PlasticRuleValue(Base):
    def __init__(self, rule, symbol, kind):
        self._rule = rule
        self._symbol = symbol
        self._kind = kind

    @property
    def expression(self):
        return self._rule._expr(self._symbol)

    @expression.setter
    def expression(self, text):
        self._rule._writable()
        text = str(text)
        q = plain_quantity(text, self._kind)
        if q is not None:
            e = fmt(q) + (" mm" if self._kind == LENGTH else "")
        else:
            out = []
            for k, v in tokenize(text):
                out.append("T" if (k == "name" and v == "Thickness") else v)
            e = rcad_expression(" ".join(out), self._kind)
            Evaluator(self._rule._symbols_except(self._symbol)).eval(e)
        self._rule._values[self._symbol] = e

    @property
    def value(self):
        v = self._rule._eval(self._symbol)
        return v / 10.0 if self._kind == LENGTH else math.radians(v)

    @value.setter
    def value(self, v):
        self._rule._writable()
        if not isinstance(v, (int, float)) or isinstance(v, bool) or not math.isfinite(v):
            raise NotSupported("PlasticRuleValue.value must be a finite number")
        if self._kind == LENGTH:
            self._rule._values[self._symbol] = fmt(v * 10.0) + " mm"
        else:
            self._rule._values[self._symbol] = fmt(math.degrees(v))


class PlasticRule(Base):
    def __init__(self, name, material, values, library):
        self._name = name
        self._material = material
        self._values = dict(values)
        self._library = library

    def _writable(self):
        if self._library:
            raise NotSupported(
                "library plastic rules are read-only; copy one with designPlasticRules.addByCopy"
            )

    def _expr(self, symbol):
        return self._values.get(symbol, _RULE_DEFAULTS[symbol])

    def _eval(self, symbol, seen=()):
        if symbol in seen:
            raise NotSupported("plastic rule values refer to each other in a cycle")
        deps = {}
        for kind, tok in tokenize(self._expr(symbol)):
            if kind == "name" and tok in _RULE_DEFAULTS:
                deps[tok] = self._eval(tok, seen + (symbol,))
        return Evaluator(deps).eval(self._expr(symbol))

    def _symbols_except(self, symbol):
        return {s: 0.0 for s in _RULE_DEFAULTS if s != symbol}

    @property
    def name(self):
        return self._name

    @name.setter
    def name(self, value):
        self._writable()
        if not _valid_rule_name(value):
            raise NotSupported("R-CAD plastic rule names are identifiers (letters, digits, _)")
        self._name = value

    @property
    def material(self):
        return self._material

    @material.setter
    def material(self, value):
        self._writable()
        if value is not None and not isinstance(value, str):
            raise NotSupported("PlasticRule.material takes an R-CAD material catalog key (e.g. 'abs')")
        self._material = value

    @property
    def isDefault(self):
        return False

    @property
    def isUsed(self):
        return False

    @property
    def units(self):
        return "mm"

    def __getattr__(self, name):
        if name in _RULE_PROPS:
            sym, kind = _RULE_PROPS[name]
            return PlasticRuleValue(self, sym, kind)
        return Base.__getattr__(self, name)

    def _json(self):
        out = {"name": self._name}
        if self._material:
            out["material"] = self._material
        out["values"] = dict(sorted(self._values.items()))
        return out


class PlasticRules(Collection):
    def __init__(self, library, items=()):
        super().__init__(items)
        self._library = library

    def itemByName(self, name):
        for r in self._items:
            if r._name.lower() == str(name).lower():
                return r
        return None

    def addByCopy(self, existingPlasticRule, name):
        if self._library:
            raise NotSupported("the plastic rule library is read-only in R-CAD")
        if not isinstance(existingPlasticRule, PlasticRule):
            raise NotSupported("PlasticRules.addByCopy takes a PlasticRule")
        if not _valid_rule_name(name):
            raise NotSupported("R-CAD plastic rule names are identifiers (letters, digits, _): %r" % (name,))
        if self.itemByName(name) is not None:
            raise NotSupported("a plastic rule named %r already exists" % name)
        r = PlasticRule(name, existingPlasticRule._material, existingPlasticRule._values, False)
        self._items.append(r)
        return r


class _Plastics:
    def __init__(self, root):
        self.root = root
        self.design = PlasticRules(False)
        self.library = PlasticRules(
            True,
            [PlasticRule(n, m, {"T": t, "DA": da}, True) for n, m, t, da in _PRESETS],
        )
        self.emitted = None

    def data(self):
        rules = [r._json() for r in self.design]
        bodies = {}
        rule = self.root._rule
        if rule is not None:
            for b in self.root._bodies:
                bodies[str(b._id)] = rule._name
        return {"rules": rules, "bodies": bodies}

    def _flush(self):
        if not self.design.count:
            return
        d = self.data()
        if d != self.emitted:
            _rec().add({"cmd": "set_plugin_data", "plugin": "plastics", "data": d})
            self.emitted = d
