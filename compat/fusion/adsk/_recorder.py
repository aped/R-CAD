"""Recorder: turns API calls into R-CAD commands (docs/COMMANDS.md).

Units. The API works in internal units: centimetres for lengths and radians
for angles (``ValueInput.createByReal``). R-CAD commands use millimetres and
degrees. Strings (``ValueInput.createByString``) carry their own units; a
plain number in a string is in the design's default units (mm, degrees).
Strings that reference parameters are passed on as R-CAD expressions and
bound to the feature parameter.

Ids. R-CAD numbers features 1, 2, ... in creation order after
``new_document``, and bodies 1, 2, ... in creation order. The recorder
predicts both as it appends commands (every ``create_sketch``, ``extrude``,
``shell_face``, ``fillet_loop`` and ``plugin_feature`` adds one feature;
parameters, renames, sketch edits and plugin data add none). Each step
carries the expected id so the CLI can check the prediction.
"""

import math
import re
import sys


class NotSupported(RuntimeError):
    """A member outside R-CAD's Fusion API subset, or a case it cannot map."""

    def __init__(self, message):
        super().__init__(message)
        recorder().not_supported.append(message)


def not_in_subset(qualname):
    return NotSupported("%s is not in R-CAD's Fusion API subset" % qualname)


class Base:
    """Strict base: unknown members raise NotSupported naming the member."""

    _module = "adsk.core"

    @classmethod
    def cast(cls, obj):
        return obj if isinstance(obj, cls) else None

    @property
    def objectType(self):
        return "%s::%s" % (self._module.replace(".", "::"), type(self).__name__)

    @property
    def isValid(self):
        return True

    @classmethod
    def classType(cls):
        return "%s::%s" % (cls._module.replace(".", "::"), cls.__name__)

    def __getattr__(self, name):
        # Only called for members that do not exist.
        if name.startswith("__") and name.endswith("__"):
            raise AttributeError(name)
        if name.startswith("_"):
            raise AttributeError(name)
        raise not_in_subset("%s.%s.%s" % (self._module, type(self).__name__, name))

    def __setattr__(self, name, value):
        if name.startswith("_"):
            object.__setattr__(self, name, value)
            return
        attr = getattr(type(self), name, None)
        if isinstance(attr, property):
            if attr.fset is None:
                raise NotSupported(
                    "%s.%s.%s is read-only" % (self._module, type(self).__name__, name)
                )
            attr.fset(self, value)
            return
        raise not_in_subset("%s.%s.%s" % (self._module, type(self).__name__, name))


class Collection(Base):
    """Read-only list-like collection (``count``, ``item``, iteration)."""

    def __init__(self, items=None):
        self._items = list(items or [])

    @property
    def count(self):
        return len(self._items)

    def item(self, index):
        if not isinstance(index, int) or index < 0 or index >= len(self._items):
            return None
        return self._items[index]

    def __iter__(self):
        return iter(list(self._items))

    def __len__(self):
        return len(self._items)

    def __getitem__(self, index):
        return self._items[index]


# ---------------------------------------------------------------------------
# Quantities and expressions

LENGTH, ANGLE, UNITLESS = "length", "angle", "unitless"

# Length units R-CAD expressions understand (docs/PARAMETERS.md), in mm.
RCAD_LENGTH_UNITS = {"mm": 1.0, "cm": 10.0, "m": 1000.0, "in": 25.4}
# Further length units accepted in plain quantities (converted to a number).
EXTRA_LENGTH_UNITS = {"ft": 304.8, '"': 25.4, "inch": 25.4, "um": 0.001}
ANGLE_UNITS = {"deg": 1.0, "rad": 180.0 / math.pi, "°": 1.0}

_TOKEN = re.compile(
    r"\s*(?:(?P<num>(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)"
    r"|(?P<name>[A-Za-z_][A-Za-z0-9_]*)"
    r'|(?P<unit>["°])'
    r"|(?P<op>[-+*/(),]))"
)


def tokenize(text):
    out = []
    pos = 0
    text = text.strip()
    while pos < len(text):
        m = _TOKEN.match(text, pos)
        if not m or m.end() == pos:
            raise NotSupported("cannot read the expression %r" % text)
        pos = m.end()
        for kind in ("num", "name", "unit", "op"):
            if m.group(kind) is not None:
                out.append((kind, m.group(kind)))
                break
    return out


def _unit_factor(word, kind):
    if word in RCAD_LENGTH_UNITS:
        return LENGTH, RCAD_LENGTH_UNITS[word]
    if word in EXTRA_LENGTH_UNITS:
        return LENGTH, EXTRA_LENGTH_UNITS[word]
    if word in ANGLE_UNITS:
        return ANGLE, ANGLE_UNITS[word]
    return None


def plain_quantity(text, kind):
    """``"5 mm"`` / ``"2"`` / ``"1.5 deg"`` -> number in R-CAD units, else None."""
    toks = tokenize(text)
    sign = 1.0
    if toks and toks[0] == ("op", "-"):
        sign, toks = -1.0, toks[1:]
    if not toks or toks[0][0] != "num" or len(toks) > 2:
        return None
    value = float(toks[0][1])
    if len(toks) == 1:
        return sign * value
    u = _unit_factor(toks[1][1], kind)
    if u is None:
        return None
    ukind, factor = u
    if kind != UNITLESS and ukind != kind:
        raise NotSupported("%r is not a %s" % (text, kind))
    return sign * value * factor


class Evaluator:
    """Evaluates expressions over known user parameters (to predict values
    that R-CAD will derive from the same expression)."""

    def __init__(self, params):
        self.params = params

    def eval(self, text):
        self.toks = tokenize(text)
        self.i = 0
        v = self.expr()
        if self.i != len(self.toks):
            raise NotSupported("cannot read the expression %r" % text)
        return v

    def peek(self):
        return self.toks[self.i] if self.i < len(self.toks) else (None, None)

    def take(self):
        t = self.peek()
        self.i += 1
        return t

    def expr(self):
        v = self.term()
        while self.peek() in (("op", "+"), ("op", "-")):
            op = self.take()[1]
            r = self.term()
            v = v + r if op == "+" else v - r
        return v

    def term(self):
        v = self.unary()
        while self.peek() in (("op", "*"), ("op", "/")):
            op = self.take()[1]
            r = self.unary()
            if op == "/" and r == 0:
                raise NotSupported("division by zero in an expression")
            v = v * r if op == "*" else v / r
        return v

    def unary(self):
        if self.peek() == ("op", "-"):
            self.take()
            return -self.unary()
        if self.peek() == ("op", "+"):
            self.take()
            return self.unary()
        return self.atom()

    def atom(self):
        kind, val = self.take()
        if kind == "num":
            v = float(val)
            nk, nv = self.peek()
            if nk in ("name", "unit") and _unit_factor(nv, UNITLESS):
                self.take()
                v *= _unit_factor(nv, UNITLESS)[1]
            return v
        if kind == "name":
            if self.peek() == ("op", "("):
                self.take()
                args = [self.expr()]
                while self.peek() == ("op", ","):
                    self.take()
                    args.append(self.expr())
                if self.take() != ("op", ")"):
                    raise NotSupported("missing ) in an expression")
                fn = {"min": min, "max": max, "abs": lambda x: abs(x)}.get(val)
                if fn is None:
                    raise NotSupported("function %s() is not supported in expressions" % val)
                return fn(*args)
            if val not in self.params:
                raise NotSupported("unknown parameter %r in an expression" % val)
            return self.params[val]
        if (kind, val) == ("op", "("):
            v = self.expr()
            if self.take() != ("op", ")"):
                raise NotSupported("missing ) in an expression")
            return v
        raise NotSupported("cannot read the expression")


def rcad_expression(text, kind):
    """Translate an expression to R-CAD's syntax: angle units are dropped
    (R-CAD angles are plain degrees), other units must be R-CAD length
    units."""
    out = []
    for k, v in tokenize(text):
        if k in ("name", "unit") and v in ANGLE_UNITS:
            if v == "rad":
                raise NotSupported("'rad' inside an expression is not supported; use deg")
            continue
        if k in ("name", "unit") and v in EXTRA_LENGTH_UNITS:
            raise NotSupported("unit %r inside an expression is not supported" % v)
        out.append(v)
    return " ".join(out)


def fmt(x):
    """A float as a short exact string."""
    r = repr(float(x))
    return r[:-2] if r.endswith(".0") else r


# ---------------------------------------------------------------------------
# The recorder


class Recorder:
    def __init__(self):
        self.steps = []
        self.next_feature = 1
        self.next_body = 1
        self.params = {}  # user parameter -> value in R-CAD units
        self.not_supported = []
        self.script_error = None
        self.sketches = []  # sketch data objects, for flushing
        self.bodies = []  # BRepBody objects, in creation order
        self.plastics = None  # set by adsk.fusion (design rules state)
        self.steps.append({"command": {"cmd": "new_document"}})

    # -- values ------------------------------------------------------------
    def value(self, vi, kind, what):
        """(number in R-CAD units, R-CAD expression or None) of a ValueInput
        (or a plain Python number, taken in internal units)."""
        from adsk.core import ValueInput

        if isinstance(vi, (int, float)) and not isinstance(vi, bool):
            vi = ValueInput.createByReal(float(vi))
        if not isinstance(vi, ValueInput):
            raise NotSupported("%s must be a ValueInput" % what)
        if vi._real is not None:
            x = vi._real
            if not math.isfinite(x):
                raise NotSupported("%s is not a finite number" % what)
            if kind == LENGTH:
                return x * 10.0, None
            if kind == ANGLE:
                return math.degrees(x), None
            return x, None
        text = vi._string
        q = plain_quantity(text, kind)
        if q is not None:
            return q, None
        v = Evaluator(self.params).eval(text)
        return v, rcad_expression(text, kind)

    def number(self, vi, kind, what):
        """A value that must be a plain number (no parameter binding)."""
        v, _ = self.value(vi, kind, what)
        return v

    # -- commands ----------------------------------------------------------
    def add(self, command, feature=False, body=False, resolve=None):
        """Append a command; returns (feature id or None, body id or None)."""
        step = {"command": command}
        fid = bid = None
        if feature:
            fid = self.next_feature
            self.next_feature += 1
            step["feature"] = fid
        if body:
            bid = self.next_body
            self.next_body += 1
            step["body"] = bid
        if resolve:
            step["resolve"] = resolve
        self.steps.append(step)
        return fid, bid

    def flush_sketches(self):
        for s in self.sketches:
            s._flush()

    def flush_plastics(self):
        if self.plastics is not None:
            self.plastics._flush()

    def finish(self):
        self.flush_sketches()
        self.flush_plastics()
        return {"steps": self.steps}


_RECORDER = [None]


def recorder():
    if _RECORDER[0] is None:
        _RECORDER[0] = Recorder()
    return _RECORDER[0]


def reset():
    """Start a new recording (and a new application state)."""
    _RECORDER[0] = Recorder()
    mods = [sys.modules.get("adsk.core"), sys.modules.get("adsk.fusion")]
    for m in mods:
        if m is not None and hasattr(m, "_reset"):
            m._reset()
    return _RECORDER[0]
