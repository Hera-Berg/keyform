//! Main window: menu bar, toolbar, dock area, status bar.
//! spec: 50-ui/main-window.md, 50-ui/menus.md, 50-ui/shortcuts.md

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};
use eframe::egui_wgpu;
use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};

use crate::canvas::{self, CanvasStatus, ViewGpu};
use crate::document::{Document, PlaybackOptions};
use crate::log::Log;
use crate::palettes;
use crate::theme::{self, Tokens};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Tab {
    /// A document view (by document uid).
    View(u64),
    /// Contents placeholder when no document is open.
    Welcome,
    Parts,
    Deformer,
    Parameter,
    Inspector,
    Log,
    Project,
    ModelSettings,
}

impl Tab {
    const PALETTES: [(Tab, &'static str); 7] = [
        (Tab::Parts, "Parts"),
        (Tab::Deformer, "Deformer"),
        (Tab::Parameter, "Parameter"),
        (Tab::Inspector, "Inspector"),
        (Tab::Log, "Log"),
        (Tab::Project, "Project"),
        (Tab::ModelSettings, "Model Settings"),
    ];

    fn name(self) -> &'static str {
        Self::PALETTES
            .iter()
            .find(|(t, _)| *t == self)
            .map_or("", |(_, n)| n)
    }
}

/// Default layout (main-window.md §4, Modeling, with the viewer's Model
/// Settings palette in the Tool Details slot):
/// `H[ V[{Parts|Project},{Deformer|Log}], V[{Model Settings},{Inspector},{Parameter}], VIEW ]`
pub fn default_layout(views: Vec<Tab>) -> DockState<Tab> {
    let contents = if views.is_empty() {
        vec![Tab::Welcome]
    } else {
        views
    };
    let mut dock = DockState::new(contents);
    let s = dock.main_surface_mut();
    let [_view, mid] = s.split_left(NodeIndex::root(), 0.56, vec![Tab::ModelSettings]);
    let [mid, left] = s.split_left(mid, 0.5, vec![Tab::Parts, Tab::Project]);
    let [_settings, below] = s.split_below(mid, 0.28, vec![Tab::Inspector]);
    let [_inspector, _params] = s.split_below(below, 0.3, vec![Tab::Parameter]);
    let [_parts, _deformer] = s.split_below(left, 0.55, vec![Tab::Deformer, Tab::Log]);
    dock
}

pub struct Slot {
    pub uid: u64,
    pub doc: Document,
    pub gpu: ViewGpu,
}

#[derive(Default)]
pub struct PaletteState {
    pub settings: crate::viewer_settings::SettingsState,
    pub parts_filter: String,
    pub param_filter: String,
    pub show_deformers: bool,
}

/// Debug aid: save a screenshot after N frames and quit (`--screenshot`).
pub struct ScreenshotRequest {
    pub path: PathBuf,
    pub after_frames: u32,
}

pub struct KeyformApp {
    dock: DockState<Tab>,
    slots: Vec<Slot>,
    active: Option<usize>,
    next_uid: u64,
    log: Log,
    tokens: Tokens,
    dark: bool,
    rs: Option<egui_wgpu::RenderState>,
    status: CanvasStatus,
    palettes: PaletteState,
    recent: Vec<PathBuf>,
    screenshot: Option<ScreenshotRequest>,
    frames: u32,
    last_time: Option<f64>,
    /// Parameter values given on the command line, applied after opening.
    pub initial_values: Vec<(String, f32)>,
    /// Motion to start on the first document (`--play N`).
    pub initial_motion: Option<usize>,
    /// Documents (by uid) waiting to be closed; modified ones ask to save
    /// first (main-window.md §5).
    closing: Vec<u64>,
    /// Quit once `closing` is done.
    quit_after_closing: bool,
    /// Set when every document was dealt with: let the window close.
    allow_quit: bool,
    window_title: String,
    /// Animation menu toggles (viewer.md §3.7), shared by all documents.
    playback: PlaybackOptions,
}

const RECENT_KEY: &str = "keyform.recent_files";
const PLAYBACK_KEY: &str = "keyform.playback";
/// Dock layout of the last session (main-window.md §4: layouts persist).
const LAYOUT_KEY: &str = "keyform.dock_layout";

/// The layout as stored: document views become the contents placeholder.
fn storable_layout(dock: &DockState<Tab>) -> DockState<Tab> {
    let mut contents = false;
    dock.filter_map_tabs(|t| match t {
        Tab::View(_) | Tab::Welcome => {
            (!std::mem::replace(&mut contents, true)).then_some(Tab::Welcome)
        }
        t => Some(*t),
    })
}

/// A stored layout is used only if it has the contents box and no palette
/// twice.
fn usable_layout(dock: &DockState<Tab>) -> bool {
    let mut seen = Vec::new();
    for (_, t) in dock.iter_all_tabs() {
        if matches!(t, Tab::View(_)) || seen.contains(t) {
            return false;
        }
        seen.push(*t);
    }
    seen.contains(&Tab::Welcome)
}

impl KeyformApp {
    pub fn new(
        cc: &eframe::CreationContext,
        files: Vec<PathBuf>,
        screenshot: Option<ScreenshotRequest>,
    ) -> Self {
        let tokens = theme::apply(&cc.egui_ctx, true);
        let cjk = crate::fonts::install(&cc.egui_ctx);
        let recent = cc
            .storage
            .and_then(|s| s.get_string(RECENT_KEY))
            .map(|s| s.lines().map(PathBuf::from).collect())
            .unwrap_or_default();
        let dock = cc
            .storage
            .and_then(|s| eframe::get_value::<DockState<Tab>>(s, LAYOUT_KEY))
            .filter(usable_layout)
            .unwrap_or_else(|| default_layout(Vec::new()));
        let playback = cc
            .storage
            .and_then(|s| eframe::get_value::<PlaybackOptions>(s, PLAYBACK_KEY))
            .unwrap_or_default();
        let mut app = Self {
            playback,
            dock,
            slots: Vec::new(),
            active: None,
            next_uid: 1,
            log: Log::default(),
            tokens,
            dark: true,
            rs: cc.wgpu_render_state.clone(),
            status: CanvasStatus::default(),
            palettes: PaletteState::default(),
            recent,
            screenshot,
            frames: 0,
            last_time: None,
            initial_values: Vec::new(),
            initial_motion: None,
            closing: Vec::new(),
            quit_after_closing: false,
            allow_quit: false,
            window_title: String::new(),
        };
        match cjk {
            Some(f) => app.log.info(format!("CJK font: {f}")),
            None => app
                .log
                .warn("No CJK font found; Japanese, Chinese and Korean text may not display."),
        }
        if app.rs.is_none() {
            app.log
                .error("No GPU device: the model view is unavailable.");
        }
        for f in files {
            app.open(&f);
        }
        app
    }

    pub fn open(&mut self, path: &Path) {
        // Re-opening an open file just activates it.
        if let Some(i) = self.slots.iter().position(|s| s.doc.path == path) {
            self.activate(i);
            return;
        }
        match Document::open(path, &mut self.log) {
            Ok(doc) => {
                let uid = self.next_uid;
                self.next_uid += 1;
                self.slots.push(Slot {
                    uid,
                    doc,
                    gpu: ViewGpu::default(),
                });
                self.add_view_tab(Tab::View(uid));
                self.activate(self.slots.len() - 1);
                self.recent.retain(|p| p != path);
                self.recent.insert(0, path.to_path_buf());
                self.recent.truncate(10);
            }
            Err(e) => self.log.error(e),
        }
    }

    fn add_view_tab(&mut self, tab: Tab) {
        let contents = self
            .dock
            .find_tab_from(|t| matches!(t, Tab::View(_) | Tab::Welcome));
        match contents {
            Some(path) => {
                if let Ok(leaf) = self.dock.leaf_mut(path.node_path()) {
                    leaf.append_tab(tab);
                }
                if let Some(w) = self.dock.find_tab(&Tab::Welcome) {
                    self.dock.remove_tab(w);
                }
                if let Some(p) = self.dock.find_tab(&tab) {
                    let _ = self.dock.set_active_tab(p);
                }
            }
            None => self.dock.push_to_first_leaf(tab),
        }
    }

    fn activate(&mut self, i: usize) {
        self.active = Some(i);
        if let Some(p) = self
            .slots
            .get(i)
            .and_then(|s| self.dock.find_tab(&Tab::View(s.uid)))
        {
            let _ = self.dock.set_active_tab(p);
        }
    }

    fn close(&mut self, i: usize) {
        if i >= self.slots.len() {
            return;
        }
        let uid = self.slots[i].uid;
        if let Some(p) = self.dock.find_tab(&Tab::View(uid)) {
            self.dock.remove_tab(p);
        }
        self.slots.remove(i);
        self.active = if self.slots.is_empty() {
            None
        } else {
            Some(i.min(self.slots.len() - 1))
        };
        if self.slots.is_empty() && self.dock.find_tab(&Tab::Welcome).is_none() {
            self.dock = default_layout(Vec::new());
        }
    }

    /// Closes a document, asking to save it first if it was modified.
    fn request_close(&mut self, uid: u64) {
        if !self.closing.contains(&uid) {
            self.closing.push(uid);
        }
    }

    /// Works through `closing`: unmodified documents close at once, the
    /// first modified one shows the save prompt.
    fn closing_step(&mut self, ctx: &egui::Context) {
        while let Some(&uid) = self.closing.first() {
            let Some(i) = self.slots.iter().position(|s| s.uid == uid) else {
                self.closing.remove(0);
                continue;
            };
            if !self.slots[i].doc.is_modified() {
                self.closing.remove(0);
                self.close(i);
                continue;
            }
            self.activate(i);
            let title = self.slots[i].doc.title.clone();
            let mut choice = None;
            let modal = egui::Modal::new(egui::Id::new("save-before-close")).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(format!("Do you want to save file for {title} ?"));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Yes").clicked() {
                        choice = Some(Some(true));
                    }
                    if ui.button("No").clicked() {
                        choice = Some(Some(false));
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(None);
                    }
                });
            });
            if modal.should_close() && choice.is_none() {
                choice = Some(None);
            }
            match choice {
                Some(Some(save)) => {
                    if save && !self.save_slot(i) {
                        self.cancel_closing();
                        return;
                    }
                    self.closing.remove(0);
                    self.close(i);
                }
                Some(None) => self.cancel_closing(),
                None => {}
            }
            return;
        }
        if self.quit_after_closing {
            self.quit_after_closing = false;
            self.allow_quit = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn cancel_closing(&mut self) {
        self.closing.clear();
        self.quit_after_closing = false;
    }

    fn open_dialog(&mut self) {
        let files = rfd::FileDialog::new()
            .add_filter("Models", &["cmo3", "moc3", "json"])
            .add_filter("Model project (*.cmo3)", &["cmo3"])
            .add_filter("Runtime model (*.moc3, *.model3.json)", &["moc3", "json"])
            .pick_files();
        for f in files.unwrap_or_default() {
            self.open(&f);
        }
    }

    fn reset_layout(&mut self) {
        let views = self.slots.iter().map(|s| Tab::View(s.uid)).collect();
        self.dock = default_layout(views);
    }

    fn toggle_palette(&mut self, tab: Tab) {
        match self.dock.find_tab(&tab) {
            Some(p) => {
                self.dock.remove_tab(p);
            }
            None => self.dock.push_to_focused_leaf(tab),
        }
    }

    fn active_doc(&mut self) -> Option<&mut Document> {
        self.active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc)
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui
                    .add(egui::Button::new("Open").shortcut_text("Ctrl+O"))
                    .clicked()
                {
                    self.open_dialog();
                }
                ui.menu_button("Recent Files", |ui| {
                    let recent = self.recent.clone();
                    if recent.is_empty() {
                        ui.weak("(none)");
                    }
                    for p in recent {
                        if ui.button(p.display().to_string()).clicked() {
                            self.open(&p);
                        }
                    }
                });
                ui.separator();
                if ui
                    .add_enabled(
                        self.active.is_some(),
                        egui::Button::new("Close").shortcut_text("Ctrl+W"),
                    )
                    .clicked()
                {
                    if let Some(uid) = self.active.and_then(|i| self.slots.get(i)).map(|s| s.uid) {
                        self.request_close(uid);
                    }
                }
                if ui
                    .add_enabled(!self.slots.is_empty(), egui::Button::new("Close all"))
                    .clicked()
                {
                    for uid in self.slots.iter().map(|s| s.uid).collect::<Vec<_>>() {
                        self.request_close(uid);
                    }
                }
                ui.separator();
                let doc = self.active.and_then(|i| self.slots.get(i)).map(|s| &s.doc);
                let project = doc.is_some_and(|d| d.editable());
                let (label, can_save) = match doc {
                    Some(d) if !project => ("Save model3.json", d.can_save_settings()),
                    Some(_) => ("Save", true),
                    None => ("Save", false),
                };
                if ui
                    .add_enabled(can_save, egui::Button::new(label).shortcut_text("Ctrl+S"))
                    .clicked()
                {
                    self.save_active();
                }
                if ui
                    .add_enabled(
                        project,
                        egui::Button::new("Save as…").shortcut_text("Ctrl+Shift+S"),
                    )
                    .clicked()
                {
                    self.save_as_active();
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Exit").shortcut_text("Alt+F4"))
                    .clicked()
                {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                let doc = self.active.and_then(|i| self.slots.get(i)).map(|s| &s.doc);
                let undo = doc.and_then(|d| d.history.undo_name()).map(str::to_owned);
                let redo = doc.and_then(|d| d.history.redo_name()).map(str::to_owned);
                let r = ui.add_enabled(
                    undo.is_some(),
                    egui::Button::new("Undo").shortcut_text("Ctrl+Z"),
                );
                if r.clicked() {
                    self.undo_active(false);
                }
                if let Some(n) = undo {
                    r.on_hover_text(n);
                }
                let r = ui.add_enabled(
                    redo.is_some(),
                    egui::Button::new("Redo").shortcut_text("Ctrl+Y"),
                );
                if r.clicked() {
                    self.undo_active(true);
                }
                if let Some(n) = redo {
                    r.on_hover_text(n);
                }
                ui.separator();
                if ui
                    .add_enabled(
                        self.active.is_some(),
                        egui::Button::new("Deselect").shortcut_text("Escape"),
                    )
                    .clicked()
                {
                    if let Some(d) = self.active_doc() {
                        d.select(None);
                    }
                }
            });
            ui.menu_button("Show", |ui| {
                let has = self.active.is_some();
                if ui
                    .add_enabled(has, egui::Button::new("Fit to View"))
                    .clicked()
                {
                    if let Some(d) = self.active_doc() {
                        d.view.fit_pending = true;
                    }
                }
                if ui
                    .add_enabled(
                        has,
                        egui::Button::new("Zoom In").shortcut_text("Ctrl+Alt+Shift+Left"),
                    )
                    .clicked()
                {
                    if let Some(d) = self.active_doc() {
                        d.view.zoom *= 1.25;
                    }
                }
                if ui
                    .add_enabled(
                        has,
                        egui::Button::new("Zoom Out").shortcut_text("Ctrl+Alt+Shift+Right"),
                    )
                    .clicked()
                {
                    if let Some(d) = self.active_doc() {
                        d.view.zoom /= 1.25;
                    }
                }
                if ui
                    .add_enabled(has, egui::Button::new("Reset Rotation"))
                    .clicked()
                {
                    if let Some(d) = self.active_doc() {
                        d.view.rotation = 0.0;
                    }
                }
                let mut mirror = self
                    .active
                    .and_then(|i| self.slots.get(i))
                    .is_some_and(|s| s.doc.view.mirror);
                if ui
                    .add_enabled(
                        has,
                        egui::Checkbox::new(&mut mirror, "Reflect Horizontally"),
                    )
                    .changed()
                {
                    if let Some(d) = self.active_doc() {
                        d.view.mirror = mirror;
                    }
                }
            });
            ui.menu_button("Animation", |ui| self.animation_menu(ui));
            ui.menu_button("Window", |ui| {
                for (tab, name) in Tab::PALETTES {
                    let mut shown = self.dock.find_tab(&tab).is_some();
                    if ui.checkbox(&mut shown, name).changed() {
                        self.toggle_palette(tab);
                    }
                }
                ui.separator();
                if ui.button("Reset Current Layout").clicked() {
                    self.reset_layout();
                }
                ui.separator();
                let mut dark = self.dark;
                if ui.checkbox(&mut dark, "Dark Theme").changed() {
                    self.dark = dark;
                    self.tokens = theme::apply(ui.ctx(), dark);
                }
            });
            ui.menu_button("Help", |ui| {
                ui.label(format!("Keyform {}", env!("CARGO_PKG_VERSION")));
                ui.weak("Clean-room 2D-mesh rigging and animation editor.");
            });
        });
    }

    /// Playback toggles. spec: 70-advanced/viewer.md §3.7
    fn animation_menu(&mut self, ui: &mut egui::Ui) {
        let o = &mut self.playback;
        ui.checkbox(&mut o.expression, "Apply Expression");
        ui.checkbox(&mut o.eye_blink, "Automatic Eye-Blinking");
        ui.checkbox(&mut o.physics, "Apply Physics");
        ui.checkbox(&mut o.pose, "Enable Pose Switching");
        ui.add_enabled(
            false,
            egui::Checkbox::new(&mut o.motion_sync, "Motion-sync"),
        )
        .on_disabled_hover_text("Motion-sync lip sync is not implemented yet");
        ui.checkbox(&mut o.idle_motion, "Idle Motion");
        ui.checkbox(&mut o.track_cursor, "Track cursor")
            .on_hover_text("Drag over the view: left button = primary, right = secondary target");
        ui.checkbox(&mut o.breath, "Breath");
        ui.add_enabled(
            false,
            egui::Checkbox::new(&mut o.parameter_repeat, "Parameter repeat"),
        )
        .on_disabled_hover_text("Not implemented yet");
        ui.separator();
        if ui
            .checkbox(&mut o.old_playback, "Use old playback methods")
            .changed()
            && o.old_playback
        {
            self.log.info(
                "The motion degradation observed in the SDK is simulated when the motion \
                 includes curves with significantly adjusted Bezier handles.",
            );
        }
        let o = &mut self.playback;
        if ui
            .checkbox(&mut o.old_fading, "Use Old Fading Method")
            .changed()
            && o.old_fading
        {
            self.log.warn(
                "The old fading method may result in unnatural expressions depending on the \
                 parameter settings.",
            );
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("workspace")
                .selected_text("Viewer")
                .width(140.0)
                .show_ui(ui, |ui| {
                    let _ = ui.selectable_label(true, "Viewer");
                    ui.add_enabled(false, egui::Button::selectable(false, "Modeling"));
                    ui.add_enabled(false, egui::Button::selectable(false, "Animation"));
                });
            ui.separator();
            ui.weak("Edit Level:");
            for l in ["1", "2", "3"] {
                ui.add_enabled(false, egui::Button::new(l));
            }
            ui.separator();
            if let Some(i) = self.active {
                if ui.button("Reset parameters").clicked() {
                    self.slots[i].doc.reset_parameters();
                }
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&self.log.status).color(self.tokens.secondary_text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.active.is_some() {
                    ui.label(format!("{:.0}%", self.status.zoom * 100.0));
                    if self.status.rotation != 0.0 {
                        ui.label(format!("{:.0}°", self.status.rotation));
                    }
                    if let Some(c) = self.status.cursor {
                        ui.label(format!("x {:.0}  y {:.0}", c[0], c[1]));
                    }
                }
            });
        });
    }

    /// Save: a project is written back to its `.cmo3`; for a runtime model
    /// the viewer binds Save to writing model3.json (shortcuts.md).
    fn save_active(&mut self) {
        if let Some(i) = self.active {
            self.save_slot(i);
        }
    }

    /// Saves document `i`; returns false if that failed.
    fn save_slot(&mut self, i: usize) -> bool {
        let Some(s) = self.slots.get_mut(i) else {
            return false;
        };
        if !s.doc.editable() {
            crate::viewer_settings::save(&mut s.doc, &mut self.log);
            return true;
        }
        match s.doc.save_project(None) {
            Ok(p) => {
                self.log.info(format!("saved {}", p.display()));
                true
            }
            Err(e) => {
                self.log.error(format!("save failed: {e}"));
                false
            }
        }
    }

    fn save_as_active(&mut self) {
        let Some(s) = self.active.and_then(|i| self.slots.get_mut(i)) else {
            return;
        };
        if !s.doc.editable() {
            return;
        }
        let mut dialog = rfd::FileDialog::new()
            .add_filter("Model project (*.cmo3)", &["cmo3"])
            .set_file_name(s.doc.title.clone());
        if let Some(dir) = s.doc.path.parent() {
            dialog = dialog.set_directory(dir);
        }
        let Some(mut path) = dialog.save_file() else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("cmo3");
        }
        match s.doc.save_project(Some(&path)) {
            Ok(p) => {
                self.log.info(format!("saved {}", p.display()));
                self.recent.retain(|r| r != &p);
                self.recent.insert(0, p);
                self.recent.truncate(10);
            }
            Err(e) => self.log.error(format!("save failed: {e}")),
        }
    }

    fn undo_active(&mut self, redo: bool) {
        let Some(d) = self.active_doc() else { return };
        let name = if redo { d.redo() } else { d.undo() };
        if let Some(n) = name {
            self.log.status = format!("{}: {n}", if redo { "Redo" } else { "Undo" });
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut as K, Modifiers as M};
        let shift_cmd = M::COMMAND | M::SHIFT;
        // Shift variants first: consume_shortcut ignores extra Shift.
        let (save_as, save) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&K::new(shift_cmd, Key::S)),
                i.consume_shortcut(&K::new(M::COMMAND, Key::S)),
            )
        });
        if save_as {
            self.save_as_active();
        } else if save {
            self.save_active();
        }
        // Text fields keep Ctrl+Z / Ctrl+Y for their own editing.
        if !ctx.egui_wants_keyboard_input() {
            let (redo, undo) = ctx.input_mut(|i| {
                (
                    i.consume_shortcut(&K::new(M::COMMAND, Key::Y))
                        || i.consume_shortcut(&K::new(shift_cmd, Key::Z)),
                    i.consume_shortcut(&K::new(M::COMMAND, Key::Z)),
                )
            });
            if redo {
                self.undo_active(true);
            } else if undo {
                self.undo_active(false);
            }
        }
        let (open, close) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND,
                    egui::Key::O,
                )),
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND,
                    egui::Key::W,
                )),
            )
        });
        if open {
            self.open_dialog();
        }
        if close {
            if let Some(uid) = self.active.and_then(|i| self.slots.get(i)).map(|s| s.uid) {
                self.request_close(uid);
            }
        }
        // Drag & drop (viewer.md §2: "drop a moc3 or model3.json").
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        for p in dropped {
            self.open(&p);
        }
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(req) = &self.screenshot else { return };
        self.frames += 1;
        if self.frames == req.after_frames {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = image {
            let rgba = keyform_formats::image::RgbaImage {
                width: img.size[0] as u32,
                height: img.size[1] as u32,
                data: img.pixels.iter().flat_map(|c| c.to_array()).collect(),
            };
            match rgba
                .encode_png()
                .map_err(|e| e.to_string())
                .and_then(|png| std::fs::write(&req.path, png).map_err(|e| e.to_string()))
            {
                Ok(()) => eprintln!("screenshot saved to {}", req.path.display()),
                Err(e) => eprintln!("screenshot failed: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ctx.request_repaint();
        }
    }
}

struct Viewer<'a> {
    slots: &'a mut Vec<Slot>,
    active: &'a mut Option<usize>,
    log: &'a mut Log,
    tokens: &'a Tokens,
    rs: Option<&'a egui_wgpu::RenderState>,
    status: &'a mut CanvasStatus,
    palettes: &'a mut PaletteState,
    closed: &'a mut Vec<u64>,
    open_request: &'a mut bool,
}

impl Viewer<'_> {
    fn active_doc(&mut self) -> Option<&mut Document> {
        self.active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc)
    }
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        match *tab {
            Tab::View(uid) => self
                .slots
                .iter()
                .find(|s| s.uid == uid)
                .map_or("?".into(), |s| {
                    let star = if s.doc.is_modified() { "*" } else { "" };
                    format!("{}{star}", s.doc.title)
                })
                .into(),
            Tab::Welcome => "Home".into(),
            t => t.name().into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        match *tab {
            Tab::View(uid) => {
                let Some(i) = self.slots.iter().position(|s| s.uid == uid) else {
                    return;
                };
                let slot = &mut self.slots[i];
                let r = canvas::canvas_ui(
                    ui,
                    &mut slot.doc,
                    &mut slot.gpu,
                    self.rs,
                    self.tokens,
                    self.status,
                );
                if r.clicked()
                    || r.dragged()
                    || r.hovered() && ui.input(|i| i.pointer.any_pressed())
                {
                    *self.active = Some(i);
                }
            }
            Tab::Welcome => {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() / 3.0);
                        ui.heading("Keyform");
                        ui.label("Drop a .cmo3, .moc3 or .model3.json here");
                        if ui.button("Open…").clicked() {
                            *self.open_request = true;
                        }
                    });
                });
            }
            Tab::Parts => match self
                .active
                .and_then(|i| self.slots.get_mut(i))
                .map(|s| &mut s.doc)
            {
                Some(d) => {
                    let p = &mut *self.palettes;
                    palettes::parts_ui(ui, d, &mut p.show_deformers, &mut p.parts_filter);
                }
                None => {
                    ui.weak("No model");
                }
            },
            Tab::Deformer => match self.active_doc() {
                Some(d) => palettes::deformer_ui(ui, d),
                None => {
                    ui.weak("No model");
                }
            },
            Tab::Parameter => {
                // Disjoint field borrows: the document and the palette filter.
                let tokens = *self.tokens;
                let doc = self
                    .active
                    .and_then(|i| self.slots.get_mut(i))
                    .map(|s| &mut s.doc);
                match doc {
                    Some(d) => palettes::parameter_ui(
                        ui,
                        d,
                        &tokens,
                        &mut self.palettes.param_filter,
                        self.log,
                    ),
                    None => {
                        ui.weak("No model");
                    }
                }
            }
            Tab::Inspector => {
                let doc = self
                    .active
                    .and_then(|i| self.slots.get_mut(i))
                    .map(|s| &mut s.doc);
                match doc {
                    Some(d) => palettes::inspector_ui(ui, d, self.log),
                    None => {
                        ui.weak("No model");
                    }
                }
            }
            Tab::Log => {
                let tokens = *self.tokens;
                palettes::log_ui(ui, self.log, &tokens);
            }
            Tab::Project => {
                let docs: Vec<&Document> = self.slots.iter().map(|s| &s.doc).collect();
                if let Some(i) = palettes::project_ui(ui, &docs, *self.active) {
                    *self.active = Some(i);
                }
            }
            Tab::ModelSettings => {
                let doc = self
                    .active
                    .and_then(|i| self.slots.get_mut(i))
                    .map(|s| &mut s.doc);
                match doc {
                    Some(d) => crate::viewer_settings::settings_ui(
                        ui,
                        d,
                        &mut self.palettes.settings,
                        self.log,
                    ),
                    None => {
                        ui.weak("No model");
                    }
                }
            }
        }
    }

    fn is_closeable(&self, tab: &Tab) -> bool {
        !matches!(tab, Tab::Welcome)
    }

    fn on_close(&mut self, tab: &mut Tab) -> OnCloseResponse {
        if let Tab::View(uid) = *tab {
            self.closed.push(uid);
            // A modified document's tab stays until the save prompt is answered.
            if self
                .slots
                .iter()
                .any(|s| s.uid == uid && s.doc.is_modified())
            {
                return OnCloseResponse::Ignore;
            }
        }
        OnCloseResponse::Close
    }

    fn on_tab_button(&mut self, tab: &mut Tab, response: &egui::Response) {
        if let (Tab::View(uid), true) = (*tab, response.clicked()) {
            *self.active = self.slots.iter().position(|s| s.uid == uid);
        }
    }

    fn scroll_bars(&self, _tab: &Tab) -> [bool; 2] {
        [false, false]
    }
}

impl eframe::App for KeyformApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        if !self.initial_values.is_empty() {
            for (id, v) in std::mem::take(&mut self.initial_values) {
                for s in &mut self.slots {
                    if let Some(&p) = s.doc.param_index.get(&id) {
                        s.doc.values[p] = v;
                    }
                }
            }
        }

        if let (Some(n), Some(s)) = (self.initial_motion.take(), self.slots.first_mut()) {
            s.doc.play_motion(n);
        }

        // Playback clock.
        let now = ctx.input(|i| i.time);
        let dt = self.last_time.map_or(0.0, |t| (now - t).clamp(0.0, 0.1)) as f32;
        self.last_time = Some(now);
        let mut animating = false;
        for s in &mut self.slots {
            s.doc.tick(dt, &self.playback, &mut self.log);
            animating |= s.doc.is_animating(&self.playback);
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));

        let mut closed = Vec::new();
        let mut open_request = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.tokens.background))
            .show(ui, |ui| {
                let mut viewer = Viewer {
                    slots: &mut self.slots,
                    active: &mut self.active,
                    log: &mut self.log,
                    tokens: &self.tokens,
                    rs: self.rs.as_ref(),
                    status: &mut self.status,
                    palettes: &mut self.palettes,
                    closed: &mut closed,
                    open_request: &mut open_request,
                };
                DockArea::new(&mut self.dock).show_inside(ui, &mut viewer);
            });
        for uid in closed {
            self.request_close(uid);
        }
        // Quitting with modified documents asks about each first.
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_quit
            && self.slots.iter().any(|s| s.doc.is_modified())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            for uid in self.slots.iter().map(|s| s.uid).collect::<Vec<_>>() {
                self.request_close(uid);
            }
            self.quit_after_closing = true;
        }
        self.closing_step(&ctx);
        let title = match self.active.and_then(|i| self.slots.get(i)) {
            Some(s) => format!(
                "{}{} - Keyform",
                s.doc.title,
                if s.doc.is_modified() { "*" } else { "" }
            ),
            None => "Keyform".to_owned(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
        if open_request {
            self.open_dialog();
        }
        if animating {
            ctx.request_repaint();
        }
        self.screenshot_step(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let recent: Vec<String> = self
            .recent
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        storage.set_string(RECENT_KEY, recent.join("\n"));
        eframe::set_value(storage, LAYOUT_KEY, &storable_layout(&self.dock));
        eframe::set_value(storage, PLAYBACK_KEY, &self.playback);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_layout_keeps_contents_box_once() {
        let dock = default_layout(vec![Tab::View(1), Tab::View(2)]);
        assert!(!usable_layout(&dock), "views are not stored");
        let stored = storable_layout(&dock);
        assert!(usable_layout(&stored));
        let welcome = stored
            .iter_all_tabs()
            .filter(|(_, t)| **t == Tab::Welcome)
            .count();
        assert_eq!(welcome, 1);
        let palettes = stored.iter_all_tabs().count() - 1;
        assert_eq!(palettes, Tab::PALETTES.len());
    }
}
