//! Main window: menu bar, toolbar, dock area, status bar.
//! spec: 50-ui/main-window.md, 50-ui/menus.md, 50-ui/shortcuts.md

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};
use eframe::egui_wgpu;
use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};

use crate::canvas::{self, CanvasStatus, ViewGpu};
use crate::document::Document;
use crate::log::Log;
use crate::palettes;
use crate::theme::{self, Tokens};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
}

const RECENT_KEY: &str = "keyform.recent_files";

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
        let mut app = Self {
            dock: default_layout(Vec::new()),
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
                    if let Some(i) = self.active {
                        self.close(i);
                    }
                }
                if ui
                    .add_enabled(!self.slots.is_empty(), egui::Button::new("Close all"))
                    .clicked()
                {
                    while !self.slots.is_empty() {
                        self.close(0);
                    }
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
                ui.add_enabled(false, egui::Button::new("Undo").shortcut_text("Ctrl+Z"));
                ui.add_enabled(false, egui::Button::new("Redo").shortcut_text("Ctrl+Y"));
                ui.separator();
                if ui
                    .add_enabled(
                        self.active.is_some(),
                        egui::Button::new("Deselect").shortcut_text("Escape"),
                    )
                    .clicked()
                {
                    if let Some(d) = self.active_doc() {
                        d.selection = None;
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
                let doc = &mut self.slots[i].doc;
                ui.checkbox(&mut doc.physics_enabled, "Physics");
                if ui.button("Reset parameters").clicked() {
                    doc.reset_parameters();
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

    fn shortcuts(&mut self, ctx: &egui::Context) {
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
            if let Some(i) = self.active {
                self.close(i);
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
                .map_or("?".into(), |s| s.doc.title.clone())
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
                    Some(d) => {
                        palettes::parameter_ui(ui, d, &tokens, &mut self.palettes.param_filter)
                    }
                    None => {
                        ui.weak("No model");
                    }
                }
            }
            Tab::Inspector => match self.active_doc() {
                Some(d) => palettes::inspector_ui(ui, d),
                None => {
                    ui.weak("No model");
                }
            },
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
            Tab::ModelSettings => match self.active_doc() {
                Some(d) => palettes::model_settings_ui(ui, d),
                None => {
                    ui.weak("No model");
                }
            },
        }
    }

    fn is_closeable(&self, tab: &Tab) -> bool {
        !matches!(tab, Tab::Welcome)
    }

    fn on_close(&mut self, tab: &mut Tab) -> OnCloseResponse {
        if let Tab::View(uid) = *tab {
            self.closed.push(uid);
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

        // Playback clock.
        let now = ctx.input(|i| i.time);
        let dt = self.last_time.map_or(0.0, |t| (now - t).clamp(0.0, 0.1)) as f32;
        self.last_time = Some(now);
        let mut animating = false;
        for s in &mut self.slots {
            s.doc.tick(dt, &mut self.log);
            animating |= s.doc.is_animating();
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
            if let Some(i) = self.slots.iter().position(|s| s.uid == uid) {
                self.close(i);
            }
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
    }
}
