# Vent slots (example external plugin)

A plugin served by an external program: `plugin.json` declares the feature,
`plugin.py` (Python 3, standard library only) answers the requests.

Feature `vent_slots` cuts `count` parallel rounded slots into a planar face,
centred on the face's frame origin:

| Parameter      | Kind   | Default | Meaning                                            |
|----------------|--------|---------|----------------------------------------------------|
| `count`        | count  | 5       | number of slots (1 to 50)                          |
| `slot_width`   | length | 2 mm    | width of a slot (diameter of its round ends)       |
| `slot_length`  | length | 20 mm   | overall length of a slot, ends included            |
| `pitch`        | length | 4 mm    | centre distance between neighbouring slots         |
| `depth`        | length | 0 mm    | cut depth below the face; 0 cuts through the body  |
| `rotation_deg` | angle  | 0°      | rotation of the row about the face normal          |

Slot `i` (0-based) is centred at `(0, (i - (count - 1) / 2) * pitch)` in the
face's plane coordinates, rotated by `rotation_deg`; its long axis is the
rotated `x` axis. Each slot is one `prism` operation in `cut` mode with
operation id `i`, so a later feature that references a slot face keeps it
when the count or sizes change. The tool reaches 1 mm above the face; a
through cut starts 1 mm below the body (`body_span`).

The plugin refuses (with an `error` answer) slots shorter than they are wide
and a pitch that would make neighbouring slots touch.

## Install

Copy or link this directory into a plugin search directory, e.g.
`~/.local/share/r-cad/plugins/vent-slots`, or add the absolute path of
`examples/plugins` to `RCAD_PLUGIN_PATH`. `python3` must be on `PATH`.

## Protocol

Every call runs `python3 -I plugin.py` in this directory with one JSON
request on standard input; the program prints one JSON answer and exits:

```text
-> {"method": "build", "request": {"feature": "vent_slots", "params": {...}, "choices": {}, "inputs": {"face": {"kind": "face", ...}}, "data": null}}
<- {"ok": [{"op": "prism", "id": 0, "body": "face", "frame": {...}, "profile": {"shape": "loop", "segments": [...]}, "from": -9.0, "to": 1.0, "mode": "cut", "label": "slot 1"}, ...]}
```

Try it by hand:

```sh
echo '{"method": "rule_symbols", "data": null, "body": 1}' | python3 -I plugin.py
```
