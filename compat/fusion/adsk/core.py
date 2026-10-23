"""``adsk.core`` subset: Application, UserInterface, Point3D, ValueInput,
ObjectCollection, and the add-in command model: command definitions, events
and handlers, commands and command inputs, toolbar placement stubs
(docs/FUSION_SCRIPTS.md)."""

import math
import sys

from adsk._recorder import Base, Collection, NotSupported, not_in_subset, recorder


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
    def __init__(self):
        self._definitions = CommandDefinitions()
        self._workspaces = Workspaces()
        self._panels = ToolbarPanels(None)
        self._tabs = ToolbarTabs(None)

    @property
    def commandDefinitions(self):
        return self._definitions

    @property
    def workspaces(self):
        return self._workspaces

    @property
    def allToolbarPanels(self):
        return self._panels

    @property
    def allToolbarTabs(self):
        return self._tabs

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

    def log(self, message, level=0, type=0):
        """Printed, like a message box (to standard error through the CLI)."""
        print("[log] %s" % (message,))
        sys.stdout.flush()
        return True


class LogLevels:
    InfoLogLevel = 0
    WarningLogLevel = 1
    ErrorLogLevel = 2


class LogTypes:
    ConsoleLogType = 0
    FileLogType = 1


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



# ---------------------------------------------------------------------------
# Units of command inputs and the units manager
#
# A value input holds a value in internal units (cm, radians) and the
# expression the user typed. A plain number in an expression is in the
# input's unit type, as in the command dialog.


class InvalidValue(ValueError):
    """An expression or value that does not fit an input (not an API gap)."""


def _unit_kind(units):
    """(kind, R-CAD units per one ``units``) of a unit type string."""
    from adsk import _recorder as r

    units = (units or "").strip()
    if units == "":
        return r.UNITLESS, 1.0
    if units in r.RCAD_LENGTH_UNITS:
        return r.LENGTH, r.RCAD_LENGTH_UNITS[units]
    if units in r.EXTRA_LENGTH_UNITS:
        return r.LENGTH, r.EXTRA_LENGTH_UNITS[units]
    if units in r.ANGLE_UNITS:
        return r.ANGLE, r.ANGLE_UNITS[units]
    raise NotSupported(
        "unit type %r is not supported (lengths mm, cm, m, in, ft; angles deg, rad; or \"\")"
        % units
    )


def _quietly(fn, *args):
    """Run a parser; a NotSupported it raises is an invalid value, not an API
    gap, so it is taken back off the recorder's list."""
    rec = recorder()
    n = len(rec.not_supported)
    try:
        return fn(*args)
    except NotSupported as e:
        del rec.not_supported[n:]
        raise InvalidValue(str(e)) from None


def _evaluate(text, units):
    """Internal value (cm / rad / plain) of an expression typed for ``units``."""
    from adsk import _recorder as r

    kind, factor = _unit_kind(units)
    if not isinstance(text, str) or not text.strip():
        raise InvalidValue("an empty expression")

    def parse():
        toks = r.tokenize(text)
        sign = -1.0 if toks and toks[0] == ("op", "-") else 1.0
        body = toks[1:] if toks and toks[0] in (("op", "-"), ("op", "+")) else toks
        if len(body) == 1 and body[0][0] == "num":
            # A plain number is in the input's units.
            return sign * float(body[0][1]) * factor
        q = r.plain_quantity(text, kind)
        if q is not None:
            return q
        return r.Evaluator(recorder().params).eval(text)

    v = _quietly(parse)
    if not math.isfinite(v):
        raise InvalidValue("%r is not a finite value" % text)
    if kind == r.LENGTH:
        return v / 10.0
    if kind == r.ANGLE:
        return math.radians(v)
    return v


def _format(value, units, show_units=True):
    """An internal value as an expression in ``units``."""
    from adsk import _recorder as r

    kind, factor = _unit_kind(units)
    if kind == r.LENGTH:
        x = value * 10.0 / factor
    elif kind == r.ANGLE:
        x = math.degrees(value) / factor
    else:
        x = value
    text = r.fmt(float("%.12g" % x))
    return "%s %s" % (text, units) if show_units and units else text


class UnitsManager(Base):
    """``Design.unitsManager``: R-CAD designs are in millimetres."""

    @property
    def defaultLengthUnits(self):
        return "mm"

    @property
    def internalUnits(self):
        return "cm"

    def evaluateExpression(self, expression, units="cm"):
        return _evaluate(expression, units)

    def isValidExpression(self, expression, units):
        try:
            _evaluate(expression, units)
            return True
        except InvalidValue:
            return False

    def formatInternalValue(self, value, units="", showUnits=True):
        return _format(float(value), units or "mm", showUnits)

    def convert(self, valueInInputUnits, inputUnits, outputUnits):
        ki, fi = _unit_kind(inputUnits)
        ko, fo = _unit_kind(outputUnits)
        if ki != ko:
            raise NotSupported(
                "UnitsManager.convert: %r and %r are different kinds" % (inputUnits, outputUnits)
            )
        return float(valueInInputUnits) * fi / fo


# ---------------------------------------------------------------------------
# Events and handlers
#
# An event keeps the handlers added to it and calls ``handler.notify(args)``
# when the CLI fires it. An exception a handler does not catch fails the run.


class EventHandler:
    """Base of every handler. A plain object: subclasses may keep any state."""

    def notify(self, args):
        pass


class CommandCreatedEventHandler(EventHandler):
    pass


class CommandEventHandler(EventHandler):
    pass


class InputChangedEventHandler(EventHandler):
    pass


class ValidateInputsEventHandler(EventHandler):
    pass


class Event(Base):
    _handler_type = EventHandler

    def __init__(self, sender, name):
        self._sender = sender
        self._name = name
        self._handlers = []

    @property
    def name(self):
        return self._name

    @property
    def sender(self):
        return self._sender

    def _add(self, handler):
        if not callable(getattr(handler, "notify", None)):
            raise TypeError(
                "%s.add needs a %s with a notify(args) method"
                % (self._name, self._handler_type.__name__)
            )
        if any(h is handler for h in self._handlers):
            return False
        self._handlers.append(handler)
        return True

    def _remove(self, handler):
        for i, h in enumerate(self._handlers):
            if h is handler:
                del self._handlers[i]
                return True
        return False

    def _fire(self, args):
        for h in list(self._handlers):
            h.notify(args)


# The handler annotations name the handler class of this module, so helper
# code that looks the handler type up from ``add`` finds it.
class CommandCreatedEvent(Event):
    _handler_type = CommandCreatedEventHandler

    def add(self, handler: "CommandCreatedEventHandler") -> bool:
        return self._add(handler)

    def remove(self, handler: "CommandCreatedEventHandler") -> bool:
        return self._remove(handler)


class CommandEvent(Event):
    _handler_type = CommandEventHandler

    def add(self, handler: "CommandEventHandler") -> bool:
        return self._add(handler)

    def remove(self, handler: "CommandEventHandler") -> bool:
        return self._remove(handler)


class InputChangedEvent(Event):
    _handler_type = InputChangedEventHandler

    def add(self, handler: "InputChangedEventHandler") -> bool:
        return self._add(handler)

    def remove(self, handler: "InputChangedEventHandler") -> bool:
        return self._remove(handler)


class ValidateInputsEvent(Event):
    _handler_type = ValidateInputsEventHandler

    def add(self, handler: "ValidateInputsEventHandler") -> bool:
        return self._add(handler)

    def remove(self, handler: "ValidateInputsEventHandler") -> bool:
        return self._remove(handler)


class EventArgs(Base):
    def __init__(self, event):
        self._event = event

    @property
    def firingEvent(self):
        return self._event


class CommandCreatedEventArgs(EventArgs):
    def __init__(self, event, command):
        super().__init__(event)
        self._command = command

    @property
    def command(self):
        return self._command


class CommandTerminationReason:
    UnknownTerminationReason = 0
    CompletedTerminationReason = 1
    CancelledTerminationReason = 2
    AbortedTerminationReason = 3
    PreEmptedTerminationReason = 4
    SessionEndingTerminationReason = 5


class CommandEventArgs(EventArgs):
    def __init__(self, event, command, reason=CommandTerminationReason.UnknownTerminationReason):
        super().__init__(event)
        self._command = command
        self._failed = False
        self._failed_message = ""
        self._valid_result = False
        self._reason = reason

    @property
    def command(self):
        return self._command

    @property
    def executeFailed(self):
        return self._failed

    @executeFailed.setter
    def executeFailed(self, value):
        self._failed = bool(value)

    @property
    def executeFailedMessage(self):
        return self._failed_message

    @executeFailedMessage.setter
    def executeFailedMessage(self, value):
        self._failed_message = str(value)

    @property
    def isValidResult(self):
        return self._valid_result

    @isValidResult.setter
    def isValidResult(self, value):
        self._valid_result = bool(value)

    @property
    def terminationReason(self):
        return self._reason


class InputChangedEventArgs(EventArgs):
    def __init__(self, event, changed):
        super().__init__(event)
        self._input = changed

    @property
    def input(self):
        return self._input

    @property
    def inputs(self):
        return self._input._command.commandInputs


class ValidateInputsEventArgs(EventArgs):
    def __init__(self, event, command):
        super().__init__(event)
        self._command = command
        self._valid = True

    @property
    def inputs(self):
        return self._command.commandInputs

    @property
    def areInputsValid(self):
        return self._valid

    @areInputsValid.setter
    def areInputsValid(self, value):
        self._valid = bool(value)


# ---------------------------------------------------------------------------
# Command definitions and commands


def _ui_property(name, default):
    """A dialog setting with no effect without a dialog: stored, read back."""

    def get(self):
        return self._ui.get(name, default)

    def put(self, value):
        self._ui[name] = value

    return property(get, put)


class ControlDefinition(Base):
    def __init__(self, definition):
        self._definition = definition
        self._ui = {}

    @property
    def name(self):
        return self._definition._name

    isEnabled = _ui_property("isEnabled", True)
    isVisible = _ui_property("isVisible", True)


class ButtonControlDefinition(ControlDefinition):
    pass


class CommandDefinition(Base):
    def __init__(self, owner, cid, name, tooltip, resource_folder):
        self._owner = owner
        self._id = cid
        self._name = name
        self._tooltip = tooltip
        self._resources = resource_folder
        self._ui = {}
        self._created = CommandCreatedEvent(self, "commandCreated")
        self._control = ButtonControlDefinition(self)
        self._deleted = False

    @property
    def id(self):
        return self._id

    @property
    def name(self):
        return self._name

    @name.setter
    def name(self, value):
        self._name = str(value)

    @property
    def tooltip(self):
        return self._tooltip

    @tooltip.setter
    def tooltip(self, value):
        self._tooltip = str(value)

    @property
    def resourceFolder(self):
        return self._resources

    @resourceFolder.setter
    def resourceFolder(self, value):
        self._resources = str(value)

    toolClipFilename = _ui_property("toolClipFilename", "")

    @property
    def controlDefinition(self):
        return self._control

    @property
    def commandCreated(self):
        return self._created

    @property
    def isValid(self):
        return not self._deleted

    def execute(self, input=None):
        """Requests the command; the CLI runs it after ``run`` returns, as
        a command started from ``run`` starts once ``run`` has returned."""
        if input is not None:
            raise NotSupported("CommandDefinition.execute: the input argument is not mapped")
        self._owner._requested.append(self)
        return True

    def deleteMe(self):
        self._deleted = True
        self._owner._items = [d for d in self._owner._items if d is not self]
        return True


class CommandDefinitions(Collection):
    def __init__(self):
        super().__init__()
        self._requested = []

    def itemById(self, id):
        return next((d for d in self._items if d._id == id), None)

    def addButtonDefinition(self, id, name, tooltip, resourceFolder=""):
        if not isinstance(id, str) or not id:
            raise TypeError("addButtonDefinition: the id must be a non-empty string")
        if self.itemById(id) is not None:
            raise RuntimeError("a command definition with the id %r already exists" % id)
        d = CommandDefinition(self, id, str(name), str(tooltip), str(resourceFolder or ""))
        self._items.append(d)
        return d


class Command(Base):
    """A running instance of a command definition: its inputs and events."""

    def __init__(self, definition):
        self._definition = definition
        self._ui = {}
        self._registry = {}
        self._inputs = CommandInputs(self, None)
        self._execute = CommandEvent(self, "execute")
        self._preview = CommandEvent(self, "executePreview")
        self._destroy = CommandEvent(self, "destroy")
        self._activate = CommandEvent(self, "activate")
        self._deactivate = CommandEvent(self, "deactivate")
        self._changed = InputChangedEvent(self, "inputChanged")
        self._validate = ValidateInputsEvent(self, "validateInputs")

    @property
    def parentCommandDefinition(self):
        return self._definition

    @property
    def commandInputs(self):
        return self._inputs

    @property
    def execute(self):
        return self._execute

    @property
    def executePreview(self):
        return self._preview

    @property
    def destroy(self):
        return self._destroy

    @property
    def activate(self):
        return self._activate

    @property
    def deactivate(self):
        return self._deactivate

    @property
    def inputChanged(self):
        return self._changed

    @property
    def validateInputs(self):
        return self._validate

    okButtonText = _ui_property("okButtonText", "OK")
    cancelButtonText = _ui_property("cancelButtonText", "Cancel")
    isOKButtonVisible = _ui_property("isOKButtonVisible", True)
    isRepeatable = _ui_property("isRepeatable", True)
    isExecutedWhenPreEmpted = _ui_property("isExecutedWhenPreEmpted", True)
    isPositionDependent = _ui_property("isPositionDependent", False)
    helpFile = _ui_property("helpFile", "")

    def setDialogInitialSize(self, width, height):
        return True

    def setDialogMinimumSize(self, width, height):
        return True


# ---------------------------------------------------------------------------
# Command inputs


class CommandInput(Base):
    def __init__(self, parent, cid, name):
        self._parent = parent  # the CommandInputs holding it
        self._command = parent._command
        self._id = cid
        self._name = str(name)
        self._ui = {}
        self._visible = True
        self._enabled = True

    @property
    def id(self):
        return self._id

    @property
    def name(self):
        return self._name

    @property
    def parentCommand(self):
        return self._command

    @property
    def parentCommandInput(self):
        return self._parent._owner

    @property
    def commandInputs(self):
        return self._parent

    @property
    def isVisible(self):
        return self._visible

    @isVisible.setter
    def isVisible(self, value):
        self._visible = bool(value)

    @property
    def isEnabled(self):
        return self._enabled

    @isEnabled.setter
    def isEnabled(self, value):
        self._enabled = bool(value)

    tooltip = _ui_property("tooltip", "")
    tooltipDescription = _ui_property("tooltipDescription", "")
    toolClipFilename = _ui_property("toolClipFilename", "")
    isFullWidth = _ui_property("isFullWidth", False)

    def deleteMe(self):
        self._parent._items = [i for i in self._parent._items if i is not self]
        self._command._registry.pop(self._id, None)
        return True

    # -- headless use (adsk._addin) ----------------------------------------
    def _set_text(self, text):
        raise InvalidValue("%s inputs cannot be set from the command line" % type(self).__name__)

    def _describe(self):
        d = {"id": self._id, "name": self._name, "type": type(self).__name__}
        if not self._visible:
            d["visible"] = False
        if not self._enabled:
            d["enabled"] = False
        return d


class ValueCommandInput(CommandInput):
    def __init__(self, parent, cid, name, units, initial):
        super().__init__(parent, cid, name)
        _unit_kind(units)  # refuses unsupported unit types
        self._units = (units or "").strip()
        self._valid = True
        if not isinstance(initial, ValueInput):
            raise NotSupported("%s: the initial value must be a ValueInput" % type(self).__name__)
        if initial._real is not None:
            self._value = float(initial._real)
            self._expr = _format(self._value, self._units)
        else:
            self._value = _evaluate(initial._string, self._units)
            self._expr = self._normalized(initial._string)

    def _normalized(self, text):
        """The expression as shown: a plain number gets the input's units."""
        t = str(text).strip()
        try:
            float(t)
        except ValueError:
            return t
        return "%s %s" % (t, self._units) if self._units else t

    @property
    def unitType(self):
        return self._units

    @property
    def value(self):
        return self._value

    @value.setter
    def value(self, v):
        if not isinstance(v, (int, float)) or isinstance(v, bool) or not math.isfinite(v):
            raise TypeError("%s.value must be a finite number" % self._id)
        self._value = float(v)
        self._expr = _format(self._value, self._units)
        self._valid = True

    @property
    def expression(self):
        return self._expr

    @expression.setter
    def expression(self, text):
        try:
            self._value = _evaluate(text, self._units)
        except InvalidValue:
            # As in the dialog: an invalid expression is shown, the value stays.
            self._expr = str(text)
            self._valid = False
            return
        self._expr = self._normalized(text)
        self._valid = True

    @property
    def isValidExpression(self):
        return self._valid

    def _set_text(self, text):
        self._value = _evaluate(text, self._units)  # InvalidValue with the reason
        self._expr = self._normalized(text)
        self._valid = True

    def _describe(self):
        d = super()._describe()
        d.update(unitType=self._units, expression=self._expr, value=self._value)
        return d


class DistanceValueCommandInput(ValueCommandInput):
    def setManipulator(self, origin, direction):
        return True


class BoolValueCommandInput(CommandInput):
    def __init__(self, parent, cid, name, is_check_box, resource_folder, initial):
        super().__init__(parent, cid, name)
        self._check = bool(is_check_box)
        self._value = bool(initial)
        self._resources = str(resource_folder or "")

    @property
    def value(self):
        return self._value

    @value.setter
    def value(self, v):
        self._value = bool(v)

    @property
    def isCheckBox(self):
        return self._check

    @property
    def resourceFolder(self):
        return self._resources

    text = _ui_property("text", "")

    def _set_text(self, text):
        t = text.strip().lower()
        if t in ("true", "1", "yes", "on"):
            self._value = True
        elif t in ("false", "0", "no", "off"):
            self._value = False
        else:
            raise InvalidValue("%r is not true or false" % text)

    def _describe(self):
        d = super()._describe()
        d.update(value=self._value, isCheckBox=self._check)
        return d


class IntegerSpinnerCommandInput(CommandInput):
    def __init__(self, parent, cid, name, minimum, maximum, step, initial):
        super().__init__(parent, cid, name)
        for v in (minimum, maximum, step, initial):
            if not isinstance(v, int) or isinstance(v, bool):
                raise TypeError("addIntegerSpinnerCommandInput takes integers")
        if minimum > maximum:
            raise RuntimeError("addIntegerSpinnerCommandInput: the minimum is above the maximum")
        self._min, self._max, self._step = minimum, maximum, step
        self._value = initial

    @property
    def value(self):
        return self._value

    @value.setter
    def value(self, v):
        if not isinstance(v, int) or isinstance(v, bool):
            raise TypeError("%s.value must be an integer" % self._id)
        self._value = v

    @property
    def minimumValue(self):
        return self._min

    @property
    def maximumValue(self):
        return self._max

    @property
    def spinStep(self):
        return self._step

    def _set_text(self, text):
        try:
            v = int(text.strip())
        except ValueError:
            raise InvalidValue("%r is not an integer" % text) from None
        if not self._min <= v <= self._max:
            raise InvalidValue("%d is outside %d..%d" % (v, self._min, self._max))
        self._value = v

    def _describe(self):
        d = super()._describe()
        d.update(value=self._value, minimum=self._min, maximum=self._max, step=self._step)
        return d


class DropDownStyles:
    LabeledIconDropDownStyle = 0
    CheckBoxDropDownStyle = 1
    TextListDropDownStyle = 2


class ListItem(Base):
    def __init__(self, owner, name, icon):
        self._owner = owner
        self._name = str(name)
        self._selected = False
        self._icon = str(icon or "")

    @property
    def name(self):
        return self._name

    @name.setter
    def name(self, value):
        self._name = str(value)

    @property
    def index(self):
        return self._owner._items.index(self)

    @property
    def icon(self):
        return self._icon

    @property
    def isSelected(self):
        return self._selected

    @isSelected.setter
    def isSelected(self, value):
        if value and self._owner._single:
            for i in self._owner._items:
                i._selected = False
        self._selected = bool(value)

    def deleteMe(self):
        self._owner._items = [i for i in self._owner._items if i is not self]
        return True


class ListItems(Collection):
    def __init__(self, single):
        super().__init__()
        self._single = single

    def add(self, name, isSelected, icon="", beforeIndex=-1):
        item = ListItem(self, name, icon)
        if isinstance(beforeIndex, int) and 0 <= beforeIndex < len(self._items):
            self._items.insert(beforeIndex, item)
        else:
            self._items.append(item)
        if isSelected:
            item.isSelected = True
        return item

    def clear(self):
        self._items = []
        return True


class DropDownCommandInput(CommandInput):
    def __init__(self, parent, cid, name, style):
        super().__init__(parent, cid, name)
        self._style = style
        self._list = ListItems(style != DropDownStyles.CheckBoxDropDownStyle)

    @property
    def dropDownStyle(self):
        return self._style

    @property
    def listItems(self):
        return self._list

    @property
    def selectedItem(self):
        return next((i for i in self._list._items if i._selected), None)

    maxVisibleItems = _ui_property("maxVisibleItems", 0)

    def _set_text(self, text):
        names = [text] if self._list._single else [n.strip() for n in text.split(",") if n.strip()]
        found = []
        for n in names:
            item = next((i for i in self._list._items if i._name == n), None)
            if item is None:
                raise InvalidValue(
                    "%r is not an item (items: %s)"
                    % (n, ", ".join(repr(i._name) for i in self._list._items))
                )
            found.append(item)
        for i in self._list._items:
            i._selected = False
        for i in found:
            i._selected = True

    def _describe(self):
        d = super()._describe()
        sel = [i._name for i in self._list._items if i._selected]
        d.update(
            items=[i._name for i in self._list._items],
            selected=sel if not self._list._single else (sel[0] if sel else None),
            multiple=not self._list._single,
        )
        return d


class Selection(Base):
    def __init__(self, entity):
        self._entity = entity

    @property
    def entity(self):
        return self._entity


_FACE_FILTERS = {"Faces", "PlanarFaces"}
_BODY_FILTERS = {"Bodies", "SolidBodies"}


class SelectionCommandInput(CommandInput):
    def __init__(self, parent, cid, name, prompt):
        super().__init__(parent, cid, name)
        self._prompt = str(prompt)
        self._filters = []
        self._min, self._max = 1, 1
        self._selections = []

    @property
    def commandPrompt(self):
        return self._prompt

    @commandPrompt.setter
    def commandPrompt(self, value):
        self._prompt = str(value)

    @property
    def selectionFilters(self):
        return list(self._filters)

    def addSelectionFilter(self, filter):
        self._filters.append(str(filter))
        return True

    def clearSelectionFilter(self):
        self._filters = []
        return True

    def setSelectionLimits(self, minimum, maximum=0):
        for v in (minimum, maximum):
            if not isinstance(v, int) or isinstance(v, bool) or v < 0:
                raise TypeError("setSelectionLimits takes non-negative integers")
        self._min, self._max = minimum, maximum
        return True

    @property
    def selectionCount(self):
        return len(self._selections)

    def selection(self, index):
        if not isinstance(index, int) or not 0 <= index < len(self._selections):
            return None
        return self._selections[index]

    def _accepts(self, entity):
        from adsk import fusion

        if isinstance(entity, fusion.BRepFace):
            ok = _FACE_FILTERS
        elif isinstance(entity, fusion.BRepBody):
            ok = _BODY_FILTERS
        else:
            return False
        return not self._filters or any(f in ok for f in self._filters)

    def addSelection(self, entity):
        if not self._accepts(entity):
            return False
        if self._max and len(self._selections) >= self._max:
            return False
        self._selections.append(Selection(entity))
        return True

    def clearSelection(self):
        self._selections = []
        return True

    def _describe(self):
        d = super()._describe()
        d.update(
            prompt=self._prompt,
            filters=list(self._filters),
            minimum=self._min,
            maximum=self._max,
            selected=len(self._selections),
        )
        return d


class StringValueCommandInput(CommandInput):
    def __init__(self, parent, cid, name, initial):
        super().__init__(parent, cid, name)
        self._value = str(initial)

    @property
    def value(self):
        return self._value

    @value.setter
    def value(self, v):
        self._value = str(v)

    isPassword = _ui_property("isPassword", False)
    isReadOnly = _ui_property("isReadOnly", False)

    def _set_text(self, text):
        if self.isReadOnly:
            raise InvalidValue("the input is read-only")
        self._value = text

    def _describe(self):
        d = super()._describe()
        d.update(value=self._value)
        return d


class TextBoxCommandInput(CommandInput):
    def __init__(self, parent, cid, name, text, rows, read_only):
        super().__init__(parent, cid, name)
        self._text = str(text)
        self._rows = rows
        self._read_only = bool(read_only)

    @property
    def text(self):
        return self._text

    @text.setter
    def text(self, v):
        self._text = str(v)

    @property
    def formattedText(self):
        return self._text

    @formattedText.setter
    def formattedText(self, v):
        self._text = str(v)

    @property
    def numRows(self):
        return self._rows

    @numRows.setter
    def numRows(self, v):
        self._rows = v

    @property
    def isReadOnly(self):
        return self._read_only

    @isReadOnly.setter
    def isReadOnly(self, v):
        self._read_only = bool(v)

    def _set_text(self, text):
        if self._read_only:
            raise InvalidValue("the text box is read-only")
        self._text = text

    def _describe(self):
        d = super()._describe()
        d.update(text=self._text, readOnly=self._read_only)
        return d


class GroupCommandInput(CommandInput):
    def __init__(self, parent, cid, name):
        super().__init__(parent, cid, name)
        self._children = CommandInputs(self._command, self)

    @property
    def children(self):
        return self._children

    isExpanded = _ui_property("isExpanded", True)
    isEnabledCheckBoxDisplayed = _ui_property("isEnabledCheckBoxDisplayed", False)

    def _describe(self):
        d = super()._describe()
        d["children"] = [i._describe() for i in self._children._items]
        return d


class TabCommandInput(GroupCommandInput):
    def activate(self):
        return True


class CommandInputs(Collection):
    def __init__(self, command, owner):
        super().__init__()
        self._command = command
        self._owner = owner  # the group or tab input, or None

    @property
    def command(self):
        return self._command

    def itemById(self, id):
        """Any input of the command, in a group or tab or not."""
        return self._command._registry.get(id)

    def _add(self, cls, cid, *args):
        if not isinstance(cid, str) or not cid:
            raise TypeError("a command input id must be a non-empty string")
        if cid in self._command._registry:
            raise RuntimeError("a command input with the id %r already exists" % cid)
        i = cls(self, cid, *args)
        self._items.append(i)
        self._command._registry[cid] = i
        return i

    def addValueInput(self, id, name, unitType, initialValue):
        return self._add(ValueCommandInput, id, name, unitType, initialValue)

    def addDistanceValueCommandInput(self, id, name, initialValue):
        return self._add(DistanceValueCommandInput, id, name, "mm", initialValue)

    def addBoolValueInput(self, id, name, isCheckBox, resourceFolder="", initialValue=False):
        return self._add(BoolValueCommandInput, id, name, isCheckBox, resourceFolder, initialValue)

    def addIntegerSpinnerCommandInput(self, id, name, min, max, spinStep, initialValue):
        return self._add(IntegerSpinnerCommandInput, id, name, min, max, spinStep, initialValue)

    def addDropDownCommandInput(self, id, name, dropDownStyle):
        return self._add(DropDownCommandInput, id, name, dropDownStyle)

    def addSelectionInput(self, id, name, commandPrompt):
        return self._add(SelectionCommandInput, id, name, commandPrompt)

    def addStringValueInput(self, id, name, initialValue=""):
        return self._add(StringValueCommandInput, id, name, initialValue)

    def addTextBoxCommandInput(self, id, name, formattedText, numRows, isReadOnly):
        return self._add(TextBoxCommandInput, id, name, formattedText, numRows, isReadOnly)

    def addGroupCommandInput(self, id, name):
        return self._add(GroupCommandInput, id, name)

    def addTabCommandInput(self, id, name, resourceFolder=""):
        return self._add(TabCommandInput, id, name)


# ---------------------------------------------------------------------------
# Workspaces, toolbars and controls: placement is recorded (for --list); there
# is no user interface. An unknown id gives a placeholder, since the built-in
# ids of the Fusion user interface are not modelled.


class CommandControl(Base):
    def __init__(self, owner, definition):
        self._owner = owner
        self._definition = definition
        self._ui = {}

    @property
    def id(self):
        return self._definition._id

    @property
    def commandDefinition(self):
        return self._definition

    isPromoted = _ui_property("isPromoted", False)
    isPromotedByDefault = _ui_property("isPromotedByDefault", False)
    isVisible = _ui_property("isVisible", True)

    def deleteMe(self):
        self._owner._items = [c for c in self._owner._items if c is not self]
        return True


class SeparatorControl(Base):
    def __init__(self, owner, cid):
        self._owner = owner
        self._id = cid

    @property
    def id(self):
        return self._id

    def deleteMe(self):
        self._owner._items = [c for c in self._owner._items if c is not self]
        return True


class DropDownControl(Base):
    def __init__(self, owner, cid):
        self._owner = owner
        self._id = cid
        self._controls = ToolbarControls()

    @property
    def id(self):
        return self._id

    @property
    def controls(self):
        return self._controls

    def deleteMe(self):
        self._owner._items = [c for c in self._owner._items if c is not self]
        return True


class ToolbarControls(Collection):
    def itemById(self, id):
        return next((c for c in self._items if c.id == id), None)

    def addCommand(self, commandDefinition, positionID="", isBefore=True):
        if not isinstance(commandDefinition, CommandDefinition):
            raise TypeError("addCommand takes a CommandDefinition")
        c = CommandControl(self, commandDefinition)
        self._items.append(c)
        return c

    def addSeparator(self, id="", positionID="", isBefore=True):
        c = SeparatorControl(self, id)
        self._items.append(c)
        return c

    def addDropDown(self, text, resourceFolder, id="", positionID="", isBefore=True):
        c = DropDownControl(self, id)
        self._items.append(c)
        return c


class ToolbarPanel(Base):
    def __init__(self, owner, pid, name):
        self._owner = owner
        self._id = pid
        self._name = name
        self._controls = ToolbarControls()

    @property
    def id(self):
        return self._id

    @property
    def name(self):
        return self._name

    @property
    def controls(self):
        return self._controls

    def deleteMe(self):
        self._owner._items = [p for p in self._owner._items if p is not self]
        return True


class ToolbarPanels(Collection):
    def __init__(self, parent):
        super().__init__()
        self._parent = parent  # a Workspace, a ToolbarTab, or None (all panels)

    def itemById(self, id):
        p = next((p for p in self._items if p._id == id), None)
        if p is None:
            p = ToolbarPanel(self, id, id)
            self._items.append(p)
        return p

    def add(self, id, name, positionID="", isBefore=True):
        p = ToolbarPanel(self, id, name)
        self._items.append(p)
        return p


class ToolbarTab(Base):
    def __init__(self, owner, tid, name):
        self._owner = owner
        self._id = tid
        self._name = name
        self._panels = ToolbarPanels(self)

    @property
    def id(self):
        return self._id

    @property
    def name(self):
        return self._name

    @property
    def toolbarPanels(self):
        return self._panels

    def deleteMe(self):
        self._owner._items = [t for t in self._owner._items if t is not self]
        return True


class ToolbarTabs(Collection):
    def __init__(self, parent):
        super().__init__()
        self._parent = parent

    def itemById(self, id):
        t = next((t for t in self._items if t._id == id), None)
        if t is None:
            t = ToolbarTab(self, id, id)
            self._items.append(t)
        return t

    def add(self, id, name):
        t = ToolbarTab(self, id, name)
        self._items.append(t)
        return t


class Workspace(Base):
    def __init__(self, wid):
        self._id = wid
        self._panels = ToolbarPanels(self)
        self._tabs = ToolbarTabs(self)

    @property
    def id(self):
        return self._id

    @property
    def name(self):
        return self._id

    @property
    def toolbarPanels(self):
        return self._panels

    @property
    def toolbarTabs(self):
        return self._tabs


class Workspaces(Collection):
    def itemById(self, id):
        w = next((w for w in self._items if w._id == id), None)
        if w is None:
            w = Workspace(id)
            self._items.append(w)
        return w


def _placements(ui, definition):
    """Where the add-in placed controls for ``definition``."""
    out = []

    def walk(controls, where):
        for c in controls._items:
            if isinstance(c, CommandControl) and c._definition is definition:
                out.append(dict(where, promoted=bool(c.isPromoted)))
            elif isinstance(c, DropDownControl):
                walk(c._controls, dict(where, dropDown=c._id))

    def panels(coll, where):
        for p in coll._items:
            walk(p._controls, dict(where, panel=p._id))

    def tabs(coll, where):
        for t in coll._items:
            panels(t._panels, dict(where, tab=t._id))

    panels(ui._panels, {})
    tabs(ui._tabs, {})
    for w in ui._workspaces._items:
        panels(w._panels, {"workspace": w._id})
        tabs(w._tabs, {"workspace": w._id})
    return out
