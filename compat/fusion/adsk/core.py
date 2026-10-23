"""``adsk.core`` subset: Application, UserInterface, Point3D, ValueInput,
ObjectCollection (docs/FUSION_SCRIPTS.md)."""

import math
import sys

from adsk._recorder import Base, NotSupported, not_in_subset, recorder


def __getattr__(name):
    if name.startswith("__"):
        raise AttributeError(name)
    raise not_in_subset("adsk.core.%s" % name)


class DialogResults:
    DialogError = -1
    DialogOK = 0
    DialogCancel = 1
    DialogYes = 2
    DialogNo = 3


class MessageBoxButtonTypes:
    OKButtonType = 0
    OKCancelButtonType = 1
    RetryCancelButtonType = 2
    YesNoButtonType = 3
    YesNoCancelButtonType = 4


class MessageBoxIconTypes:
    NoIconIconType = 0
    QuestionIconType = 1
    InformationIconType = 2
    WarningIconType = 3
    CriticalIconType = 4


class ValueTypes:
    RealValueType = 0
    StringValueType = 1


class UserInterface(Base):
    def messageBox(self, text, title="", buttons=0, icon=0):
        """Prints the message. A message shown while an exception is being
        handled (the usual ``except: ui.messageBox(traceback...)``) is taken
        as the script reporting a failure."""
        text = str(text)
        print("[%s] %s" % (title or "message", text))
        sys.stdout.flush()
        if sys.exc_info()[1] is not None and recorder().script_error is None:
            recorder().script_error = text
        return DialogResults.DialogOK


class Application(Base):
    _instance = None

    @staticmethod
    def get():
        if Application._instance is None:
            Application._instance = Application()
            Application._instance._ui = UserInterface()
            from adsk import fusion

            Application._instance._design = fusion.Design()
        return Application._instance

    @property
    def activeProduct(self):
        return self._design

    @property
    def userInterface(self):
        return self._ui

    @property
    def documents(self):
        return Documents()

    @property
    def activeDocument(self):
        return Document(self._design)


class DocumentTypes:
    FusionDesignDocumentType = 0


class Document(Base):
    def __init__(self, design):
        self._design = design

    @property
    def design(self):
        return self._design


class Documents(Base):
    def add(self, documentType, visible=True, options=None):
        """One document per script: allowed before anything was modelled."""
        if documentType != DocumentTypes.FusionDesignDocumentType:
            raise NotSupported("Documents.add: only FusionDesignDocumentType is supported")
        if len(recorder().steps) > 1:
            raise NotSupported("Documents.add: a script works in one document")
        return Document(Application.get()._design)


def _reset():
    Application._instance = None


class Point3D(Base):
    """A point in internal units (cm). Sketch methods take sketch-space
    points (z must be 0)."""

    def __init__(self, x=0.0, y=0.0, z=0.0):
        for v in (x, y, z):
            if not isinstance(v, (int, float)) or isinstance(v, bool) or not math.isfinite(v):
                raise NotSupported("Point3D coordinates must be finite numbers")
        self._x, self._y, self._z = float(x), float(y), float(z)

    @staticmethod
    def create(x=0.0, y=0.0, z=0.0):
        return Point3D(x, y, z)

    @property
    def x(self):
        return self._x

    @x.setter
    def x(self, v):
        self._x = float(v)

    @property
    def y(self):
        return self._y

    @y.setter
    def y(self, v):
        self._y = float(v)

    @property
    def z(self):
        return self._z

    @z.setter
    def z(self, v):
        self._z = float(v)

    def copy(self):
        return Point3D(self._x, self._y, self._z)

    def asArray(self):
        return [self._x, self._y, self._z]

    def distanceTo(self, other):
        return math.dist(self.asArray(), other.asArray())

    def isEqualTo(self, other):
        return self.asArray() == other.asArray()


class ValueInput(Base):
    """A value: a real in internal units (cm, radians) or a string
    expression with units."""

    def __init__(self, real=None, string=None):
        self._real = real
        self._string = string

    @staticmethod
    def createByReal(value):
        if not isinstance(value, (int, float)) or isinstance(value, bool):
            raise NotSupported("ValueInput.createByReal needs a number")
        return ValueInput(real=float(value))

    @staticmethod
    def createByString(value):
        if not isinstance(value, str) or not value.strip():
            raise NotSupported("ValueInput.createByString needs a non-empty string")
        return ValueInput(string=value)

    @property
    def valueType(self):
        return ValueTypes.RealValueType if self._real is not None else ValueTypes.StringValueType

    @property
    def realValue(self):
        return self._real if self._real is not None else 0.0

    @property
    def stringValue(self):
        return self._string if self._string is not None else ""


class ObjectCollection(Base):
    def __init__(self):
        self._items = []

    @staticmethod
    def create():
        return ObjectCollection()

    @staticmethod
    def createWithArray(objects):
        c = ObjectCollection()
        for o in objects:
            c.add(o)
        return c

    @property
    def count(self):
        return len(self._items)

    def add(self, item):
        if item is None:
            return False
        self._items.append(item)
        return True

    def item(self, index):
        if not isinstance(index, int) or index < 0 or index >= len(self._items):
            return None
        return self._items[index]

    def removeByIndex(self, index):
        if 0 <= index < len(self._items):
            del self._items[index]
            return True
        return False

    def removeByItem(self, item):
        for i, o in enumerate(self._items):
            if o is item:
                del self._items[i]
                return True
        return False

    def clear(self):
        self._items = []
        return True

    def contains(self, item):
        return any(o is item for o in self._items)

    def find(self, item, startIndex=0):
        for i in range(startIndex, len(self._items)):
            if self._items[i] is item:
                return i
        return -1

    def __iter__(self):
        return iter(list(self._items))

    def __len__(self):
        return len(self._items)
