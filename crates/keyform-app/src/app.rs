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
    ToolDetails,
}

impl Tab {
    const PALETTES: [(Tab, &'static str); 8] = [
        (Tab::Parts, "Parts"),
        (Tab::Deformer, "Deformer"),
        (Tab::Parameter, "Parameter"),
        (Tab::Inspector, "Inspector"),
        (Tab::Log, "Log"),
        (Tab::Project, "Project"),
        (Tab::ModelSettings, "Model Settings"),
        (Tab::ToolDetails, "Tool Details"),
    ];

    fn name(self) -> &'static str {
        Self::PALETTES
            .iter()
            .find(|(t, _)| *t == self)
            .map_or("", |(_, n)| n)
    }
}

/// Default layout (main-window.md §4, Modeling, with the viewer's Model
/// Settings palette beside Tool Details):
/// `H[ V[{Parts|Project},{Deformer|Log}], V[{Tool Details|Model Settings},{Inspector},{Parameter}], VIEW ]`
pub fn default_layout(views: Vec<Tab>) -> DockState<Tab> {
    let contents = if views.is_empty() {
        vec![Tab::Welcome]
    } else {
        views
    };
    let mut dock = DockState::new(contents);
    let s = dock.main_surface_mut();
    let [_view, mid] = s.split_left(
        NodeIndex::root(),
        0.56,
        vec![Tab::ToolDetails, Tab::ModelSettings],
    );
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
    pub parts_dnd_disabled: bool,
    /// The Parts palette asked for the Parts Settings dialog.
    pub open_parts_settings: bool,
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
    pub palettes: PaletteState,
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
    /// Then open Edit Texture Atlas (`--atlas`).
    pub initial_atlas: bool,
    /// Focus a palette by name (`--tab "Tool Details"`).
    pub initial_tab: Option<String>,
    /// Then pick a glue tool (`--glue-tool A|B|AB|BA|remerge`, with `--select`).
    pub initial_glue_tool: Option<keyform_edit::glue_brush::GlueBrush>,
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
    /// Animation image sequence / GIF / video export (dialogs.md §14).
    export_media: crate::export_media::MediaState,
    create_deformer: crate::create_deformer::CreateDeformerState,
    auto_mesh: crate::auto_mesh_dialog::AutoMeshDialog,
    atlas: crate::texture_atlas::AtlasWindow,
    new_model: crate::new_model_dialog::NewModelDialog,
    parts_settings: crate::parts_settings::PartsSettings,
    api_server: Option<crate::external_server::Server>,
    plugins: crate::external_host::Plugins,
    api_window: bool,
    api_mode: &'static str,
    api_recording: crate::external_host::ApiRecording,
    physics_editor: crate::physics_editor::PhysicsEditor,
    motion_sync: crate::motion_sync_dialog::MotionSyncDialog,
    sway: crate::generators_ui::SwayDialog,
    rotate3d: crate::generators_ui::Rotate3dDialog,
    face: crate::generators_ui::FaceDialog,
    template: crate::template_dialog::TemplateDialog,
    blend_limits: crate::blend_limits_dialog::BlendLimitsDialog,
    forms: crate::form_dialogs::FormDialogs,
    /// A PSD waiting for the import dialogs.
    pending_psd: Option<crate::psd_dialog::PendingPsd>,
    /// The macOS open-document handler was re-registered after launch.
    open_events_ready: bool,
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
    /// External application integration (off by default; 127.0.0.1 only).
    /// spec: 70-advanced/external-api.md §1
    pub api_enabled: bool,
    pub api_port: u16,
    /// UI language (dialogs.md §11 "Language").
    pub lang: crate::i18n::Lang,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mask_warnings: true,
            mask_dialog_suppressed: false,
            api_enabled: false,
            api_port: crate::external_api::DEFAULT_PORT,
            lang: crate::i18n::Lang::En,
        }
    }
}

const SETTINGS_KEY: &str = "keyform.settings";
const PLUGINS_KEY: &str = "keyform.external_apps";
const MS_PRESETS_KEY: &str = "keyform.motion_sync_presets";
const EXPORT_IMAGE_KEY: &str = "keyform.export_image";

/// `targetVersionNo` of "SDK(N/A)/Latest Cubism" (cmo3.md §2): no mask hint.
const TARGET_LATEST: i32 = 9_000_000;

const RECENT_KEY: &str = "keyform.recent_files";
const PLAYBACK_KEY: &str = "keyform.playback";
/// Dock layout of the last session (main-window.md §4: layouts persist).
const LAYOUT_KEY: &str = "keyform.dock_layout";
/// Version of the stored layout; palettes added since are put in once.
const LAYOUT_VERSION_KEY: &str = "keyform.dock_layout_version";
const LAYOUT_VERSION: u32 = 2;

/// Puts palettes newer than layout version `v` into a stored layout: Tool
/// Details (version 2) beside Model Settings.
fn upgrade_layout(mut dock: DockState<Tab>, v: u32) -> DockState<Tab> {
    if v < 2 && dock.find_tab(&Tab::ToolDetails).is_none() {
        let at = dock
            .find_tab(&Tab::ModelSettings)
            .or_else(|| dock.find_tab(&Tab::Inspector));
        match at {
            Some(p) => {
                dock[p.surface][p.node].insert_tab(egui_dock::TabIndex(0), Tab::ToolDetails);
            }
            None => dock.push_to_focused_leaf(Tab::ToolDetails),
        }
    }
    dock
}

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
        let layout_version = cc
            .storage
            .and_then(|s| eframe::get_value::<u32>(s, LAYOUT_VERSION_KEY))
            .unwrap_or(1);
        let dock = cc
            .storage
            .and_then(|s| eframe::get_value::<DockState<Tab>>(s, LAYOUT_KEY))
            .filter(usable_layout)
            .map(|d| upgrade_layout(d, layout_version))
            .unwrap_or_else(|| default_layout(Vec::new()));
        let playback = cc
            .storage
            .and_then(|s| eframe::get_value::<PlaybackOptions>(s, PLAYBACK_KEY))
            .unwrap_or_default();
        let settings = cc
            .storage
            .and_then(|s| eframe::get_value::<Settings>(s, SETTINGS_KEY))
            .unwrap_or_default();
        let plugins = cc
            .storage
            .and_then(|s| eframe::get_value::<crate::external_host::Plugins>(s, PLUGINS_KEY))
            .unwrap_or_default();
        crate::i18n::set(settings.lang);
        let ms_presets = cc
            .storage
            .and_then(|s| {
                eframe::get_value::<Vec<crate::motion_sync_dialog::UserPreset>>(s, MS_PRESETS_KEY)
            })
            .unwrap_or_default();
        let mut app = Self {
            api_server: None,
            plugins,
            api_window: false,
            api_mode: "Modeling",
            api_recording: Default::default(),
            settings,
            mask_dialog: false,
            create_deformer: Default::default(),
            auto_mesh: Default::default(),
            atlas: Default::default(),
            new_model: Default::default(),
            parts_settings: Default::default(),
            physics_editor: Default::default(),
            motion_sync: crate::motion_sync_dialog::MotionSyncDialog::with_presets(ms_presets),
            sway: Default::default(),
            rotate3d: Default::default(),
            face: Default::default(),
            template: Default::default(),
            blend_limits: Default::default(),
            forms: Default::default(),
            pending_psd: None,
            open_events_ready: false,
            param_dialog: Default::default(),
            export_image: cc
                .storage
                .and_then(|s| eframe::get_value(s, EXPORT_IMAGE_KEY))
                .unwrap_or_default(),
            export_media: Default::default(),
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
            initial_atlas: false,
            initial_tab: None,
            initial_glue_tool: None,
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
        let is_can3 = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("can3"));
        let opened = if is_can3 {
            Document::open_animation(path, &mut self.log)
        } else {
            Document::open(path, &mut self.log)
        };
        match opened {
            Ok(doc) => {
                self.add_slot(doc);
                self.recent.retain(|p| p != path);
                self.recent.insert(0, path.to_path_buf());
                self.recent.truncate(10);
            }
            Err(e) => self.log.error(e),
        }
    }

    /// Shows a document in a new view and activates it.
    fn add_slot(&mut self, doc: Document) {
        let uid = self.next_uid;
        self.next_uid += 1;
        self.slots.push(Slot {
            uid,
            doc,
            gpu: ViewGpu::default(),
        });
        self.add_view_tab(Tab::View(uid));
        self.activate(self.slots.len() - 1);
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
                    if ui.button(crate::i18n::tr("Yes")).clicked() {
                        choice = Some(Some(true));
                    }
                    if ui.button(crate::i18n::tr("No")).clicked() {
                        choice = Some(Some(false));
                    }
                    if ui.button(crate::i18n::tr("Cancel")).clicked() {
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

    /// Recording from external input (external-api.md §5): samples the
    /// active document's values (pushed values included) while on; when
    /// switched off the recording is saved as motion3.json, or added to the
    /// open animation as a new scene.
    fn api_record_step(&mut self, ctx: &egui::Context) {
        use keyform_anim::recorder::{Recorder, Thinning};
        let Some(i) = self.active.filter(|&i| i < self.slots.len()) else {
            return;
        };
        let rec = &mut self.api_recording;
        if rec.on {
            let d = &self.slots[i].doc;
            let r = rec.recorder.get_or_insert_with(|| {
                Recorder::new(
                    rec.fps,
                    d.model.parameters.iter().map(|p| p.id.clone()).collect(),
                )
            });
            if r.ids.len() == d.live.len() {
                r.push(ctx.input(|x| x.stable_dt).min(0.1), &d.live);
            }
            ctx.request_repaint();
        } else if let Some(r) = rec.recorder.take() {
            if r.frames.len() < 2 {
                return;
            }
            let thin = if rec.thinning == 1 {
                Thinning::Linear
            } else {
                Thinning::None
            };
            let text = r.to_motion3(thin, false);
            let d = &mut self.slots[i].doc;
            if d.anim.is_some() {
                let tmp = std::env::temp_dir().join(format!(
                    "keyform-api-recording-{}.motion3.json",
                    std::process::id()
                ));
                if std::fs::write(&tmp, &text).is_ok() {
                    match d.import_motion(&tmp) {
                        Ok(_) => self
                            .log
                            .info("Recording added to the animation as a new scene."),
                        Err(e) => self.log.warn(e),
                    }
                    let _ = std::fs::remove_file(&tmp);
                }
            } else if let Some(p) = rfd::FileDialog::new()
                .add_filter("motion3.json", &["json"])
                .set_file_name("recording.motion3.json")
                .save_file()
            {
                match std::fs::write(&p, text) {
                    Ok(()) => self.log.info(format!("recording saved to {}", p.display())),
                    Err(e) => self.log.warn(e.to_string()),
                }
            }
        }
    }

    /// Runs the external application API: starts / stops the server with
    /// the setting, answers requests, sends edit-mode events.
    fn external_api(&mut self, ctx: &egui::Context) {
        let s = self.settings;
        let running = self.api_server.as_ref().map(|x| (x.port, x.connections()));
        crate::external_host::settings_window(
            ctx,
            &mut self.api_window,
            &mut self.settings.api_enabled,
            &mut self.settings.api_port,
            running,
            &mut self.plugins,
            &mut self.api_recording,
        );
        self.api_record_step(ctx);
        if s.api_enabled
            && self
                .api_server
                .as_ref()
                .is_none_or(|x| x.port != s.api_port)
        {
            match crate::external_server::Server::start(s.api_port) {
                Ok(x) => {
                    self.log.info(format!(
                        "External application API on 127.0.0.1:{}",
                        s.api_port
                    ));
                    self.api_server = Some(x);
                }
                Err(e) => {
                    self.log.warn(format!("External application API: {e}"));
                    self.settings.api_enabled = false;
                }
            }
        } else if !s.api_enabled && self.api_server.is_some() {
            self.api_server = None;
        }
        let Some(server) = self.api_server.as_mut() else {
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        let mut host = crate::external_host::AppHost {
            slots: &mut self.slots,
            active: self.active,
            log: &mut self.log,
            plugins: &mut self.plugins,
        };
        server.poll(&mut host, now);
        let mode = crate::external_api::Host::edit_mode(&host);
        if mode != self.api_mode {
            self.api_mode = mode;
            server.event(
                "NotifyChangeEditMode",
                &serde_json::json!({ "EditMode": mode }),
                now,
            );
        }
        // Keep polling while clients are connected.
        if server.connections() > 0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    /// Opens the Physics Settings window (also `--physics`).
    pub fn open_physics(&mut self) {
        self.physics_editor.show();
    }

    fn open_dialog(&mut self) {
        let files = rfd::FileDialog::new()
            .add_filter("Models", &["cmo3", "can3", "moc3", "json", "psd"])
            .add_filter("Animation (*.can3)", &["can3"])
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

    /// Opens a new, unsaved project in a view and makes it active.
    fn add_new_project(
        &mut self,
        path: &std::path::Path,
        doc: keyform_formats::cmo3::Cmo3Document,
    ) {
        match Document::new_project(path, doc, &mut self.log) {
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
            }
            Err(e) => self.log.error(e),
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(crate::i18n::tr("File"), |ui| {
                ui.menu_button(crate::i18n::tr("New"), |ui| {
                    if ui
                        .add(egui::Button::new(crate::i18n::tr("Model")).shortcut_text("Ctrl+N"))
                        .clicked()
                    {
                        self.new_model.open = true;
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr("Animation…")).clicked() {
                        ui.close();
                        if let Some(m) = rfd::FileDialog::new()
                            .set_title("Model for the new animation")
                            .add_filter("Model project (*.cmo3)", &["cmo3"])
                            .pick_file()
                        {
                            match Document::new_animation(&m, &mut self.log) {
                                Ok(doc) => self.add_slot(doc),
                                Err(e) => self.log.error(e),
                            }
                        }
                    }
                });
                if ui
                    .add(egui::Button::new(crate::i18n::tr("Open")).shortcut_text("Ctrl+O"))
                    .clicked()
                {
                    self.open_dialog();
                }
                ui.menu_button(crate::i18n::tr("Recent Files"), |ui| {
                    let recent = self.recent.clone();
                    if recent.is_empty() {
                        ui.weak(crate::i18n::tr("(none)"));
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
                        egui::Button::new(crate::i18n::tr("Close")).shortcut_text("Ctrl+W"),
                    )
                    .clicked()
                {
                    if let Some(uid) = self.active.and_then(|i| self.slots.get(i)).map(|s| s.uid) {
                        self.request_close(uid);
                    }
                }
                if ui
                    .add_enabled(
                        !self.slots.is_empty(),
                        egui::Button::new(crate::i18n::tr("Close all")),
                    )
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
                        egui::Button::new(crate::i18n::tr("Save as…"))
                            .shortcut_text("Ctrl+Shift+S"),
                    )
                    .clicked()
                {
                    self.save_as_active();
                }
                ui.separator();
                ui.menu_button(crate::i18n::tr("Export Image/Video"), |ui| {
                    let animation = self
                        .active
                        .and_then(|i| self.slots.get(i))
                        .is_some_and(|s| s.doc.mode == Mode::Animation && s.doc.anim.is_some());
                    if ui
                        .add_enabled(
                            animation,
                            egui::Button::new(crate::i18n::tr("Image sequence / GIF / Video…")),
                        )
                        .on_disabled_hover_text("Needs the Animation workspace")
                        .clicked()
                    {
                        self.export_media.open = true;
                    }
                    let modeling = self
                        .active
                        .and_then(|i| self.slots.get(i))
                        .is_some_and(|s| s.doc.mode == Mode::Modeling);
                    if ui
                        .add_enabled(modeling, egui::Button::new(crate::i18n::tr("Image…")))
                        .on_disabled_hover_text("Needs the Modeling view of a project")
                        .clicked()
                    {
                        self.export_image.open = true;
                    }
                });
                ui.menu_button(crate::i18n::tr("Export For Runtime"), |ui| {
                    if ui
                        .add_enabled(
                            project,
                            egui::Button::new(crate::i18n::tr("Export as moc3 file…")),
                        )
                        .clicked()
                    {
                        self.export_runtime_active();
                    }
                    let anim = self
                        .active
                        .and_then(|i| self.slots.get(i))
                        .is_some_and(|s| s.doc.anim.is_some());
                    if ui
                        .add_enabled(
                            anim,
                            egui::Button::new(crate::i18n::tr(
                                "Export motion files (motion3.json)…",
                            )),
                        )
                        .on_disabled_hover_text("Needs an animation (.can3)")
                        .clicked()
                    {
                        self.export_motions_active();
                    }
                    if ui
                        .add_enabled(
                            anim,
                            egui::Button::new(crate::i18n::tr(
                                "Export expression files (exp3.json)…",
                            )),
                        )
                        .on_disabled_hover_text("Needs an animation (.can3)")
                        .clicked()
                    {
                        self.export_expressions_active();
                    }
                });
                ui.separator();
                if ui
                    .add(egui::Button::new(crate::i18n::tr("Exit")).shortcut_text("Alt+F4"))
                    .clicked()
                {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button(crate::i18n::tr("Edit"), |ui| {
                let doc = self.active.and_then(|i| self.slots.get(i)).map(|s| &s.doc);
                let undo = doc.and_then(|d| d.history.undo_name()).map(str::to_owned);
                let redo = doc.and_then(|d| d.history.redo_name()).map(str::to_owned);
                let r = ui.add_enabled(
                    undo.is_some(),
                    egui::Button::new(crate::i18n::tr("Undo")).shortcut_text("Ctrl+Z"),
                );
                if r.clicked() {
                    self.undo_active(false);
                }
                if let Some(n) = undo {
                    r.on_hover_text(n);
                }
                let r = ui.add_enabled(
                    redo.is_some(),
                    egui::Button::new(crate::i18n::tr("Redo")).shortcut_text("Ctrl+Y"),
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
                        egui::Button::new(crate::i18n::tr("Delete")).shortcut_text("Delete"),
                    )
                    .clicked()
                {
                    self.delete_active();
                }
                ui.separator();
                if ui
                    .add_enabled(
                        self.active.is_some(),
                        egui::Button::new(crate::i18n::tr("Deselect")).shortcut_text("Escape"),
                    )
                    .clicked()
                {
                    if let Some(d) = self.active_doc() {
                        d.select(None);
                    }
                }
            });
            ui.menu_button(crate::i18n::tr("Show"), |ui| {
                ui.checkbox(
                    &mut self.settings.mask_warnings,
                    crate::i18n::tr("Always show warnings regarding masks"),
                );
                ui.menu_button(crate::i18n::tr("Language"), |ui| {
                    for l in crate::i18n::Lang::ALL {
                        if ui
                            .radio_value(&mut self.settings.lang, l, l.native_name())
                            .clicked()
                        {
                            crate::i18n::set(l);
                            ui.close();
                        }
                    }
                });
                if ui
                    .button(crate::i18n::tr(
                        "External Application Integration Settings…",
                    ))
                    .clicked()
                {
                    self.api_window = true;
                    ui.close();
                }
                ui.separator();
                let has = self.active.is_some();
                if ui
                    .add_enabled(has, egui::Button::new(crate::i18n::tr("Fit to View")))
                    .clicked()
                {
                    if let Some(d) = self.active_doc() {
                        d.view.fit_pending = true;
                    }
                }
                if ui
                    .add_enabled(
                        has,
                        egui::Button::new(crate::i18n::tr("Zoom In"))
                            .shortcut_text("Ctrl+Alt+Shift+Left"),
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
                        egui::Button::new(crate::i18n::tr("Zoom Out"))
                            .shortcut_text("Ctrl+Alt+Shift+Right"),
                    )
                    .clicked()
                {
                    if let Some(d) = self.active_doc() {
                        d.view.zoom /= 1.25;
                    }
                }
                if ui
                    .add_enabled(has, egui::Button::new(crate::i18n::tr("Reset Rotation")))
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
                        egui::Checkbox::new(&mut mirror, crate::i18n::tr("Reflect Horizontally")),
                    )
                    .changed()
                {
                    if let Some(d) = self.active_doc() {
                        d.view.mirror = mirror;
                    }
                }
            });
            if self.active_doc().is_some_and(|d| d.mesh_edit.is_some()) {
                ui.menu_button(crate::i18n::tr("Mesh Edit"), |ui| self.mesh_edit_menu(ui));
            } else {
                ui.menu_button(crate::i18n::tr("Modeling"), |ui| self.modeling_menu(ui));
            }
            ui.menu_button(crate::i18n::tr("Animation"), |ui| self.animation_menu(ui));
            ui.menu_button(crate::i18n::tr("Window"), |ui| {
                for (tab, name) in Tab::PALETTES {
                    let mut shown = self.dock.find_tab(&tab).is_some();
                    if ui.checkbox(&mut shown, name).changed() {
                        self.toggle_palette(tab);
                    }
                }
                ui.separator();
                if ui.button(crate::i18n::tr("Reset Current Layout")).clicked() {
                    self.reset_layout();
                }
                ui.separator();
                let mut dark = self.dark;
                if ui
                    .checkbox(&mut dark, crate::i18n::tr("Dark Theme"))
                    .changed()
                {
                    self.dark = dark;
                    self.tokens = theme::apply(ui.ctx(), dark);
                }
            });
            ui.menu_button(crate::i18n::tr("Help"), |ui| {
                ui.label(format!("Keyform {}", env!("CARGO_PKG_VERSION")));
                ui.weak(crate::i18n::tr(
                    "Clean-room 2D-mesh rigging and animation editor.",
                ));
            });
        });
    }

    /// Modeling menu (subset). spec: 50-ui/menus.md "Modeling"
    fn modeling_menu(&mut self, ui: &mut egui::Ui) {
        use crate::create_deformer::Kind;
        let Some(i) = self.active else {
            ui.weak(crate::i18n::tr("No model"));
            return;
        };
        let doc = &mut self.slots[i].doc;
        let ok = doc.editable() && doc.mode == Mode::Modeling;
        ui.menu_button(crate::i18n::tr("Deformer"), |ui| {
            if ui
                .add_enabled(
                    ok,
                    egui::Button::new(crate::i18n::tr("Create Warp Deformer…")),
                )
                .clicked()
            {
                self.create_deformer.show(Kind::Warp, doc);
                ui.close();
            }
            if ui
                .add_enabled(
                    ok,
                    egui::Button::new(crate::i18n::tr("Create rotation deformer…")),
                )
                .clicked()
            {
                self.create_deformer.show(Kind::Rotation, doc);
                ui.close();
            }
        });
        ui.menu_button(crate::i18n::tr("ArtMesh"), |ui| {
            let one = ok
                && doc
                    .selection
                    .objects
                    .iter()
                    .filter(|o| matches!(o, crate::document::Selection::ArtMesh(_)))
                    .count()
                    == 1;
            if ui
                .add_enabled(
                    one,
                    egui::Button::new(crate::i18n::tr("Mesh edit")).shortcut_text("Ctrl+E"),
                )
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
                    egui::Button::new(crate::i18n::tr("Automatic Mesh generator…"))
                        .shortcut_text("Ctrl+Shift+A"),
                )
                .clicked()
            {
                if let Err(e) = self.auto_mesh.show(doc) {
                    self.log.warn(e);
                }
                ui.close();
            }
        });
        ui.menu_button(crate::i18n::tr("Glue"), |ui| {
            for t in keyform_edit::glue_brush::GlueBrush::ALL {
                let on = doc.glue_tool.as_ref().is_some_and(|g| g.tool == t);
                if ui
                    .add_enabled(ok, egui::Button::selectable(on, t.label()))
                    .clicked()
                {
                    if let Err(e) = doc.set_glue_tool((!on).then_some(t)) {
                        self.log.warn(e);
                    }
                    ui.close();
                }
            }
            ui.separator();
            if ui
                .add_enabled(
                    ok,
                    egui::Button::new(crate::i18n::tr("Generate and Register Glue"))
                        .shortcut_text("Ctrl+G"),
                )
                .on_hover_text(crate::i18n::tr(
                    "Glue the selected vertex of each of two ArtMeshes",
                ))
                .clicked()
            {
                if let Err(e) = doc.glue_selected_vertices() {
                    self.log.warn(e);
                }
                ui.close();
            }
            if ui
                .add_enabled(
                    ok,
                    egui::Button::new(crate::i18n::tr("Unbind Glue Vertices"))
                        .shortcut_text("Ctrl+Shift+G"),
                )
                .on_hover_text(crate::i18n::tr(
                    "Remove the glue between the selected vertex of each of two ArtMeshes",
                ))
                .clicked()
            {
                if let Err(e) = doc.unglue_selected_vertices() {
                    self.log.warn(e);
                }
                ui.close();
            }
        });
        ui.menu_button(crate::i18n::tr("Part"), |ui| {
            if ui
                .add_enabled(ok, egui::Button::new(crate::i18n::tr("New Part")))
                .clicked()
            {
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
        ui.menu_button(crate::i18n::tr("Parameter"), |ui| {
            if ui
                .add_enabled(
                    ok,
                    egui::Button::new(crate::i18n::tr("Limit Settings for Blend Shape Weights…")),
                )
                .clicked()
            {
                self.blend_limits.show();
                ui.close();
            }
            if ui
                .add_enabled(
                    ok,
                    egui::Button::new(crate::i18n::tr("Synthesize corners…"))
                        .shortcut_text("Ctrl+4"),
                )
                .clicked()
            {
                if let Err(e) = self.forms.open_corners(doc) {
                    self.log.warn(e);
                }
                ui.close();
            }
            if ui
                .add_enabled(ok, egui::Button::new(crate::i18n::tr("Motion Mirroring…")))
                .clicked()
            {
                if let Err(e) = self.forms.open_mirror(doc) {
                    self.log.warn(e);
                }
                ui.close();
            }
            if ui
                .add_enabled(
                    ok,
                    egui::Button::new(crate::i18n::tr("Extended Interpolation…")),
                )
                .clicked()
            {
                self.forms.open_extended(doc);
                ui.close();
            }
            if ui
                .add_enabled(ok, egui::Button::new(crate::i18n::tr("Batch Reflect…")))
                .clicked()
            {
                self.forms.open_batch(doc);
                ui.close();
            }
        });
        ui.menu_button(crate::i18n::tr("Edit Form"), |ui| {
            if ui
                .add_enabled(
                    ok,
                    egui::Button::new(crate::i18n::tr("Copy Form")).shortcut_text("Ctrl+Shift+C"),
                )
                .clicked()
            {
                if let Err(e) = doc.copy_form() {
                    self.log.warn(e);
                }
                ui.close();
            }
            let has = !doc.form_clipboard.is_empty();
            if ui
                .add_enabled(
                    ok && has,
                    egui::Button::new(crate::i18n::tr("Paste Form")).shortcut_text("Ctrl+Shift+V"),
                )
                .clicked()
            {
                if let Err(e) = doc.paste_form("Paste Form", 1.0, self.forms.special) {
                    self.log.warn(e);
                }
                ui.close();
            }
            if ui
                .add_enabled(
                    ok && has,
                    egui::Button::new(crate::i18n::tr("Blend Form…")).shortcut_text("Ctrl+Shift+B"),
                )
                .clicked()
            {
                self.forms.open_blend();
                ui.close();
            }
            if ui
                .button(crate::i18n::tr("Options for Paste Form Special…"))
                .clicked()
            {
                self.forms.special_open = true;
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(ok, egui::Button::new(crate::i18n::tr("Flip…")))
                .clicked()
            {
                self.forms.open_flip();
                ui.close();
            }
        });
        ui.separator();
        ui.menu_button(crate::i18n::tr("Texture Atlas"), |ui| {
            if ui
                .add_enabled(
                    ok,
                    egui::Button::new(crate::i18n::tr("Edit Texture Atlas…"))
                        .shortcut_text("Ctrl+T"),
                )
                .clicked()
            {
                self.open_atlas();
                ui.close();
            }
        });
        ui.separator();
        if ui
            .add_enabled(
                doc_ok_for_physics(self),
                egui::Button::new(crate::i18n::tr("Physics Settings…")),
            )
            .clicked()
        {
            self.physics_editor.show();
            ui.close();
        }
        if ui
            .add_enabled(
                doc_ok_for_physics(self),
                egui::Button::new(crate::i18n::tr("Motion-sync Settings…")),
            )
            .clicked()
        {
            self.motion_sync.show();
            ui.close();
        }
        ui.menu_button(crate::i18n::tr("Model Template"), |ui| {
            if ui
                .add_enabled(
                    doc_ok_for_physics(self),
                    egui::Button::new(crate::i18n::tr("Apply Model Template…")),
                )
                .clicked()
            {
                self.template.show();
                ui.close();
            }
            if ui
                .add_enabled(
                    doc_ok_for_physics(self),
                    egui::Button::new(crate::i18n::tr("Export model template…")),
                )
                .clicked()
            {
                self.template.export = Some((String::new(), String::new(), String::new()));
                ui.close();
            }
        });
        ui.separator();
        if ui
            .add_enabled(
                doc_ok_for_physics(self),
                egui::Button::new(crate::i18n::tr("Auto Generation of Sway Motion…")),
            )
            .clicked()
        {
            self.sway.show();
            ui.close();
        }
        if ui
            .add_enabled(
                doc_ok_for_physics(self),
                egui::Button::new(crate::i18n::tr("Apply 3D Rotation Expression…")),
            )
            .clicked()
        {
            self.rotate3d.open = true;
            ui.close();
        }
        if ui
            .add_enabled(
                doc_ok_for_physics(self),
                egui::Button::new(crate::i18n::tr("Auto Generation of Face Motion…")),
            )
            .clicked()
        {
            self.face.open = true;
            ui.close();
        }
        if ui
            .add_enabled(
                self.active.is_some(),
                egui::Button::new(crate::i18n::tr("Set Random Pose")),
            )
            .clicked()
        {
            if let Some(d) = self
                .active
                .and_then(|i| self.slots.get_mut(i))
                .map(|s| &mut s.doc)
            {
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(1, |t| t.as_nanos() as u64);
                crate::generators_ui::random_pose(d, seed);
            }
            ui.close();
        }
    }

    /// Modeling ▸ Texture Atlas ▸ Edit Texture Atlas [Ctrl+T].
    fn open_atlas(&mut self) {
        let Some(doc) = self.active.and_then(|i| self.slots.get(i)).map(|s| &s.doc) else {
            return;
        };
        if let Err(e) = self.atlas.show(doc) {
            self.log.warn(e);
        }
    }

    /// The "Mesh Edit mode" menu (subset). spec: 50-ui/menus.md "Mesh Edit mode"
    fn mesh_edit_menu(&mut self, ui: &mut egui::Ui) {
        use crate::mesh_edit::MeshTool;
        if ui
            .add(egui::Button::new(crate::i18n::tr("Finish Mesh editing")).shortcut_text("Ctrl+E"))
            .clicked()
        {
            self.toggle_mesh_edit();
            ui.close();
        }
        if ui
            .button(crate::i18n::tr(
                "Automatically Generate Meshes(Mesh Editing Mode)",
            ))
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
            .add(
                egui::Button::new(crate::i18n::tr("Auto attach Edge (line)"))
                    .shortcut_text("Ctrl+R"),
            )
            .on_hover_text(crate::i18n::tr("Automatically join vertices"))
            .clicked()
        {
            notice = s.auto_connect().err();
            ui.close();
        }
        if ui
            .button(crate::i18n::tr("Reset Mesh"))
            .on_hover_text(crate::i18n::tr(
                "Restores the mesh to the state immediately after the ArtMesh was created.",
            ))
            .clicked()
        {
            notice = s.reset_mesh().err();
            ui.close();
        }
        ui.separator();
        if ui.button(crate::i18n::tr("Edge swap")).clicked() {
            notice = s.swap_edge().err();
            ui.close();
        }
        if ui
            .button(crate::i18n::tr("4 Division"))
            .on_hover_text(crate::i18n::tr("Divide one triangle into four triangles"))
            .clicked()
        {
            notice = s.divide4().err();
            ui.close();
        }
        if ui.button(crate::i18n::tr("Merge Vertices")).clicked() {
            notice = s.merge().err();
            ui.close();
        }
        ui.menu_button(crate::i18n::tr("Mirror Edit"), |ui| {
            mirror_controls(ui, s, canvas);
        });
        ui.separator();
        if ui
            .add_enabled(
                !s.selected.is_empty(),
                egui::Button::new(crate::i18n::tr("Delete Vertex")).shortcut_text("Delete"),
            )
            .clicked()
        {
            s.delete_selected(tolerance);
            ui.close();
        }
        ui.menu_button(crate::i18n::tr("Subtools"), |ui| {
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
        ui.checkbox(&mut o.expression, crate::i18n::tr("Apply Expression"));
        ui.checkbox(&mut o.eye_blink, crate::i18n::tr("Automatic Eye-Blinking"));
        ui.checkbox(&mut o.physics, crate::i18n::tr("Apply Physics"));
        ui.checkbox(&mut o.pose, crate::i18n::tr("Enable Pose Switching"));
        ui.checkbox(&mut o.motion_sync, crate::i18n::tr("Motion-sync"))
            .on_hover_text(crate::i18n::tr(
                "Lip sync from the playing motion's sound (motionsync3.json)",
            ));
        ui.checkbox(&mut o.idle_motion, crate::i18n::tr("Idle Motion"));
        ui.checkbox(&mut o.track_cursor, crate::i18n::tr("Track cursor"))
            .on_hover_text(crate::i18n::tr(
                "Drag over the view: left button = primary, right = secondary target",
            ));
        ui.checkbox(&mut o.breath, crate::i18n::tr("Breath"));
        ui.checkbox(&mut o.parameter_repeat, crate::i18n::tr("Parameter repeat"))
            .on_hover_text(crate::i18n::tr(
                "Repeat parameters wrap around instead of stopping at min/max",
            ));
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
            let label = crate::i18n::tr(match mode {
                Mode::Modeling => "Modeling",
                Mode::Viewer => "Viewer",
                Mode::Animation => "Animation",
            });
            let has_anim = doc.as_ref().is_some_and(|d| d.anim.is_some());
            let mut chosen = mode;
            egui::ComboBox::from_id_salt("workspace")
                .selected_text(label)
                .width(140.0)
                .show_ui(ui, |ui| {
                    ui.add_enabled_ui(editable, |ui| {
                        ui.selectable_value(
                            &mut chosen,
                            Mode::Modeling,
                            crate::i18n::tr("Modeling"),
                        );
                    });
                    ui.selectable_value(&mut chosen, Mode::Viewer, crate::i18n::tr("Viewer"));
                    ui.add_enabled_ui(has_anim, |ui| {
                        ui.selectable_value(
                            &mut chosen,
                            Mode::Animation,
                            crate::i18n::tr("Animation"),
                        );
                    });
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
            ui.weak(crate::i18n::tr("Edit Level:"));
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
                if ui
                    .add_enabled(modeling, b)
                    .on_hover_text(crate::i18n::tr(tip))
                    .clicked()
                {
                    chosen = Some(l);
                }
            }
            if let (Some(l), Some(d)) = (chosen, doc) {
                d.set_edit_level(l);
            }
            ui.separator();
            if let Some(d) = self
                .active
                .and_then(|i| self.slots.get_mut(i))
                .map(|s| &mut s.doc)
                .filter(|d| d.mode == crate::document::Mode::Modeling)
            {
                let arrow = !d.lasso
                    && d.glue_tool.is_none()
                    && d.soft_select.is_none()
                    && d.path_tool.is_none();
                if ui
                    .add(egui::Button::selectable(arrow, crate::i18n::tr("Arrow")))
                    .on_hover_text(crate::i18n::tr(
                        "Arrow tool (A): click to select, drag for a rectangle",
                    ))
                    .clicked()
                {
                    d.lasso = false;
                    let _ = d.set_glue_tool(None);
                    let _ = d.set_path_tool(false);
                }
                if ui
                    .add(egui::Button::selectable(d.lasso, crate::i18n::tr("Lasso")))
                    .on_hover_text(crate::i18n::tr(
                        "Lasso Tool (L): drag on empty space to lasso points",
                    ))
                    .clicked()
                {
                    d.lasso = true;
                    let _ = d.set_soft_select(false);
                }
                if ui
                    .add(egui::Button::selectable(
                        d.soft_select.is_some(),
                        crate::i18n::tr("Soft"),
                    ))
                    .on_hover_text(crate::i18n::tr(
                        "Brush Selection tool (B): paint weighted point selections",
                    ))
                    .clicked()
                {
                    let on = d.soft_select.is_none();
                    let _ = d.set_soft_select(on);
                }
                if ui
                    .add(egui::Button::selectable(
                        d.path_tool.is_some(),
                        crate::i18n::tr("Path"),
                    ))
                    .on_hover_text(crate::i18n::tr("Control Point edit (P): deform paths"))
                    .clicked()
                {
                    let on = d.path_tool.is_none();
                    if let Err(e) = d.set_path_tool(on) {
                        d.notice = Some(e);
                    }
                }
                if ui
                    .add(egui::Button::selectable(
                        d.temp_path.is_some(),
                        crate::i18n::tr("Temp path"),
                    ))
                    .on_hover_text(crate::i18n::tr(
                        "Temporary path: bend the selected points along a temporary line",
                    ))
                    .clicked()
                {
                    let on = d.temp_path.is_none();
                    if let Err(e) = d.set_temp_path(on) {
                        d.notice = Some(e);
                    }
                }
                ui.separator();
            }
            if self.active_doc().is_some_and(|d| d.mesh_edit.is_some()) {
                ui.weak(crate::i18n::tr("Mesh edit"));
                if ui
                    .button(crate::i18n::tr("Finish"))
                    .on_hover_text(crate::i18n::tr("Finish Mesh editing (Ctrl+E)"))
                    .clicked()
                {
                    self.toggle_mesh_edit();
                }
                ui.separator();
            }
            if let Some(i) = self.active {
                if ui.button(crate::i18n::tr("Reset parameters")).clicked() {
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

    /// Unused source images (psd-import.md §7.1), asked before every save
    /// of a model: Yes removes them (one undo step), No keeps them, Cancel
    /// aborts the save (returns false).
    fn unused_source_images(&mut self, i: usize) -> bool {
        use keyform_formats::psd_replace::{remove_layered_images, unused_layered_images};
        let Some(s) = self.slots.get_mut(i) else {
            return false;
        };
        let unused = unused_layered_images(&s.doc.model);
        if unused.is_empty() {
            return true;
        }
        let mut names = unused
            .iter()
            .map(|&k| s.doc.model.layered_images[k].name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        if names.chars().count() > 60 {
            names = names.chars().take(60).collect::<String>() + "…";
        }
        let answer = rfd::MessageDialog::new()
            .set_title("Confirm")
            .set_description(format!(
                "There are {} unused source images\n{names}\n\nWould you like to delete them?",
                unused.len()
            ))
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show();
        match answer {
            rfd::MessageDialogResult::Yes => {
                if s.doc.mode != crate::document::Mode::Modeling || s.doc.mesh_edit.is_some() {
                    self.log.warn("Please call in Modeling View");
                    return false;
                }
                s.doc.edit("Remove unused raw images", |m| {
                    remove_layered_images(m, &unused)
                });
                true
            }
            rfd::MessageDialogResult::No => true,
            _ => false,
        }
    }

    /// Saves document `i`; returns false if that failed.
    fn save_slot(&mut self, i: usize) -> bool {
        let Some(s) = self.slots.get_mut(i) else {
            return false;
        };
        // An animation saves its .can3 (the model is saved from Modeling);
        // a new one asks where first.
        if s.doc.anim.as_ref().is_some_and(|a| a.dirty) {
            let to = if s.doc.anim.as_ref().is_some_and(|a| a.untitled) {
                let st = s.doc.anim.as_ref().expect("anim");
                let dir = st
                    .path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_default();
                let name = st
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                match rfd::FileDialog::new()
                    .set_directory(dir)
                    .set_file_name(name)
                    .add_filter("Animation (*.can3)", &["can3"])
                    .save_file()
                {
                    Some(p) => Some(p),
                    None => return false,
                }
            } else {
                None
            };
            match s.doc.save_animation_to(to.as_deref()) {
                Ok(p) => self.log.info(format!("saved {}", p.display())),
                Err(e) => {
                    self.log.error(format!("save failed: {e}"));
                    return false;
                }
            }
            if !s.doc.is_modified() {
                return true;
            }
        }
        if !s.doc.editable() {
            crate::viewer_settings::save(&mut s.doc, &mut self.log);
            return true;
        }
        let untitled = s.doc.untitled;
        if !untitled && !self.unused_source_images(i) {
            return false;
        }
        if untitled {
            let active = self.active;
            self.active = Some(i);
            self.save_as_active();
            self.active = active;
            return !self.slots.get(i).is_some_and(|s| s.doc.untitled);
        }
        let Some(s) = self.slots.get_mut(i) else {
            return false;
        };
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
        let Some(i) = self.active else { return };
        if !self.unused_source_images(i) {
            return;
        }
        let Some(s) = self.slots.get_mut(i) else {
            return;
        };
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

    /// File ▸ Export For Runtime ▸ motion files: every scene marked for
    /// export as `<scene>.motion3.json` in a chosen folder (motion3-json.md §2).
    fn export_motions_active(&mut self) {
        let Some(d) = self.active.and_then(|i| self.slots.get(i)).map(|s| &s.doc) else {
            return;
        };
        let start = d
            .anim
            .as_ref()
            .and_then(|a| a.path.parent().map(|p| p.to_path_buf()))
            .unwrap_or_default();
        let Some(dir) = rfd::FileDialog::new().set_directory(start).pick_folder() else {
            return;
        };
        match d.export_motions(&dir) {
            Ok(files) => self.log.info(format!(
                "Exported {} motion files to {}",
                files.len(),
                dir.display()
            )),
            Err(e) => self.log.error(e),
        }
    }

    /// Exports every scene of the animation as an expression (exp3.json).
    fn export_expressions_active(&mut self) {
        let Some(d) = self.active.and_then(|i| self.slots.get(i)).map(|s| &s.doc) else {
            return;
        };
        let start = d
            .anim
            .as_ref()
            .and_then(|a| a.path.parent().map(|p| p.to_path_buf()))
            .unwrap_or_default();
        let Some(dir) = rfd::FileDialog::new().set_directory(start).pick_folder() else {
            return;
        };
        match d.export_expressions(&dir) {
            Ok(files) => self.log.info(format!(
                "Exported {} expression files to {}",
                files.len(),
                dir.display()
            )),
            Err(e) => self.log.error(e),
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
                // External-API export events (external-api.md §4.1).
                if let Some(server) = &self.api_server {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_millis() as u64);
                    server.export_events(&files, &s.doc.path, now);
                }
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
                        crate::i18n::tr("Do not show again"),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ok = ui.button(crate::i18n::tr("OK")).clicked();
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
        // The Animation workspace undoes animation edits.
        if d.mode == Mode::Animation {
            if let Some(n) = d.anim.as_mut().and_then(|a| a.step(redo)) {
                self.log.status = format!("{}: {n}", if redo { "Redo" } else { "Undo" });
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
        // Glue Vertices [Ctrl+G] / unbind [Ctrl+Shift+G] in the Modeling
        // view (glue.md "Bind / unbind").
        if !ctx.egui_wants_keyboard_input() {
            let unbind = ctx.input_mut(|i| i.consume_shortcut(&K::new(shift_cmd, Key::G)));
            let bind =
                !unbind && ctx.input_mut(|i| i.consume_shortcut(&K::new(M::COMMAND, Key::G)));
            if bind || unbind {
                // In mesh edit: between the two meshes (glue.md "Bind / unbind").
                let mut done = false;
                if let Some(d) = self.active_doc() {
                    let max_dist = 40.0 / d.view.zoom.max(1e-6);
                    let model = d.model.clone();
                    if let Some(s) = d.mesh_edit.as_mut() {
                        done = true;
                        match s.bind(unbind, max_dist, &model) {
                            Ok(n) => self.log.info(format!(
                                "{n} vertex pairs {} (applied when mesh editing finishes).",
                                if unbind { "unbound" } else { "bound" }
                            )),
                            Err(e) => self.log.warn(e),
                        }
                    }
                }
                if let Some(d) = self.active_doc().filter(|d| !done && d.mesh_edit.is_none()) {
                    let r = if bind {
                        d.glue_selected_vertices()
                    } else {
                        d.unglue_selected_vertices()
                    };
                    if let Err(e) = r {
                        self.log.warn(e);
                    }
                }
            }
        }
        // Arrow tool [A] / Lasso Tool [L] (menus.md "Keyboard-only commands").
        if !ctx.egui_wants_keyboard_input() {
            let (a, l, b, p) = ctx.input_mut(|i| {
                (
                    i.consume_shortcut(&K::new(M::NONE, Key::A)),
                    i.consume_shortcut(&K::new(M::NONE, Key::L)),
                    i.key_pressed(Key::B) && i.modifiers.is_none(),
                    i.consume_shortcut(&K::new(M::NONE, Key::P)),
                )
            });
            // Control Point edit [P] (canvas-and-tools.md §4).
            if p {
                if let Some(d) = self.active_doc() {
                    if let Err(e) = d.set_path_tool(true) {
                        self.log.warn(e);
                    }
                }
            }
            if a || l {
                if let Some(d) = self.active_doc() {
                    d.lasso = l;
                    let _ = d.set_soft_select(false);
                    let _ = d.set_path_tool(false);
                    if a {
                        let _ = d.set_glue_tool(None);
                    }
                }
            }
            // Brush Selection tool [B]; in a brush tool B + drag sizes the
            // brush instead (canvas-and-tools.md §1).
            if b {
                if let Some(d) = self.active_doc() {
                    let brush = d.glue_tool.is_some()
                        || d.soft_select.is_some()
                        || d.mesh_edit
                            .as_ref()
                            .is_some_and(|s| s.tool == crate::mesh_edit::MeshTool::Brush);
                    if !brush {
                        if let Err(e) = d.set_soft_select(true) {
                            self.log.warn(e);
                        }
                    }
                }
            }
        }
        // Edit Form: Copy [Ctrl+Shift+C], Paste [Ctrl+Shift+V], Blend
        // [Ctrl+Shift+B]; Synthesize corners [Ctrl+4] (menus.md).
        if !ctx.egui_wants_keyboard_input() {
            let (c, v, b, four) = ctx.input_mut(|i| {
                (
                    i.consume_shortcut(&K::new(shift_cmd, Key::C)),
                    i.consume_shortcut(&K::new(shift_cmd, Key::V)),
                    i.consume_shortcut(&K::new(shift_cmd, Key::B)),
                    i.consume_shortcut(&K::new(M::COMMAND, Key::Num4)),
                )
            });
            let special = self.forms.special;
            let mut warn = None;
            if let Some(d) = self
                .active_doc()
                .filter(|d| d.editable() && d.mesh_edit.is_none())
            {
                if c {
                    warn = d.copy_form().err();
                } else if v {
                    warn = d.paste_form("Paste Form", 1.0, special).err();
                }
            }
            if let Some(e) = warn {
                self.log.warn(e);
            }
            if b && self
                .active_doc()
                .is_some_and(|d| !d.form_clipboard.is_empty())
            {
                self.forms.open_blend();
            }
            if four {
                if let Some(i) = self.active {
                    if let Err(e) = self.forms.open_corners(&self.slots[i].doc) {
                        self.log.warn(e);
                    }
                }
            }
        }
        // File ▸ New ▸ Model [Ctrl+N].
        if !ctx.egui_wants_keyboard_input()
            && ctx.input_mut(|i| i.consume_shortcut(&K::new(M::COMMAND, Key::N)))
        {
            self.new_model.open = true;
        }
        // Edit Texture Atlas [Ctrl+T] (menus.md "Texture Atlas").
        if !ctx.egui_wants_keyboard_input()
            && ctx.input_mut(|i| i.consume_shortcut(&K::new(M::COMMAND, Key::T)))
        {
            self.open_atlas();
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
    new_request: &'a mut bool,
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
            Tab::Welcome => crate::i18n::tr("Home").into(),
            t => crate::i18n::tr(t.name()).into(),
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
                        ui.heading(crate::i18n::tr("Keyform"));
                        ui.label(crate::i18n::tr("Drop a .cmo3, .moc3 or .model3.json here"));
                        if ui.button(crate::i18n::tr("Open…")).clicked() {
                            *self.open_request = true;
                        }
                        if ui.button(crate::i18n::tr("New Model…")).clicked() {
                            *self.new_request = true;
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
                    if palettes::parts_ui(
                        ui,
                        d,
                        &mut p.show_deformers,
                        &mut p.parts_filter,
                        &mut p.parts_dnd_disabled,
                    ) {
                        p.open_parts_settings = true;
                    }
                }
                None => {
                    ui.weak(crate::i18n::tr("No model"));
                }
            },
            Tab::Deformer => match self.active_doc() {
                Some(d) => palettes::deformer_ui(ui, d),
                None => {
                    ui.weak(crate::i18n::tr("No model"));
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
                        ui.weak(crate::i18n::tr("No model"));
                    }
                }
            }
            Tab::Inspector => {
                let doc = self
                    .active
                    .and_then(|i| self.slots.get_mut(i))
                    .map(|s| &mut s.doc);
                match doc {
                    Some(d) => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| palettes::inspector_ui(ui, d, self.log));
                    }
                    None => {
                        ui.weak(crate::i18n::tr("No model"));
                    }
                }
            }
            Tab::Log => {
                let tokens = *self.tokens;
                palettes::log_ui(ui, self.log, &tokens);
            }
            Tab::Project => {
                let docs: Vec<&Document> = self.slots.iter().map(|s| &s.doc).collect();
                match palettes::project_ui(ui, &docs, *self.active) {
                    Some(palettes::ProjectAction::Activate(i)) => *self.active = Some(i),
                    Some(palettes::ProjectAction::CreateArtMesh { doc, image, layer }) => {
                        if let Some(s) = self.slots.get_mut(doc) {
                            if let Err(e) = s.doc.create_art_mesh_from_layer(image, &layer) {
                                self.log.warn(e);
                            }
                        }
                        *self.active = Some(doc);
                    }
                    None => {}
                }
            }
            Tab::ToolDetails => {
                let tokens = *self.tokens;
                let doc = self
                    .active
                    .and_then(|i| self.slots.get_mut(i))
                    .map(|s| &mut s.doc);
                tool_details_ui(ui, doc, self.log, &tokens);
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
                        ui.weak(crate::i18n::tr("No model"));
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
        // macOS open-document events: re-register once launched (AppKit may
        // have replaced the handler), then open what arrived.
        if !self.open_events_ready {
            self.open_events_ready = true;
            keyform_macos_open::install();
        }
        for p in keyform_macos_open::take() {
            self.open(&p);
        }
        self.shortcuts(&ctx);
        self.external_api(&ctx);
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
        if let Some(name) = self.initial_tab.take() {
            let tab = Tab::PALETTES
                .iter()
                .find(|(_, n)| *n == name)
                .map(|(t, _)| *t);
            if let Some(at) = tab.and_then(|t| self.dock.find_tab(&t)) {
                let _ = self.dock.set_active_tab(at);
            }
        }
        if let (true, Some(s)) = (self.initial_atlas, self.slots.first()) {
            self.initial_atlas = false;
            if let Err(e) = self.atlas.show(&s.doc) {
                self.log.warn(e);
            }
        }
        // In an animation, `--select` names a parameter whose curve to edit.
        if let (Some(id), Some(s)) = (self.initial_select.as_ref(), self.slots.first_mut()) {
            if let Some(a) = s.doc.anim.as_mut() {
                if let Some((ti, m)) = a.model_track().map(|(i, m)| (i, m.clone())) {
                    if let Some(k) = m
                        .params
                        .iter()
                        .position(|x| keyform_anim::scene::param_id(&x.id) == Some(id.as_str()))
                    {
                        a.selected = Some((ti, crate::animation::AttrRef::Param(k)));
                        a.key = Some(1);
                        a.frame = 40.0;
                        self.initial_select = None;
                    }
                }
            }
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
                })
                .or_else(|| {
                    m.glues
                        .iter()
                        .position(|x| x.info.id == id)
                        .map(Selection::Glue)
                });
            s.doc.select(sel);
            if let Some(t) = self.initial_glue_tool.take() {
                if let Err(e) = s.doc.set_glue_tool(Some(t)) {
                    self.log.warn(e);
                }
            }
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
        // Animation workspace: the timeline under the views (palettes.md §7).
        if let Some(d) = self
            .active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc)
            .filter(|d| d.mode == Mode::Animation && d.anim.is_some())
        {
            let tokens = self.tokens;
            egui::Panel::bottom("timeline")
                .resizable(true)
                .default_size(220.0)
                .show(ui, |ui| crate::animation::timeline_ui(ui, d, &tokens));
            // Space plays / pauses.
            if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                if let Some(a) = d.anim.as_mut() {
                    a.playing = !a.playing;
                }
            }
        }

        let mut closed = Vec::new();
        let mut open_request = false;
        let mut new_request = false;
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
                    new_request: &mut new_request,
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
        let doc = self
            .active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc);
        crate::texture_atlas::dialog(&ctx, &mut self.atlas, doc, &mut self.log);
        if std::mem::take(&mut self.palettes.open_parts_settings) {
            if let Some(d) = self.active.and_then(|i| self.slots.get(i)).map(|s| &s.doc) {
                self.parts_settings.show(d);
            }
        }
        let doc = self
            .active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc);
        crate::parts_settings::dialog(&ctx, &mut self.parts_settings, doc, &mut self.log);
        let doc = self
            .active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc);
        crate::physics_editor::window(&ctx, &mut self.physics_editor, doc, &mut self.log);
        if let Some(d) = self.physics_editor.new_doc.take() {
            self.add_slot(d);
        }
        let doc = self
            .active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc);
        crate::motion_sync_dialog::dialog(&ctx, &mut self.motion_sync, doc, &mut self.log);
        let doc = self
            .active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc);
        crate::generators_ui::sway_dialog(&ctx, &mut self.sway, doc, &mut self.log);
        let doc = self
            .active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc);
        crate::generators_ui::rotate3d_dialog(&ctx, &mut self.rotate3d, doc);
        let doc = self
            .active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc);
        crate::generators_ui::face_dialog(&ctx, &mut self.face, doc);
        let doc = self
            .active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc);
        crate::template_dialog::dialog(&ctx, &mut self.template, doc, &mut self.log);
        let doc = self
            .active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc);
        crate::template_dialog::export_dialog(&ctx, &mut self.template, doc, &mut self.log);
        let doc = self
            .active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc);
        crate::blend_limits_dialog::dialog(&ctx, &mut self.blend_limits, doc);
        let doc = self
            .active
            .and_then(|i| self.slots.get_mut(i))
            .map(|s| &mut s.doc);
        crate::form_dialogs::dialogs(&ctx, &mut self.forms, doc, &mut self.log);
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
                crate::psd_dialog::Outcome::NewModel(path, doc) => {
                    self.add_new_project(&path, *doc);
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
        crate::export_media::dialog(&ctx, &mut self.export_media, doc, &mut self.log);
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
        if new_request {
            self.new_model.open = true;
        }
        if let Some((w, h)) = crate::new_model_dialog::dialog(&ctx, &mut self.new_model) {
            match keyform_formats::psd_import::new_blank_model("Untitled Model", w, h) {
                Ok(doc) => self.add_new_project(std::path::Path::new("Untitled Model.cmo3"), doc),
                Err(e) => self.log.error(e.to_string()),
            }
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
        eframe::set_value(storage, LAYOUT_VERSION_KEY, &LAYOUT_VERSION);
        eframe::set_value(storage, PLAYBACK_KEY, &self.playback);
        eframe::set_value(storage, SETTINGS_KEY, &self.settings);
        // Only approved applications are remembered.
        let kept = crate::external_host::Plugins {
            list: self.plugins.list.iter().filter(|p| p.2).cloned().collect(),
        };
        eframe::set_value(storage, PLUGINS_KEY, &kept);
        eframe::set_value(storage, MS_PRESETS_KEY, &self.motion_sync.user_presets);
        eframe::set_value(storage, EXPORT_IMAGE_KEY, &self.export_image);
    }
}

/// Mirror Edit panel: on/off, axis direction and position.
/// spec: 50-ui/canvas-and-tools.md §5.1 "Mirror Edit"
fn mirror_controls(ui: &mut egui::Ui, s: &mut crate::mesh_edit::MeshEdit, canvas: [f32; 2]) {
    ui.checkbox(&mut s.prefs.mirror_on, crate::i18n::tr("Mirror Edit"));
    let selected = s.centre(true);
    let m = &mut s.prefs.mirror;
    ui.radio_value(
        &mut m.vertical,
        true,
        crate::i18n::tr("Axis: vertical (left ↔ right)"),
    );
    ui.radio_value(
        &mut m.vertical,
        false,
        crate::i18n::tr("Axis: horizontal (top ↔ bottom)"),
    );
    let k = usize::from(!m.vertical);
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Axis"));
        ui.add(egui::DragValue::new(&mut m.axis).speed(1.0).suffix(" px"));
    });
    // The axis starts at the canvas centre; both buttons return it there.
    let reset = ui.button(crate::i18n::tr("Reset Axis Position")).clicked();
    if ui.button(crate::i18n::tr("Center of Canvas")).clicked() || reset {
        m.axis = canvas[k] / 2.0;
    }
    if ui
        .add_enabled(
            selected.is_some(),
            egui::Button::new(crate::i18n::tr("Center of Selected Vertices")),
        )
        .clicked()
    {
        if let Some(c) = selected {
            m.axis = c[k];
        }
    }
}

/// Tool Details palette: the options of the current tool (mesh edit,
/// glue brushes, Arrow / Lasso). spec: 50-ui/palettes.md §5
fn tool_details_ui(ui: &mut egui::Ui, doc: Option<&mut Document>, log: &mut Log, tokens: &Tokens) {
    let Some(doc) = doc else {
        ui.weak(crate::i18n::tr("No model"));
        return;
    };
    let zoom = doc.view.zoom;
    let model = doc.model.clone();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some(s) = doc.mesh_edit.as_mut() {
                use crate::mesh_edit::MeshTool;
                ui.strong(crate::i18n::tr("Mesh edit"));
                ui.horizontal_wrapped(|ui| {
                    for (t, name, tip) in [
                        (
                            MeshTool::Select,
                            "Select",
                            "Select and move vertices (L: lasso)",
                        ),
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
                            .on_hover_text(crate::i18n::tr(tip))
                            .clicked()
                        {
                            s.tool = t;
                        }
                    }
                });
                if s.tool == MeshTool::Brush {
                    ui.horizontal_wrapped(|ui| brush_controls(ui, &mut s.prefs));
                }
                if ui
                    .button(crate::i18n::tr("Connect"))
                    .on_hover_text(crate::i18n::tr("Automatically join vertices (Ctrl+R)"))
                    .clicked()
                {
                    if let Err(e) = s.auto_connect() {
                        log.warn(e);
                    }
                }
                if s.partner.is_some() {
                    ui.horizontal(|ui| {
                        let max_dist = 40.0 / zoom.max(1e-6);
                        if ui
                            .button(crate::i18n::tr("Bind"))
                            .on_hover_text(crate::i18n::tr(
                                "Glue the selected vertices of the two ArtMeshes (Ctrl+G)",
                            ))
                            .clicked()
                        {
                            if let Err(e) = s.bind(false, max_dist, &model) {
                                log.warn(e);
                            }
                        }
                        if ui
                            .button(crate::i18n::tr("Unbind"))
                            .on_hover_text(crate::i18n::tr(
                                "Remove the glue between the selected vertices (Ctrl+Shift+G)",
                            ))
                            .clicked()
                        {
                            if let Err(e) = s.bind(true, max_dist, &model) {
                                log.warn(e);
                            }
                        }
                    });
                    ui.weak(format!(
                        "Gluing with the second ArtMesh: {} pairs to add, {} to remove at Finish.",
                        s.glue_add.len(),
                        s.glue_remove.len()
                    ));
                }
                ui.checkbox(
                    &mut s.prefs.protect_exterior,
                    crate::i18n::tr("Protect Edge exterior"),
                );
                ui.checkbox(
                    &mut s.prefs.reconstruct,
                    crate::i18n::tr("Mesh reconstruction"),
                );
                ui.checkbox(&mut s.prefs.mirror_on, crate::i18n::tr("Mirror"))
                    .on_hover_text(crate::i18n::tr(
                        "Mirror Edit: edits are mirrored across the axis",
                    ));
                if s.prefs.mirror_on {
                    ui.weak(crate::i18n::tr("Mirror Editing in Progress"));
                }
                if s.unresolved > 0 {
                    ui.colored_label(tokens.warning, format!("{} crossing edges", s.unresolved));
                }
                return;
            }
            if doc.glue_tool.is_some() {
                glue_controls(ui, doc, log);
                return;
            }
            if doc.soft_select.is_some() {
                crate::soft_select::controls(ui, doc);
                return;
            }
            if doc.temp_path.is_some() {
                crate::path_tool::temp_controls(ui, doc);
                return;
            }
            if doc.path_tool.is_some() {
                crate::path_tool::controls(ui, doc);
                return;
            }
            if doc.lasso {
                ui.strong(crate::i18n::tr("Lasso Tool"));
                ui.label(crate::i18n::tr(
                    "Drag on empty space to lasso points of the selected objects (Shift adds).",
                ));
            } else {
                ui.strong(crate::i18n::tr("Arrow tool"));
                ui.label(crate::i18n::tr(
                    "Click to select (Shift toggles); drag on empty space for a rectangle.",
                ));
            }
        });
}

/// Mesh-edit brush settings. spec: 50-ui/canvas-and-tools.md §5.1 "Brush"
fn brush_controls(ui: &mut egui::Ui, b: &mut crate::mesh_edit::MeshPrefs) {
    brush_fields(
        ui,
        "brush-shape",
        &mut b.brush_size,
        &mut b.brush_level,
        &mut b.brush_hardness,
        &mut b.brush_angle,
        &mut b.brush_shape,
    );
}

/// Brush size, level, hardness, angle and shape fields.
fn brush_fields(
    ui: &mut egui::Ui,
    id: &str,
    size: &mut f32,
    level: &mut f32,
    hardness: &mut f32,
    angle: &mut f32,
    shape: &mut crate::mesh_edit::BrushShape,
) {
    use crate::mesh_edit::BrushShape;
    ui.add(
        egui::DragValue::new(size)
            .range(2.0..=4000.0)
            .prefix("size ")
            .suffix(" px"),
    );
    ui.add(
        egui::DragValue::new(level)
            .range(0.0..=1.0)
            .speed(0.01)
            .prefix("level "),
    );
    ui.add(
        egui::DragValue::new(hardness)
            .range(0.0..=1.0)
            .speed(0.01)
            .prefix("hardness "),
    );
    ui.add(
        egui::DragValue::new(angle)
            .range(-180.0..=180.0)
            .prefix("angle ")
            .suffix("°"),
    );
    egui::ComboBox::from_id_salt(id)
        .selected_text(format!("{shape:?}"))
        .show_ui(ui, |ui| {
            for s in [BrushShape::Circle, BrushShape::Square, BrushShape::Line] {
                ui.selectable_value(shape, s, format!("{s:?}"));
            }
        });
}

/// Glue tool bar: the tools, brush settings, and "Glue Weight A : B" of
/// the painted glue. spec: 30-algorithms/glue.md "Weight brushes"
fn glue_controls(ui: &mut egui::Ui, doc: &mut Document, log: &mut Log) {
    use keyform_edit::glue_brush::{ui_weights, GlueBrush};
    let Some(cur) = doc.glue_tool.as_ref().map(|t| t.tool) else {
        return;
    };
    ui.weak(crate::i18n::tr("Glue:"));
    let mut chosen = None;
    for t in GlueBrush::ALL {
        if ui
            .add(egui::Button::selectable(cur == t, t.label()))
            .clicked()
        {
            chosen = Some(Some(t));
        }
    }
    let p = &mut doc.glue_prefs;
    ui.horizontal_wrapped(|ui| {
        brush_fields(
            ui,
            "glue-brush-shape",
            &mut p.size,
            &mut p.level,
            &mut p.hardness,
            &mut p.angle,
            &mut p.shape,
        )
    });
    ui.add(
        egui::DragValue::new(&mut p.display_opacity)
            .range(0.0..=1.0)
            .speed(0.01)
            .prefix("display "),
    );
    match ui_weights(&doc.model, &doc.glue_targets()) {
        Some([a, b]) => ui.label(format!("Glue Weight A : B  {a:.2} : {b:.2}")),
        None => ui.weak(crate::i18n::tr("Glue Weight A : B  —")),
    };
    if ui
        .button(crate::i18n::tr("Done"))
        .on_hover_text(crate::i18n::tr("Back to the Arrow tool"))
        .clicked()
    {
        chosen = Some(None);
    }
    if let Some(t) = chosen {
        if let Err(e) = doc.set_glue_tool(t) {
            log.warn(e);
        }
    }
}

/// Modeling ▸ Physics Settings needs an editable model in Modeling.
/// spec: 70-advanced/physics-editor.md
fn doc_ok_for_physics(app: &KeyformApp) -> bool {
    app.active
        .and_then(|i| app.slots.get(i))
        .is_some_and(|s| s.doc.editable() && s.doc.mode == Mode::Modeling)
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
