//! Main window: menu bar, toolbar, dock area, status bar.
//! spec: 50-ui/main-window.md, 50-ui/menus.md, 50-ui/shortcuts.md

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};
use eframe::egui_wgpu;
use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};

use crate::canvas::{self, CanvasStatus, ViewGpu};
use crate::document::{Document, Mode, PlaybackOptions};
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
    /// Object to select in the first document (`--select ID`).
    pub initial_select: Option<String>,
    /// Start mesh editing on that object (`--mesh-edit`).
    pub initial_mesh_edit: bool,
    /// Then open the Automatic Mesh generator (`--auto-mesh`).
    pub initial_auto_mesh: bool,
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
    /// User settings (persisted).
    settings: Settings,
    /// The "Issues on Mask Settings" dialog is open.
    mask_dialog: bool,
    export_image: crate::export_image::ExportImageState,
    create_deformer: crate::create_deformer::CreateDeformerState,
    auto_mesh: crate::auto_mesh_dialog::AutoMeshDialog,
    /// A PSD waiting for the import dialogs.
    pending_psd: Option<crate::psd_dialog::PendingPsd>,
    param_dialog: crate::param_dialog::ParamDialog,
}

/// User settings. TODO: an Environment Settings dialog; for now the Show
/// menu holds the switches.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Settings ▸ Notification ▸ "Always show warnings regarding masks".
    pub mask_warnings: bool,
    /// The mask dialog's "do not show again" (export time).
    pub mask_dialog_suppressed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mask_warnings: true,
            mask_dialog_suppressed: false,
        }
    }
}

const SETTINGS_KEY: &str = "keyform.settings";
const EXPORT_IMAGE_KEY: &str = "keyform.export_image";

/// `targetVersionNo` of "SDK(N/A)/Latest Cubism" (cmo3.md §2): no mask hint.
const TARGET_LATEST: i32 = 9_000_000;

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
        let settings = cc
            .storage
            .and_then(|s| eframe::get_value::<Settings>(s, SETTINGS_KEY))
            .unwrap_or_default();
        let mut app = Self {
            settings,
            mask_dialog: false,
            create_deformer: Default::default(),
            auto_mesh: Default::default(),
            pending_psd: None,
            param_dialog: Default::default(),
            export_image: cc
                .storage
                .and_then(|s| eframe::get_value(s, EXPORT_IMAGE_KEY))
                .unwrap_or_default(),
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
            initial_select: None,
            initial_mesh_edit: false,
            initial_auto_mesh: false,
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
        // A PSD goes to the import dialogs (psd-import.md §1).
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("psd") || e.eq_ignore_ascii_case("psb"))
        {
            match crate::psd_dialog::read(path) {
                Ok(p) => self.pending_psd = Some(p),
                Err(e) => self.log.error(e),
            }
            return;
        }
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
            .add_filter("Models", &["cmo3", "moc3", "json", "psd"])
            .add_filter("Model project (*.cmo3)", &["cmo3"])
            .add_filter("Photoshop document (*.psd)", &["psd"])
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
                ui.menu_button("Export Image/Video", |ui| {
                    let modeling = self
                        .active
                        .and_then(|i| self.slots.get(i))
                        .is_some_and(|s| s.doc.mode == Mode::Modeling);
                    if ui
                        .add_enabled(modeling, egui::Button::new("Image…"))
                        .on_disabled_hover_text("Needs the Modeling view of a project")
                        .clicked()
                    {
                        self.export_image.open = true;
                    }
                });
                ui.menu_button("Export For Runtime", |ui| {
                    if ui
                        .add_enabled(project, egui::Button::new("Export as moc3 file…"))
                        .clicked()
                    {
                        self.export_runtime_active();
                    }
                });
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
                        egui::Button::new("Delete").shortcut_text("Delete"),
                    )
                    .clicked()
                {
                    self.delete_active();
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
                ui.checkbox(
                    &mut self.settings.mask_warnings,
                    "Always show warnings regarding masks",
                );
                ui.separator();
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
            if self.active_doc().is_some_and(|d| d.mesh_edit.is_some()) {
                ui.menu_button("Mesh Edit", |ui| self.mesh_edit_menu(ui));
            } else {
                ui.menu_button("Modeling", |ui| self.modeling_menu(ui));
            }
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

    /// Modeling menu (subset). spec: 50-ui/menus.md "Modeling"
    fn modeling_menu(&mut self, ui: &mut egui::Ui) {
        use crate::create_deformer::Kind;
        let Some(i) = self.active else {
            ui.weak("No model");
            return;
        };
        let doc = &mut self.slots[i].doc;
        let ok = doc.editable() && doc.mode == Mode::Modeling;
        ui.menu_button("Deformer", |ui| {
            if ui
                .add_enabled(ok, egui::Button::new("Create Warp Deformer…"))
                .clicked()
            {
                self.create_deformer.show(Kind::Warp, doc);
                ui.close();
            }
            if ui
                .add_enabled(ok, egui::Button::new("Create rotation deformer…"))
                .clicked()
            {
                self.create_deformer.show(Kind::Rotation, doc);
                ui.close();
            }
        });
        ui.menu_button("ArtMesh", |ui| {
            let one = ok
                && doc
                    .selection
                    .objects
                    .iter()
                    .filter(|o| matches!(o, crate::document::Selection::ArtMesh(_)))
                    .count()
                    == 1;
            if ui
                .add_enabled(one, egui::Button::new("Mesh edit").shortcut_text("Ctrl+E"))
                .clicked()
            {
                if let Err(e) = doc.start_mesh_edit() {
                    self.log.warn(e);
                }
                ui.close();
            }
            if ui
                .add_enabled(
                    ok,
                    egui::Button::new("Automatic Mesh generator…").shortcut_text("Ctrl+Shift+A"),
                )
                .clicked()
            {
                if let Err(e) = self.auto_mesh.show(doc) {
                    self.log.warn(e);
                }
                ui.close();
            }
        });
        ui.menu_button("Glue", |ui| {
            if ui
                .add_enabled(ok, egui::Button::new("Generate and Register Glue"))
                .on_hover_text("Glue the selected vertex of each of two ArtMeshes")
                .clicked()
            {
                if let Err(e) = doc.glue_selected_vertices() {
                    self.log.warn(e);
                }
                ui.close();
            }
        });
        ui.menu_button("Part", |ui| {
            if ui.add_enabled(ok, egui::Button::new("New Part")).clicked() {
                let parent = match doc.primary() {
                    Some(crate::document::Selection::Part(p)) => Some(p),
                    _ => None,
                };
                let r = doc.try_edit("New Part", |m| {
                    Ok(keyform_edit::objects::create_part(m, "New Part", parent))
                });
                if let Ok(p) = r {
                    doc.select(Some(crate::document::Selection::Part(p)));
                }
                ui.close();
            }
        });
    }

    /// The "Mesh Edit mode" menu (subset). spec: 50-ui/menus.md "Mesh Edit mode"
    fn mesh_edit_menu(&mut self, ui: &mut egui::Ui) {
        use crate::mesh_edit::MeshTool;
        if ui
            .add(egui::Button::new("Finish Mesh editing").shortcut_text("Ctrl+E"))
            .clicked()
        {
            self.toggle_mesh_edit();
            ui.close();
        }
        if ui
            .button("Automatically Generate Meshes(Mesh Editing Mode)")
            .clicked()
        {
            self.open_auto_mesh();
            ui.close();
        }
        ui.separator();
        let Some(d) = self.active_doc() else { return };
        let canvas = [d.model.canvas.width as f32, d.model.canvas.height as f32];
        let tolerance = 1.0 / d.view.zoom.max(1e-6);
        let Some(s) = d.mesh_edit.as_mut() else {
            return;
        };
        let mut notice = None;
        if ui
            .add(egui::Button::new("Auto attach Edge (line)").shortcut_text("Ctrl+R"))
            .on_hover_text("Automatically join vertices")
            .clicked()
        {
            notice = s.auto_connect().err();
            ui.close();
        }
        if ui
            .button("Reset Mesh")
            .on_hover_text(
                "Restores the mesh to the state immediately after the ArtMesh was created.",
            )
            .clicked()
        {
            notice = s.reset_mesh().err();
            ui.close();
        }
        ui.separator();
        if ui.button("Edge swap").clicked() {
            notice = s.swap_edge().err();
            ui.close();
        }
        if ui
            .button("4 Division")
            .on_hover_text("Divide one triangle into four triangles")
            .clicked()
        {
            notice = s.divide4().err();
            ui.close();
        }
        if ui.button("Merge Vertices").clicked() {
            notice = s.merge().err();
            ui.close();
        }
        ui.menu_button("Mirror Edit", |ui| {
            mirror_controls(ui, s, canvas);
        });
        ui.separator();
        if ui
            .add_enabled(
                !s.selected.is_empty(),
                egui::Button::new("Delete Vertex").shortcut_text("Delete"),
            )
            .clicked()
        {
            s.delete_selected(tolerance);
            ui.close();
        }
        ui.menu_button("Subtools", |ui| {
            for (t, name) in [
                (MeshTool::Select, "Select Arrow"),
                (MeshTool::AddPoint, "Add Point Tool"),
                (MeshTool::Erase, "Eraser"),
                (MeshTool::Brush, "Deform brushes"),
            ] {
                if ui.selectable_label(s.tool == t, name).clicked() {
                    s.tool = t;
                    ui.close();
                }
            }
        });
        if let Some(n) = notice {
            self.log.info(n);
        }
    }

    /// Automatic Mesh generator (dialog, or the popup in mesh edit mode).
    fn open_auto_mesh(&mut self) {
        let Some(i) = self.active else { return };
        let doc = &self.slots[i].doc;
        if !doc.editable() || doc.mode != Mode::Modeling {
            return;
        }
        if let Err(e) = self.auto_mesh.show(doc) {
            self.log.warn(e);
        }
    }

    /// Mesh edit [Ctrl+E]: starts or finishes mesh editing.
    fn toggle_mesh_edit(&mut self) {
        let Some(d) = self.active_doc() else { return };
        let r = if d.mesh_edit.is_some() {
            d.finish_mesh_edit()
        } else {
            d.start_mesh_edit().map(|()| None)
        };
        match r {
            Ok(Some(w)) => self.log.warn(w),
            Ok(None) => {}
            Err(e) => self.log.warn(e),
        }
    }

    /// Playback toggles. spec: 70-advanced/viewer.md §3.7
    fn animation_menu(&mut self, ui: &mut egui::Ui) {
        let o = &mut self.playback;
        ui.checkbox(&mut o.expression, "Apply Expression");
        ui.checkbox(&mut o.eye_blink, "Automatic Eye-Blinking");
        ui.checkbox(&mut o.physics, "Apply Physics");
        ui.checkbox(&mut o.pose, "Enable Pose Switching");
        ui.checkbox(&mut o.motion_sync, "Motion-sync")
            .on_hover_text("Lip sync from the playing motion's sound (motionsync3.json)");
        ui.checkbox(&mut o.idle_motion, "Idle Motion");
        ui.checkbox(&mut o.track_cursor, "Track cursor")
            .on_hover_text("Drag over the view: left button = primary, right = secondary target");
        ui.checkbox(&mut o.breath, "Breath");
        ui.checkbox(&mut o.parameter_repeat, "Parameter repeat")
            .on_hover_text("Repeat parameters wrap around instead of stopping at min/max");
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
            // Workspace of the active document: projects edit in Modeling and
            // can preview playback in Viewer; runtime models only play.
            let doc = self
                .active
                .and_then(|i| self.slots.get_mut(i))
                .map(|s| &mut s.doc);
            let (mode, editable) = doc
                .as_ref()
                .map_or((Mode::Viewer, false), |d| (d.mode, d.editable()));
            let label = match mode {
                Mode::Modeling => "Modeling",
                Mode::Viewer => "Viewer",
            };
            let mut chosen = mode;
            egui::ComboBox::from_id_salt("workspace")
                .selected_text(label)
                .width(140.0)
                .show_ui(ui, |ui| {
                    ui.add_enabled_ui(editable, |ui| {
                        ui.selectable_value(&mut chosen, Mode::Modeling, "Modeling");
                    });
                    ui.selectable_value(&mut chosen, Mode::Viewer, "Viewer");
                    ui.add_enabled(false, egui::Button::selectable(false, "Animation"));
                });
            if let Some(d) = doc {
                if chosen != d.mode {
                    if let Err(e) = d.finish_mesh_edit() {
                        self.log.warn(e);
                    } else {
                        d.mode = chosen;
                        d.reset_parameters();
                    }
                }
            }
            ui.separator();
            ui.weak("Edit Level:");
            let doc = self
                .active
                .and_then(|i| self.slots.get_mut(i))
                .map(|s| &mut s.doc);
            let modeling = doc.as_ref().is_some_and(|d| d.mode == Mode::Modeling);
            let current = doc.as_ref().map_or(2, |d| d.edit_level);
            let mut chosen = None;
            for (l, tip) in [
                (1u8, "Edit Level 1 (refine)"),
                (2, "Edit Level 2 (normal)"),
                (3, "Edit Level 3 (rough)"),
            ] {
                let b = egui::Button::selectable(current == l && modeling, l.to_string());
                if ui.add_enabled(modeling, b).on_hover_text(tip).clicked() {
                    chosen = Some(l);
                }
            }
            if let (Some(l), Some(d)) = (chosen, doc) {
                d.set_edit_level(l);
            }
            ui.separator();
            if let Some(s) = self
                .active
                .and_then(|i| self.slots.get_mut(i))
                .and_then(|s| s.doc.mesh_edit.as_mut())
            {
                use crate::mesh_edit::MeshTool;
                ui.weak("Mesh:");
                for (t, name, tip) in [
                    (MeshTool::Select, "Select", "Select and move vertices"),
                    (
                        MeshTool::AddPoint,
                        "Add Point",
                        "Add a vertex; on an edge it splits the edge",
                    ),
                    (
                        MeshTool::Erase,
                        "Eraser",
                        "Delete vertices and edges (Alt: edges only)",
                    ),
                    (
                        MeshTool::Brush,
                        "Brush",
                        "Drag the vertices under a soft brush",
                    ),
                ] {
                    if ui
                        .add(egui::Button::selectable(s.tool == t, name))
                        .on_hover_text(tip)
                        .clicked()
                    {
                        s.tool = t;
                    }
                }
                if s.tool == MeshTool::Brush {
                    brush_controls(ui, &mut s.prefs);
                }
                if ui
                    .button("Connect")
                    .on_hover_text("Automatically join vertices (Ctrl+R)")
                    .clicked()
                {
                    if let Err(e) = s.auto_connect() {
                        self.log.warn(e);
                    }
                }
                ui.checkbox(&mut s.prefs.protect_exterior, "Protect Edge exterior");
                ui.checkbox(&mut s.prefs.reconstruct, "Mesh reconstruction");
                ui.checkbox(&mut s.prefs.mirror_on, "Mirror")
                    .on_hover_text("Mirror Edit: edits are mirrored across the axis");
                if s.prefs.mirror_on {
                    ui.weak("Mirror Editing in Progress");
                }
                if s.unresolved > 0 {
                    ui.colored_label(
                        self.tokens.warning,
                        format!("{} crossing edges", s.unresolved),
                    );
                }
                if ui
                    .button("Finish")
                    .on_hover_text("Finish Mesh editing (Ctrl+E)")
                    .clicked()
                {
                    self.toggle_mesh_edit();
                }
                ui.separator();
            }
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

    /// File ▸ Export For Runtime: writes the runtime set into a chosen
    /// folder, then shows the mask check if it found problems (§21).
    fn export_runtime_active(&mut self) {
        let Some(s) = self.active.and_then(|i| self.slots.get(i)) else {
            return;
        };
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = s.doc.path.parent() {
            dialog = dialog.set_directory(dir);
        }
        let Some(dir) = dialog.pick_folder() else {
            return;
        };
        match s.doc.export_runtime(&dir) {
            Ok((files, warnings)) => {
                for w in warnings {
                    self.log.warn(w);
                }
                self.log.info(format!(
                    "exported {} files to {}",
                    files.len(),
                    dir.display()
                ));
                if !s.doc.mask_issues.is_empty() && !self.settings.mask_dialog_suppressed {
                    self.mask_dialog = true;
                }
            }
            Err(e) => self.log.error(format!("export failed: {e}")),
        }
    }

    /// The "Issues on Mask Settings" dialog. spec: 50-ui/dialogs.md §21
    fn mask_dialog_ui(&mut self, ctx: &egui::Context) {
        use keyform_edit::checks::{MASK_CONSEQUENCES, MASK_INTRO, MASK_MANUAL, MASK_TITLE};
        if !self.mask_dialog {
            return;
        }
        let Some(doc) = self.active.and_then(|i| self.slots.get(i)).map(|s| &s.doc) else {
            self.mask_dialog = false;
            return;
        };
        let lists = doc.mask_issues.describe(&doc.model);
        let mut open = true;
        let mut ok = false;
        egui::Window::new(MASK_TITLE)
            .collapsible(false)
            .resizable(true)
            .default_width(460.0)
            .open(&mut open)
            .show(ctx, |ui| {
                for t in MASK_INTRO {
                    ui.label(t);
                }
                for t in MASK_CONSEQUENCES {
                    ui.label(format!("  • {t}"));
                }
                ui.label(MASK_MANUAL);
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .show(ui, |ui| {
                        for l in &lists {
                            ui.strong(l.message);
                            for (item, lines) in &l.items {
                                ui.label(format!("  • {item}"));
                                for line in lines {
                                    ui.weak(format!("      ◦ {line}"));
                                }
                            }
                            ui.add_space(4.0);
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut self.settings.mask_dialog_suppressed,
                        "Do not show again",
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ok = ui.button("OK").clicked();
                    });
                });
            });
        if !open || ok {
            self.mask_dialog = false;
        }
    }

    fn delete_active(&mut self) {
        if let Some(d) = self.active_doc() {
            let tolerance = 1.0 / d.view.zoom.max(1e-6);
            if let Some(s) = d.mesh_edit.as_mut() {
                s.delete_selected(tolerance);
                return;
            }
        }
        if let Some(d) = self.active_doc() {
            match d.delete_selection() {
                Ok(Some(note)) => self.log.info(note),
                Ok(None) => {}
                Err(e) => self.log.warn(e),
            }
        }
    }

    fn undo_active(&mut self, redo: bool) {
        let Some(d) = self.active_doc() else { return };
        // Mesh edit mode has its own steps until it is finished.
        if let Some(s) = d.mesh_edit.as_mut() {
            if if redo { s.redo() } else { s.undo() } {
                self.log.status = format!("{}: Mesh Edit", if redo { "Redo" } else { "Undo" });
            }
            return;
        }
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
        // Delete (shortcuts.md: Delete / Backspace).
        if !ctx.egui_wants_keyboard_input()
            && ctx.input_mut(|i| {
                i.consume_key(M::NONE, Key::Delete) || i.consume_key(M::NONE, Key::Backspace)
            })
        {
            self.delete_active();
        }
        // Automatic Mesh generator [Ctrl+Shift+A] (menus.md "ArtMesh").
        if !ctx.egui_wants_keyboard_input()
            && ctx.input_mut(|i| i.consume_shortcut(&K::new(shift_cmd, Key::A)))
        {
            self.open_auto_mesh();
        }
        // Auto attach Edge (line) [Ctrl+R] in mesh edit mode (§5.1).
        if !ctx.egui_wants_keyboard_input() {
            let ctrl_r = ctx.input_mut(|i| i.consume_shortcut(&K::new(M::COMMAND, Key::R)));
            if ctrl_r {
                if let Some(s) = self.active_doc().and_then(|d| d.mesh_edit.as_mut()) {
                    if let Err(e) = s.auto_connect() {
                        self.log.warn(e);
                    }
                }
            }
        }
        // Mesh edit [Ctrl+E] (menus.md "ArtMesh", "Mesh Edit mode").
        if ctx.input_mut(|i| i.consume_shortcut(&K::new(M::COMMAND, Key::E))) {
            self.toggle_mesh_edit();
        }
        // Edit level keys 1, 2, 3 (main-window.md §1).
        if !ctx.egui_wants_keyboard_input() {
            let level = ctx.input_mut(|i| {
                [(Key::Num1, 1u8), (Key::Num2, 2), (Key::Num3, 3)]
                    .into_iter()
                    .find(|(k, _)| i.consume_key(M::NONE, *k))
                    .map(|(_, l)| l)
            });
            if let (Some(l), Some(d)) = (level, self.active_doc()) {
                if d.mode == Mode::Modeling {
                    d.set_edit_level(l);
                }
            }
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
    mask_warnings: bool,
    mask_dialog: &'a mut bool,
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
                // Mask hint (dialogs.md §21), Modeling view only.
                let doc = &slot.doc;
                if self.mask_warnings
                    && doc.mode == Mode::Modeling
                    && doc.model.target_version != TARGET_LATEST
                    && !doc.mask_issues.is_empty()
                {
                    let text = egui::RichText::new(keyform_edit::checks::MASK_HINT)
                        .color(self.tokens.accent)
                        .size(15.0);
                    let galley_w = 360.0;
                    let at = egui::Rect::from_min_size(
                        r.rect.right_bottom() - egui::vec2(galley_w + 12.0, 34.0),
                        egui::vec2(galley_w, 26.0),
                    );
                    let hint = ui.put(
                        at,
                        egui::Button::new(text).fill(self.tokens.background.gamma_multiply(0.9)),
                    );
                    if hint.clicked() {
                        *self.active = Some(i);
                        *self.mask_dialog = true;
                    }
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
        if let (Some(id), Some(s)) = (self.initial_select.take(), self.slots.first_mut()) {
            use crate::document::Selection;
            let m = &s.doc.model;
            let sel = m
                .art_meshes
                .iter()
                .position(|x| x.info.id == id)
                .map(Selection::ArtMesh)
                .or_else(|| {
                    m.deformers
                        .iter()
                        .position(|x| x.info.id == id)
                        .map(Selection::Deformer)
                })
                .or_else(|| {
                    m.parts
                        .iter()
                        .position(|x| x.info.id == id)
                        .map(Selection::Part)
                });
            s.doc.select(sel);
            if self.initial_mesh_edit {
                match s.doc.start_mesh_edit() {
                    Ok(()) => {
                        // Frame the mesh.
                        let p = s.doc.mesh_edit.as_ref().map(|e| &e.graph.points);
                        if let Some(p) = p.filter(|p| !p.is_empty()) {
                            let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
                            for q in p.chunks_exact(2) {
                                b = [
                                    b[0].min(q[0]),
                                    b[1].min(q[1]),
                                    b[2].max(q[0]),
                                    b[3].max(q[1]),
                                ];
                            }
                            s.doc.view.fit_box = Some(b);
                            s.doc.view.fit_pending = true;
                        }
                    }
                    Err(e) => self.log.warn(e),
                }
            }
            if self.initial_auto_mesh {
                if let Err(e) = self.auto_mesh.show(&s.doc) {
                    self.log.warn(e);
                }
            }
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

        for s in &mut self.slots {
            if let Some(n) = s.doc.notice.take() {
                self.log.warn(n);
            }
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
                    mask_warnings: self.settings.mask_warnings,
                    mask_dialog: &mut self.mask_dialog,
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
        self.mask_dialog_ui(&ctx);
        let doc = self
            .active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc);
        crate::create_deformer::dialog(&ctx, &mut self.create_deformer, doc, &mut self.log);
        let doc = self
            .active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc);
        crate::auto_mesh_dialog::dialog(&ctx, &mut self.auto_mesh, doc, &mut self.log);
        if let Some(mut p) = self.pending_psd.take() {
            let mut docs: Vec<(u64, &mut Document)> =
                self.slots.iter_mut().map(|s| (s.uid, &mut s.doc)).collect();
            match crate::psd_dialog::dialog(&ctx, &mut p, &mut docs, &mut self.log) {
                crate::psd_dialog::Outcome::None => self.pending_psd = Some(p),
                crate::psd_dialog::Outcome::Cancel => {}
                crate::psd_dialog::Outcome::Done(uid) => {
                    if let Some(i) = self.slots.iter().position(|s| s.uid == uid) {
                        self.activate(i);
                    }
                }
            }
        }
        let doc = self
            .active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc);
        crate::param_dialog::dialog(&ctx, &mut self.param_dialog, doc, &mut self.log);
        let doc = self.active.and_then(|i| self.slots.get(i)).map(|s| &s.doc);
        crate::export_image::dialog(&ctx, &mut self.export_image, doc, &mut self.log);
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
        eframe::set_value(storage, SETTINGS_KEY, &self.settings);
        eframe::set_value(storage, EXPORT_IMAGE_KEY, &self.export_image);
    }
}

/// Mirror Edit panel: on/off, axis direction and position.
/// spec: 50-ui/canvas-and-tools.md §5.1 "Mirror Edit"
fn mirror_controls(ui: &mut egui::Ui, s: &mut crate::mesh_edit::MeshEdit, canvas: [f32; 2]) {
    ui.checkbox(&mut s.prefs.mirror_on, "Mirror Edit");
    let selected = s.centre(true);
    let m = &mut s.prefs.mirror;
    ui.radio_value(&mut m.vertical, true, "Axis: vertical (left ↔ right)");
    ui.radio_value(&mut m.vertical, false, "Axis: horizontal (top ↔ bottom)");
    let k = usize::from(!m.vertical);
    ui.horizontal(|ui| {
        ui.label("Axis");
        ui.add(egui::DragValue::new(&mut m.axis).speed(1.0).suffix(" px"));
    });
    // The axis starts at the canvas centre; both buttons return it there.
    let reset = ui.button("Reset Axis Position").clicked();
    if ui.button("Center of Canvas").clicked() || reset {
        m.axis = canvas[k] / 2.0;
    }
    if ui
        .add_enabled(
            selected.is_some(),
            egui::Button::new("Center of Selected Vertices"),
        )
        .clicked()
    {
        if let Some(c) = selected {
            m.axis = c[k];
        }
    }
}

/// Mesh-edit brush settings. spec: 50-ui/canvas-and-tools.md §5.1 "Brush"
fn brush_controls(ui: &mut egui::Ui, b: &mut crate::mesh_edit::MeshPrefs) {
    use crate::mesh_edit::BrushShape;
    ui.add(
        egui::DragValue::new(&mut b.brush_size)
            .range(2.0..=4000.0)
            .prefix("size ")
            .suffix(" px"),
    );
    ui.add(
        egui::DragValue::new(&mut b.brush_level)
            .range(0.0..=1.0)
            .speed(0.01)
            .prefix("level "),
    );
    ui.add(
        egui::DragValue::new(&mut b.brush_hardness)
            .range(0.0..=1.0)
            .speed(0.01)
            .prefix("hardness "),
    );
    ui.add(
        egui::DragValue::new(&mut b.brush_angle)
            .range(-180.0..=180.0)
            .prefix("angle ")
            .suffix("°"),
    );
    egui::ComboBox::from_id_salt("brush-shape")
        .selected_text(format!("{:?}", b.brush_shape))
        .show_ui(ui, |ui| {
            for s in [BrushShape::Circle, BrushShape::Square, BrushShape::Line] {
                ui.selectable_value(&mut b.brush_shape, s, format!("{s:?}"));
            }
        });
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
