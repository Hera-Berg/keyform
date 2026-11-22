//! The command table, key bindings and File ▸ Keyboard Shortcuts…
//! [Ctrl+Alt+Shift+K]. spec: 50-ui/shortcuts.md, 50-ui/dialogs.md §12.
//!
//! Every command has an ID and up to two bindings; the defaults are those
//! of menus.md. Bindings are matched with exact modifiers. The clipboard
//! commands (Cut / Copy / Paste and Copy / Paste Form) reach the app as
//! clipboard events rather than keys (egui-winit), so their keys are fixed.
//! The Import / Export file is Keyform's own (docs/shortcuts-file.md).

use std::collections::BTreeMap;

use eframe::egui::{self, Key, Modifiers};

/// A key with modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Combo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

impl Combo {
    /// "Ctrl+Shift+S", "Delete", "1", "Ctrl+Alt+Shift+Left".
    pub fn parse(s: &str) -> Option<Self> {
        let mut c = Combo {
            ctrl: false,
            shift: false,
            alt: false,
            key: Key::A,
        };
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let (last, mods) = parts.split_last()?;
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" => c.ctrl = true,
                "shift" => c.shift = true,
                "alt" => c.alt = true,
                _ => return None,
            }
        }
        c.key = Key::from_name(last).or_else(|| match *last {
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "0" => {
                Key::from_name(&format!("Num{last}"))
            }
            _ => None,
        })?;
        Some(c)
    }

    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        let k = self.key.name();
        s.push_str(k.strip_prefix("Num").filter(|d| d.len() == 1).unwrap_or(k));
        s
    }

    fn matches(&self, m: Modifiers, key: Key) -> bool {
        key == self.key
            && (m.command || m.ctrl) == self.ctrl
            && m.shift == self.shift
            && m.alt == self.alt
    }

    /// The combo of a key event.
    pub fn of(m: Modifiers, key: Key) -> Self {
        Combo {
            ctrl: m.command || m.ctrl,
            shift: m.shift,
            alt: m.alt,
            key,
        }
    }
}

/// One command of the table.
pub struct Command {
    pub id: &'static str,
    /// The dialog's category (menus.md / shortcuts.md groups).
    pub group: &'static str,
    pub label: &'static str,
    pub keys: &'static [&'static str],
    /// Keys that can't be changed (clipboard events).
    pub fixed: bool,
}

const fn cmd(
    id: &'static str,
    group: &'static str,
    label: &'static str,
    keys: &'static [&'static str],
) -> Command {
    Command {
        id,
        group,
        label,
        keys,
        fixed: false,
    }
}

const fn fixed(
    id: &'static str,
    group: &'static str,
    label: &'static str,
    keys: &'static [&'static str],
) -> Command {
    Command {
        id,
        group,
        label,
        keys,
        fixed: true,
    }
}

/// Every command with a binding, in dialog order.
pub const COMMANDS: &[Command] = &[
    cmd("file.new_model", "File", "New Model", &["Ctrl+N"]),
    cmd("file.open", "File", "Open…", &["Ctrl+O"]),
    cmd(
        "file.open_recent",
        "File",
        "Open Recent File",
        &["Ctrl+Shift+O"],
    ),
    cmd("file.close", "File", "Close", &["Ctrl+W"]),
    cmd("file.save", "File", "Save", &["Ctrl+S"]),
    cmd("file.save_as", "File", "Save as…", &["Ctrl+Shift+S"]),
    cmd(
        "file.export_still",
        "File",
        "Still Image (Sequence)…",
        &["Ctrl+I"],
    ),
    cmd("file.export_psd", "File", "PSD images(beta)…", &["Ctrl+P"]),
    cmd("file.export_video", "File", "Video…", &["Ctrl+Shift+M"]),
    cmd(
        "file.export_motion",
        "File",
        "Export motion file…",
        &["Ctrl+Alt+Shift+S"],
    ),
    cmd(
        "file.keyboard_shortcuts",
        "File",
        "Keyboard Shortcuts…",
        &["Ctrl+Alt+Shift+K"],
    ),
    cmd("edit.undo", "Edit", "Undo", &["Ctrl+Z"]),
    cmd("edit.redo", "Edit", "Redo", &["Ctrl+Y", "Ctrl+Shift+Z"]),
    fixed("edit.cut", "Edit", "Cut", &["Ctrl+X"]),
    fixed("edit.copy", "Edit", "Copy", &["Ctrl+C"]),
    fixed("edit.paste", "Edit", "Paste", &["Ctrl+V"]),
    cmd("edit.delete", "Edit", "Delete", &["Delete", "Backspace"]),
    cmd("edit.deselect", "Edit", "Deselect", &["Escape"]),
    cmd("edit.select_all", "Edit", "Select All", &["Ctrl+A"]),
    cmd("edit.search", "Edit", "Search and Replace…", &["Ctrl+F"]),
    cmd("show.zoom_in", "Show", "Zoom In", &["Ctrl+Alt+Shift+Left"]),
    cmd(
        "show.zoom_out",
        "Show",
        "Zoom Out",
        &["Ctrl+Alt+Shift+Right"],
    ),
    cmd(
        "modeling.reset_params",
        "Modeling",
        "Reset to default values",
        &["Ctrl+1"],
    ),
    cmd(
        "modeling.synth_corners",
        "Modeling",
        "Synthesize corners…",
        &["Ctrl+4"],
    ),
    cmd("modeling.mesh_edit", "Modeling", "Mesh edit", &["Ctrl+E"]),
    cmd(
        "modeling.auto_mesh",
        "Modeling",
        "Automatic Mesh generator…",
        &["Ctrl+Shift+A"],
    ),
    fixed(
        "modeling.copy_form",
        "Modeling",
        "Copy Form",
        &["Ctrl+Shift+C"],
    ),
    fixed(
        "modeling.paste_form",
        "Modeling",
        "Paste Form",
        &["Ctrl+Shift+V"],
    ),
    cmd(
        "modeling.blend_form",
        "Modeling",
        "Blend Form…",
        &["Ctrl+Shift+B"],
    ),
    cmd(
        "modeling.atlas",
        "Modeling",
        "Edit Texture Atlas…",
        &["Ctrl+T"],
    ),
    cmd(
        "modeling.lock_selected",
        "Modeling",
        "Lock/unlock Selected Objects",
        &["Ctrl+2"],
    ),
    cmd(
        "modeling.lock_unselected",
        "Modeling",
        "Lock the unselected objects",
        &["Ctrl+Shift+2"],
    ),
    cmd(
        "modeling.unlock_all",
        "Modeling",
        "Unlock all",
        &["Ctrl+Alt+2"],
    ),
    cmd(
        "modeling.hide_selected",
        "Modeling",
        "Show/Hide Selected Objects",
        &["Ctrl+3"],
    ),
    cmd(
        "modeling.hide_unselected",
        "Modeling",
        "Hide the unselected objects",
        &["Ctrl+Shift+3"],
    ),
    cmd("modeling.show_all", "Modeling", "Show all", &["Ctrl+Alt+3"]),
    cmd("modeling.level1", "Modeling", "Level 1 (refine)", &["1"]),
    cmd("modeling.level2", "Modeling", "Level 2 (normal)", &["2"]),
    cmd("modeling.level3", "Modeling", "Level 3 (rough)", &["3"]),
    cmd(
        "modeling.glue",
        "Modeling",
        "Glue / bind the selected vertices",
        &["Ctrl+G"],
    ),
    cmd(
        "modeling.unglue",
        "Modeling",
        "Unglue / unbind the selected vertices",
        &["Ctrl+Shift+G"],
    ),
    cmd(
        "mesh_edit.auto_attach",
        "Tool details: mesh edit mode",
        "Auto attach Edge",
        &["Ctrl+R"],
    ),
    cmd("animation.playback", "Animation", "Playback", &["Enter"]),
    cmd("tools.arrow", "Tools", "Arrow tool", &["A"]),
    cmd("tools.lasso", "Tools", "Lasso Tool", &["L"]),
    fixed("tools.brush", "Tools", "Brush Selection tool", &["B"]),
    cmd("tools.control_point", "Tools", "Control Point edit", &["P"]),
];

pub fn command(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// The user's bindings: changes from the defaults, by command ID ("" = no
/// binding in that slot).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Bindings {
    pub changed: BTreeMap<String, [String; 2]>,
}

impl Bindings {
    /// The two bindings of `id`.
    pub fn get(&self, id: &str) -> [Option<Combo>; 2] {
        match self.changed.get(id) {
            Some([a, b]) => [Combo::parse(a), Combo::parse(b)],
            None => {
                let k = command(id).map_or(&[][..], |c| c.keys);
                [
                    k.first().and_then(|s| Combo::parse(s)),
                    k.get(1).and_then(|s| Combo::parse(s)),
                ]
            }
        }
    }

    /// The primary binding as shown on menu items ("" when none).
    pub fn text(&self, id: &str) -> String {
        let [a, b] = self.get(id);
        a.or(b).map(|c| c.label()).unwrap_or_default()
    }

    pub fn set(&mut self, id: &str, slot: usize, combo: Option<Combo>) {
        let [a, b] = self.get(id);
        let mut v = [
            a.map(|c| c.label()).unwrap_or_default(),
            b.map(|c| c.label()).unwrap_or_default(),
        ];
        v[slot.min(1)] = combo.map(|c| c.label()).unwrap_or_default();
        self.changed.insert(id.to_owned(), v);
        self.normalise(id);
    }

    /// Drops an entry equal to the defaults.
    fn normalise(&mut self, id: &str) {
        let default = Bindings::default().get(id);
        if self.get(id) == default {
            self.changed.remove(id);
        }
    }

    /// Commands (other than `except`) that have `combo`.
    pub fn users(&self, combo: Combo, except: &str) -> Vec<&'static str> {
        COMMANDS
            .iter()
            .filter(|c| c.id != except && self.get(c.id).contains(&Some(combo)))
            .map(|c| c.id)
            .collect()
    }

    /// Whether any key is bound to more than one command.
    pub fn has_conflicts(&self) -> bool {
        COMMANDS.iter().any(|c| {
            self.get(c.id)
                .iter()
                .flatten()
                .any(|k| !self.users(*k, c.id).is_empty())
        })
    }

    /// Consumes a key press bound to `id` (exact modifiers). Returns whether
    /// one was found.
    pub fn hit(&self, i: &mut egui::InputState, id: &str) -> bool {
        let keys = self.get(id);
        let pos = i.events.iter().position(|e| match e {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => keys.iter().flatten().any(|c| c.matches(*modifiers, *key)),
            _ => false,
        });
        match pos {
            Some(p) => {
                i.events.remove(p);
                true
            }
            None => false,
        }
    }
}

// ------------------------------------------------------- import / export

/// The Keyform shortcut file (docs/shortcuts-file.md), version 1: every
/// command with at least one binding, grouped as the dialog shows them.
pub fn export_xml(b: &Bindings) -> String {
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<KeyformShortcuts version=\"1\">\n  <mode name=\"main\">\n",
    );
    let mut groups: Vec<&str> = Vec::new();
    for c in COMMANDS {
        if !groups.contains(&c.group) {
            groups.push(c.group);
        }
    }
    for g in groups {
        let items: Vec<String> = COMMANDS
            .iter()
            .filter(|c| c.group == g)
            .filter_map(|c| {
                let [a, k2] = b.get(c.id);
                if a.is_none() && k2.is_none() {
                    return None;
                }
                let mut s = format!("      <item command=\"{}\"", esc(c.id));
                if let Some(a) = a {
                    s.push_str(&format!(" key1=\"{}\"", esc(&a.label())));
                }
                if let Some(k2) = k2 {
                    s.push_str(&format!(" key2=\"{}\"", esc(&k2.label())));
                }
                s.push_str("/>\n");
                Some(s)
            })
            .collect();
        if items.is_empty() {
            continue;
        }
        out.push_str(&format!("    <group name=\"{}\">\n", esc(g)));
        for s in items {
            out.push_str(&s);
        }
        out.push_str("    </group>\n");
    }
    out.push_str("  </mode>\n</KeyformShortcuts>\n");
    out
}

/// Reads a Keyform shortcut file: the whole set (commands not in the file
/// keep their defaults) and the unknown command IDs.
pub fn import_xml(text: &str) -> Result<(Bindings, Vec<String>), String> {
    let doc = keyform_formats::xml::XmlDocument::parse(text.as_bytes())
        .map_err(|e| format!("Not a shortcut file: {e}"))?;
    if doc.root.name != "KeyformShortcuts" {
        return Err("Not a Keyform shortcut file.".into());
    }
    match doc.root.attr("version") {
        Some("1") => {}
        v => return Err(format!("Unsupported shortcut file version {v:?}.")),
    }
    let mut b = Bindings::default();
    let mut unknown = Vec::new();
    fn walk(e: &keyform_formats::xml::Element, b: &mut Bindings, unknown: &mut Vec<String>) {
        for c in &e.children {
            if c.name == "item" {
                let id = c.attr("command").unwrap_or_default();
                if command(id).is_none_or(|x| x.fixed) {
                    if command(id).is_none() {
                        unknown.push(id.to_owned());
                    }
                    continue;
                }
                let k = |a: &str| {
                    c.attr(a)
                        .and_then(Combo::parse)
                        .map(|x| x.label())
                        .unwrap_or_default()
                };
                b.changed.insert(id.to_owned(), [k("key1"), k("key2")]);
                b.normalise(id);
            } else {
                walk(c, b, unknown);
            }
        }
    }
    walk(&doc.root, &mut b, &mut unknown);
    Ok((b, unknown))
}

// ----------------------------------------------------------------- dialog

#[derive(Default)]
pub struct ShortcutsDialog {
    pub open: bool,
    /// Waiting for a key for (command, slot).
    capture: Option<(&'static str, usize)>,
    /// "A shortcut is already registered to this key…": (command, slot,
    /// combo, current users).
    replace: Option<(&'static str, usize, Combo, Vec<&'static str>)>,
    /// An import waiting for confirmation, with its report.
    import: Option<(Bindings, Vec<String>)>,
    message: Option<String>,
}

pub fn window(ctx: &egui::Context, st: &mut ShortcutsDialog, b: &mut Bindings) {
    if !st.open {
        return;
    }
    // A key for the slot being set.
    if let Some((id, slot)) = st.capture {
        let pressed = ctx.input_mut(|i| {
            let p = i.events.iter().position(|e| {
                matches!(e, egui::Event::Key { pressed: true, key, .. }
                    if !matches!(key, Key::Escape))
            });
            let esc = i.consume_key(Modifiers::NONE, Key::Escape);
            match p {
                Some(p) => match i.events.remove(p) {
                    egui::Event::Key { key, modifiers, .. } => {
                        Some(Some(Combo::of(modifiers, key)))
                    }
                    _ => None,
                },
                None if esc => Some(None),
                None => None,
            }
        });
        match pressed {
            Some(Some(c)) => {
                st.capture = None;
                let users = b.users(c, id);
                if users.iter().any(|u| command(u).is_some_and(|x| x.fixed)) {
                    st.message = Some("The same shortcut is already registered.".into());
                } else if users.is_empty() {
                    b.set(id, slot, Some(c));
                } else {
                    st.replace = Some((id, slot, c, users));
                }
            }
            Some(None) => st.capture = None,
            None => {}
        }
    }
    let mut open = true;
    egui::Window::new(crate::i18n::tr("Keyboard Shortcuts"))
        .default_size([560.0, 560.0])
        .open(&mut open)
        .show(ctx, |ui| {
            if let Some((id, slot, c, users)) = st.replace.clone() {
                ui.label(crate::i18n::tr(
                    "A shortcut is already registered to this key. Would you like to delete the existing setting?",
                ));
                ui.weak(
                    users
                        .iter()
                        .filter_map(|u| command(u).map(|c| c.label))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr("Yes")).clicked() {
                        for u in &users {
                            for s in 0..2 {
                                if b.get(u)[s] == Some(c) {
                                    b.set(u, s, None);
                                }
                            }
                        }
                        b.set(id, slot, Some(c));
                        st.replace = None;
                    }
                    if ui.button(crate::i18n::tr("No")).clicked() {
                        st.replace = None;
                    }
                });
                ui.separator();
            }
            if let Some((_, unknown)) = &st.import {
                ui.label(crate::i18n::tr(
                    "Replace all shortcut keys with the ones in the file?",
                ));
                if !unknown.is_empty() {
                    ui.weak(format!("Unknown commands skipped: {}", unknown.join(", ")));
                }
                let mut answer = None;
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr("Yes")).clicked() {
                        answer = Some(true);
                    }
                    if ui.button(crate::i18n::tr("No")).clicked() {
                        answer = Some(false);
                    }
                });
                if let Some(yes) = answer {
                    let (nb, _) = st.import.take().unwrap_or_default();
                    if yes {
                        *b = nb;
                        if b.has_conflicts() {
                            st.message = Some(
                                "The shortcut keys are overlapping. It may not function correctly."
                                    .into(),
                            );
                        }
                    }
                }
                ui.separator();
            }
            if let Some(m) = &st.message {
                ui.colored_label(ui.visuals().warn_fg_color, m);
            }
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("Import…")).clicked() {
                    if let Some(p) = crate::file_dialog::FileDialog::new()
                        .add_filter("Keyform shortcuts (*.xml)", &["xml"])
                        .pick_file()
                    {
                        match std::fs::read_to_string(&p)
                            .map_err(|e| e.to_string())
                            .and_then(|t| import_xml(&t))
                        {
                            Ok(r) => st.import = Some(r),
                            Err(e) => st.message = Some(e),
                        }
                    }
                }
                if ui.button(crate::i18n::tr("Export…")).clicked() {
                    if let Some(p) = crate::file_dialog::FileDialog::new()
                        .add_filter("Keyform shortcuts (*.xml)", &["xml"])
                        .set_file_name("keyform-shortcuts.xml")
                        .save_file()
                    {
                        st.message = std::fs::write(&p, export_xml(b))
                            .err()
                            .map(|e| e.to_string());
                    }
                }
                if ui.button(crate::i18n::tr("Restore All")).clicked() {
                    *b = Bindings::default();
                    st.message = None;
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut group = "";
                egui::Grid::new("shortcut-table")
                    .num_columns(4)
                    .striped(true)
                    .show(ui, |ui| {
                        for c in COMMANDS {
                            if c.group != group {
                                group = c.group;
                                ui.strong(crate::i18n::tr(group));
                                ui.end_row();
                            }
                            ui.label(crate::i18n::tr(c.label));
                            let keys = b.get(c.id);
                            for (slot, k) in keys.iter().enumerate() {
                                let waiting = st.capture == Some((c.id, slot));
                                let text = if waiting {
                                    crate::i18n::tr("Press a key…").to_owned()
                                } else {
                                    k.map(|k| k.label()).unwrap_or_else(|| "—".into())
                                };
                                let r = ui.add_enabled(
                                    !c.fixed,
                                    egui::Button::new(text).selected(waiting),
                                );
                                crate::icons::accessible_name(
                                    &r,
                                    &format!("{} key {}", c.label, slot + 1),
                                );
                                if r.on_hover_text(crate::i18n::tr("Set Key")).clicked() {
                                    st.capture = Some((c.id, slot));
                                    st.message = None;
                                }
                            }
                            if ui
                                .add_enabled(
                                    !c.fixed && keys.iter().any(Option::is_some),
                                    egui::Button::new(crate::i18n::tr("Delete Shortcut")),
                                )
                                .clicked()
                            {
                                b.set(c.id, 0, None);
                                b.set(c.id, 1, None);
                            }
                            ui.end_row();
                        }
                    });
            });
        });
    if !open {
        st.open = false;
        st.capture = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse_and_are_unique() {
        let b = Bindings::default();
        for c in COMMANDS {
            for k in c.keys {
                assert!(Combo::parse(k).is_some(), "{} {k}", c.id);
            }
        }
        assert!(!b.has_conflicts(), "default bindings overlap");
        assert_eq!(b.text("file.save"), "Ctrl+S");
        assert_eq!(b.text("modeling.level1"), "1");
        assert_eq!(b.text("show.zoom_in"), "Ctrl+Alt+Shift+Left");
    }

    /// Bindings match exact modifiers: Ctrl+S is Save, not Save as, and a
    /// rebound command answers to its new key.
    #[test]
    fn key_events_hit_exact_bindings() {
        let ctx = egui::Context::default();
        let press = |key: Key, modifiers: Modifiers| egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        };
        let mut b = Bindings::default();
        let mut hits = Vec::new();
        let mut out = ctx.run_ui(press(Key::S, Modifiers::COMMAND), |ui| {
            hits.push(ui.ctx().input_mut(|i| b.hit(i, "file.save_as")));
            hits.push(ui.ctx().input_mut(|i| b.hit(i, "file.save")));
        });
        out.textures_delta.clear();
        assert_eq!(hits, [false, true]);
        b.set("file.save", 0, Combo::parse("Ctrl+K"));
        let mut hits = Vec::new();
        let mut out = ctx.run_ui(press(Key::K, Modifiers::COMMAND), |ui| {
            hits.push(ui.ctx().input_mut(|i| b.hit(i, "file.save")));
        });
        out.textures_delta.clear();
        assert_eq!(hits, [true]);
    }

    #[test]
    fn set_export_import() {
        let mut b = Bindings::default();
        b.set("file.save", 1, Combo::parse("Ctrl+Alt+W"));
        b.set("edit.search", 0, Combo::parse("Ctrl+Shift+F"));
        let xml = export_xml(&b);
        let (back, unknown) = import_xml(&xml).unwrap();
        assert!(unknown.is_empty());
        assert_eq!(back, b);
        // Unknown command IDs are skipped and reported.
        let extra = xml.replacen(
            "    </group>",
            "      <item command=\"no.such\" key1=\"Ctrl+Q\"/>\n    </group>",
            1,
        );
        let (_, unknown) = import_xml(&extra).unwrap();
        assert_eq!(unknown, vec!["no.such".to_owned()]);
        // A command left out of a file keeps its default.
        let (none, _) =
            import_xml("<KeyformShortcuts version=\"1\"><mode name=\"main\"/></KeyformShortcuts>")
                .unwrap();
        assert_eq!(none.text("edit.search"), "Ctrl+F");
        // Back to the default: the change disappears.
        b.set("file.save", 1, None);
        assert!(!b.changed.contains_key("file.save"));
        assert!(import_xml("<KeyformShortcuts version=\"9\"/>").is_err());
    }
}
