# Standoffs (example plugin)

PCB mounting posts with pilot holes at points on a planar face, written with
`rcad_plugin.py`, a one-file standard-library helper for Python plugins (copy it next to
your script). See docs/PLUGINS.md for the protocol and installation.

Try it:

```sh
RCAD_PLUGIN_PATH="$PWD/examples/plugins" rcad-cli plugins
```

`crates/rcad-commands/tests/it/external_plugin.rs` builds posts on a plate through a
document and checks the volume against the closed form.
