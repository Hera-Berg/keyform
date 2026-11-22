# Keyform shortcut file

File ▸ Keyboard Shortcuts… ▸ Export / Import reads and writes one XML file
with the whole binding set (`50-ui/shortcuts.md`, "Import / Export file").
It names commands by **Keyform command IDs** (`crates/keyform-app/src/shortcuts.rs`,
`COMMANDS`); files of the reference editor are not read.

## Version 1

```xml
<?xml version="1.0" encoding="UTF-8"?>
<KeyformShortcuts version="1">
  <mode name="main">
    <group name="File">
      <item command="file.save" key1="Ctrl+S" key2="Ctrl+Alt+W"/>
      ...
    </group>
    ...
  </mode>
</KeyformShortcuts>
```

- The root element `KeyformShortcuts` carries `version`; readers refuse
  versions they don't know.
- One `mode` element per scope (only `main` so far). Inside a mode,
  `group` elements (the dialog's categories) may nest to any depth.
- Each `item` has the command ID and up to two bindings, `key1` and `key2`.
  Empty bindings are left out. A binding is modifiers then a key, joined
  with `+`: `Ctrl`, `Alt`, `Shift` (on macOS `Ctrl` is ⌘), then an egui
  key name (`A`, `1`, `Delete`, `Enter`, `Left`, `F2`, …).
- Export lists every command with at least one binding, grouped as the
  dialog shows them.
- Import replaces the current set after confirmation. Commands that aren't
  in the file keep their defaults; unknown command IDs are skipped and
  reported together; overlapping keys are reported ("The shortcut keys are
  overlapping. It may not function correctly."). The clipboard commands
  (Cut, Copy, Paste, Copy Form, Paste Form) and the brush key are fixed
  and ignored on import.
