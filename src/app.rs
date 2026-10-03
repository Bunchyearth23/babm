use crate::bess::{self, BessUpdate, UpdateStatus};
use crate::grouper::{ChassisGroup, Grouper};
use crate::merger::Merger;
use crate::paths::DetectedPaths;
use crate::scanner;
use crate::vehicle::VehicleMod;
use eframe::egui::{self, Color32, RichText, TextureHandle};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

fn bess_open_command(
    executable: &Path,
    source: &Path,
    mods_dir: &Path,
    exports_dir: Option<&Path>,
) -> std::process::Command {
    let mut command = std::process::Command::new(executable);
    command
        .arg("--open")
        .arg(source)
        .arg("--beamng-mods")
        .arg(mods_dir);
    if let Some(exports_dir) = exports_dir {
        command.arg("--bess-exports").arg(exports_dir);
    }
    command
}

fn open_in_bess(source: &Path, mods_dir: &Path, exports_dir: Option<&Path>) -> Result<(), String> {
    if !source.is_file() {
        return Err(format!(
            "The original archive is missing: {}",
            source.display()
        ));
    }
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe()
        && let Some(parent) = executable.parent()
    {
        candidates.push(parent.join("BESS.exe"));
    }
    if let Some(desktop) = dirs::desktop_dir() {
        candidates.push(desktop.join("BESS.exe"));
    }
    let executable = candidates.into_iter().find(|path| path.is_file())
        .ok_or_else(|| "BESS.exe was not found. Put it beside BABM.exe or on your desktop; use Show original to open the archive manually.".to_string())?;
    bess_open_command(&executable, source, mods_dir, exports_dir)
        .spawn()
        .map_err(|error| format!("Cannot open BESS: {error}"))?;
    Ok(())
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum AppTab {
    Chassis,
    Vehicles,
    Bess,
}

pub enum TaskResult {
    MergeSuccess {
        chassis_name: String,
        path: PathBuf,
    },
    MergeError {
        chassis_name: String,
        error: String,
    },
    UnmergeSuccess {
        chassis_name: String,
    },
    UnmergeError {
        chassis_name: String,
        error: String,
    },
    BessScan {
        generation: u64,
        result: Result<Vec<BessUpdate>, String>,
    },
    BessApplied {
        result: Result<PathBuf, String>,
    },
}

pub struct BabmApp {
    paths: DetectedPaths,
    active_dir: Option<PathBuf>,
    vehicles: Vec<VehicleMod>,
    groups: Vec<ChassisGroup>,
    selected_index: Option<usize>,
    selected_chassis_index: Option<usize>,
    filter_search: String,
    filter_automation_only: bool,
    textures: HashMap<PathBuf, TextureHandle>,
    active_tab: AppTab,
    status_message: Option<(String, bool)>, // (message, is_error)
    task_tx: Sender<TaskResult>,
    task_rx: Receiver<TaskResult>,
    running_chassis_tasks: HashSet<String>,
    bess_updates: Vec<BessUpdate>,
    bess_exports_dir: Option<PathBuf>,
    bess_scan_generation: u64,
    bess_scanning: bool,
    bess_applying: bool,
    bess_scan_error: Option<String>,
    bess_sources: HashMap<PathBuf, Result<Vec<PathBuf>, String>>,
}

impl BabmApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::with_paths(DetectedPaths::detect())
    }

    pub fn new_with_options(
        _cc: &eframe::CreationContext<'_>,
        path: Option<PathBuf>,
        exports: Option<PathBuf>,
    ) -> Self {
        Self::with_options(DetectedPaths::detect(), path, exports)
    }

    fn with_paths(paths: DetectedPaths) -> Self {
        Self::with_options(paths, None, None)
    }

    fn with_options(paths: DetectedPaths, path: Option<PathBuf>, exports: Option<PathBuf>) -> Self {
        let default_dir = path.or_else(|| paths.beamng_mods_dirs.first().cloned());
        let active_tab = if exports.is_some() {
            AppTab::Bess
        } else {
            AppTab::Chassis
        };
        let (task_tx, task_rx) = mpsc::channel();

        let mut app = Self {
            paths,
            active_dir: default_dir.clone(),
            vehicles: Vec::new(),
            groups: Vec::new(),
            selected_index: None,
            selected_chassis_index: None,
            filter_search: String::new(),
            filter_automation_only: false,
            textures: HashMap::new(),
            active_tab,
            status_message: None,
            task_tx,
            task_rx,
            running_chassis_tasks: HashSet::new(),
            bess_updates: Vec::new(),
            bess_exports_dir: exports,
            bess_scan_generation: 0,
            bess_scanning: false,
            bess_applying: false,
            bess_scan_error: None,
            bess_sources: HashMap::new(),
        };

        if let Some(ref dir) = default_dir {
            app.load_directory(dir.clone());
        }

        app
    }

    pub fn load_directory(&mut self, dir: PathBuf) {
        self.active_dir = Some(dir.clone());
        self.vehicles = scanner::scan_directory(&dir);
        self.groups = Grouper::group_vehicles(&self.vehicles, &self.paths);
        self.bess_sources = self
            .vehicles
            .iter()
            .map(|vehicle| {
                (
                    vehicle.file_path.clone(),
                    bess::sources_for_bess(&dir, &vehicle.file_path),
                )
            })
            .collect();
        self.selected_index = if self.vehicles.is_empty() {
            None
        } else {
            Some(0)
        };
        self.selected_chassis_index = if self.groups.is_empty() {
            None
        } else {
            Some(0)
        };
        self.textures.clear();
        self.refresh_bess_exports();
    }

    fn refresh_bess_exports(&mut self) {
        self.bess_scan_generation = self.bess_scan_generation.wrapping_add(1);
        self.bess_updates.clear();
        self.bess_scan_error = None;
        let Some(mods_dir) = self.active_dir.clone() else {
            return;
        };
        let exports_dir = self.bess_exports_dir.clone();
        let generation = self.bess_scan_generation;
        let tx = self.task_tx.clone();
        self.bess_scanning = true;
        std::thread::spawn(move || {
            let result = bess::discover(&mods_dir, exports_dir.as_deref());
            let _ = tx.send(TaskResult::BessScan { generation, result });
        });
    }

    fn accept_bess_scan(&mut self, generation: u64, result: Result<Vec<BessUpdate>, String>) {
        if generation != self.bess_scan_generation {
            return;
        }
        self.bess_scanning = false;
        match result {
            Ok(updates) => {
                self.bess_updates = updates;
                self.bess_scan_error = None;
            }
            Err(error) => {
                self.bess_updates.clear();
                self.bess_scan_error = Some(error);
            }
        }
    }

    fn can_apply_bess_update(&self, update: &BessUpdate) -> bool {
        self.active_dir.is_some()
            && !self.bess_applying
            && !self.bess_scanning
            && self.running_chassis_tasks.is_empty()
            && update.status == UpdateStatus::Ready
    }

    fn apply_bess_export(&mut self, update: BessUpdate) {
        if !self.can_apply_bess_update(&update) {
            return;
        }
        let Some(mods_dir) = self.active_dir.clone() else {
            return;
        };
        let tx = self.task_tx.clone();
        self.bess_applying = true;
        self.status_message = Some((
            format!("Applying BESS sounds for {}...", update.vehicle_name),
            false,
        ));
        std::thread::spawn(move || {
            let result = bess::apply_update(&mods_dir, &update.export_path)
                .map(|receipt| receipt.target_path);
            let _ = tx.send(TaskResult::BessApplied { result });
        });
    }
}

impl eframe::App for BabmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll background task results
        let mut should_reload = false;
        while let Ok(msg) = self.task_rx.try_recv() {
            match msg {
                TaskResult::MergeSuccess { chassis_name, path } => {
                    let slug = crate::grouper::slugify(&chassis_name);
                    self.running_chassis_tasks.remove(&slug);
                    let filename = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    self.status_message = Some((
                        format!(
                            "✅ Successfully merged '{}'! File: {}",
                            chassis_name, filename
                        ),
                        false,
                    ));
                    should_reload = true;
                }
                TaskResult::MergeError {
                    chassis_name,
                    error,
                } => {
                    let slug = crate::grouper::slugify(&chassis_name);
                    self.running_chassis_tasks.remove(&slug);
                    self.status_message = Some((
                        format!("❌ Merge failed for '{}': {}", chassis_name, error),
                        true,
                    ));
                }
                TaskResult::UnmergeSuccess { chassis_name } => {
                    let slug = crate::grouper::slugify(&chassis_name);
                    self.running_chassis_tasks.remove(&slug);
                    self.status_message = Some((
                        format!(
                            "✅ Chassis '{}' unmerged and original variants restored!",
                            chassis_name
                        ),
                        false,
                    ));
                    should_reload = true;
                }
                TaskResult::UnmergeError {
                    chassis_name,
                    error,
                } => {
                    let slug = crate::grouper::slugify(&chassis_name);
                    self.running_chassis_tasks.remove(&slug);
                    self.status_message = Some((
                        format!("❌ Unmerge failed for '{}': {}", chassis_name, error),
                        true,
                    ));
                }
                TaskResult::BessScan { generation, result } => {
                    self.accept_bess_scan(generation, result)
                }
                TaskResult::BessApplied { result } => {
                    self.bess_applying = false;
                    match result {
                        Ok(path) => {
                            self.status_message = Some((
                                format!(
                                    "BESS sounds applied. Backup preserved. {}",
                                    path.display()
                                ),
                                false,
                            ));
                            should_reload = true;
                        }
                        Err(error) => {
                            self.status_message =
                                Some((format!("BESS update failed: {error}"), true));
                            self.refresh_bess_exports();
                        }
                    }
                }
            }
        }

        if should_reload && let Some(dir) = self.active_dir.clone() {
            self.load_directory(dir);
        }

        // Request repaint while tasks are running for smooth spinner animation and polling
        if !self.running_chassis_tasks.is_empty() || self.bess_scanning || self.bess_applying {
            ctx.request_repaint_after(Duration::from_millis(60));
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(
                    RichText::new("🚗 BABM")
                        .strong()
                        .color(Color32::from_rgb(255, 170, 0)),
                );
                ui.label(
                    RichText::new("— Bunchy's Automation BeamNG Management")
                        .italics()
                        .color(Color32::LIGHT_GRAY),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            !self.bess_applying && self.running_chassis_tasks.is_empty(),
                            egui::Button::new("📁 Browse folder..."),
                        )
                        .clicked()
                        && let Some(folder) = rfd::FileDialog::new().pick_folder()
                    {
                        self.load_directory(folder);
                    }

                    if ui
                        .add_enabled(
                            !self.bess_applying && self.running_chassis_tasks.is_empty(),
                            egui::Button::new("🔄 Refresh"),
                        )
                        .clicked()
                        && let Some(dir) = self.active_dir.clone()
                    {
                        self.load_directory(dir);
                    }

                    if let Some(ref dir) = self.active_dir
                        && ui.button("📂 Open folder").clicked()
                    {
                        let _ = std::process::Command::new("explorer").arg(dir).spawn();
                    }
                });
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let mut dir_to_load = None;
                if !self.paths.beamng_mods_dirs.is_empty() {
                    let selected_name = self
                        .active_dir
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "Select a folder...".to_string());

                    egui::ComboBox::from_label("Active folder")
                        .selected_text(selected_name)
                        .show_ui(ui, |ui| {
                            for path in &self.paths.beamng_mods_dirs {
                                let label = path.display().to_string();
                                let is_selected = self.active_dir.as_ref() == Some(path);
                                if ui
                                    .add_enabled(
                                        !self.bess_applying
                                            && self.running_chassis_tasks.is_empty(),
                                        egui::Button::selectable(is_selected, &label),
                                    )
                                    .clicked()
                                {
                                    dir_to_load = Some(path.clone());
                                }
                            }
                        });
                } else {
                    let current_dir_text = self
                        .active_dir
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "No folder selected".to_string());
                    ui.label(
                        RichText::new(format!("Active folder: {current_dir_text}"))
                            .small()
                            .color(Color32::GRAY),
                    );
                }

                if let Some(dir) = dir_to_load {
                    self.load_directory(dir);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !self.running_chassis_tasks.is_empty() {
                        ui.spinner();
                        ui.colored_label(
                            Color32::from_rgb(255, 170, 0),
                            format!(
                                "⚡ {} background task(s)...",
                                self.running_chassis_tasks.len()
                            ),
                        );
                        ui.separator();
                    }
                    ui.label(
                        RichText::new(format!(
                            "{} chassis | {} mods detected",
                            self.groups.len(),
                            self.vehicles.len()
                        ))
                        .strong(),
                    );
                });
            });

            ui.separator();

            // Navigation tabs & search
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.active_tab,
                    AppTab::Chassis,
                    "🏎 Chassis View (Merge / Unmerge)",
                );
                ui.selectable_value(&mut self.active_tab, AppTab::Vehicles, "📋 All Vehicles");
                ui.selectable_value(
                    &mut self.active_tab,
                    AppTab::Bess,
                    format!(
                        "BESS sounds ({})",
                        self.bess_updates
                            .iter()
                            .filter(|u| u.status == UpdateStatus::Ready)
                            .count()
                    ),
                );

                ui.separator();

                ui.label("🔍 Search:");
                ui.text_edit_singleline(&mut self.filter_search);

                ui.checkbox(&mut self.filter_automation_only, "Automation only");

                if !self.filter_search.is_empty() && ui.button("✖").clicked() {
                    self.filter_search.clear();
                }
            });

            // Status message banner
            if let Some((ref msg, is_err)) = self.status_message {
                ui.add_space(4.0);
                let col = if is_err {
                    Color32::LIGHT_RED
                } else {
                    Color32::LIGHT_GREEN
                };
                ui.colored_label(col, msg);
            }

            ui.add_space(4.0);
        });

        match self.active_tab {
            AppTab::Chassis => self.render_chassis_tab(ctx),
            AppTab::Vehicles => self.render_vehicles_tab(ctx),
            AppTab::Bess => self.render_bess_tab(ctx),
        }
    }
}

impl BabmApp {
    fn render_bess_tab(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("BESS sound updates");
            ui.label("Edit an original variant in BESS, export its complete vehicle ZIP, then apply its sounds here.");
            ui.label("Grouped vehicles keep their other variants and settings. A backup is kept before each update.");
            ui.label("The original trim receives these sounds. Older BESS add-on configurations keep their own sounds.");
            ui.horizontal(|ui| {
                let label = self.bess_exports_dir.as_ref().map(|p| p.display().to_string())
                    .unwrap_or_else(|| "Automatic: BESS saved export folder and nearby BESS-exports".to_string());
                ui.label(label);
            });
            let idle = !self.bess_applying && self.running_chassis_tasks.is_empty();
            ui.horizontal(|ui| {
                if ui.add_enabled(idle && !self.bess_scanning, egui::Button::new("Refresh BESS exports")).clicked() {
                    self.refresh_bess_exports();
                }
                if ui.add_enabled(idle, egui::Button::new("Choose BESS exports folder...")).clicked()
                    && let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    self.bess_exports_dir = Some(folder);
                    self.refresh_bess_exports();
                }
                if ui.add_enabled(idle && self.bess_exports_dir.is_some(), egui::Button::new("Automatic folders")).clicked() {
                    self.bess_exports_dir = None;
                    self.refresh_bess_exports();
                }
            });
            if self.bess_scanning {
                ui.horizontal(|ui| { ui.spinner(); ui.label("Checking BESS exports and their targets..."); });
            }
            if self.bess_applying {
                ui.horizontal(|ui| { ui.spinner(); ui.label("Applying sounds and preserving the previous archive..."); });
            }
            if let Some(error) = &self.bess_scan_error {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
            ui.separator();
            let updates: Vec<_> = self.bess_updates.iter().filter(|update| {
                self.filter_search.is_empty() || update.vehicle_name.to_lowercase().contains(&self.filter_search.to_lowercase())
            }).cloned().collect();
            egui::ScrollArea::vertical().show(ui, |ui| {
                if updates.is_empty() && !self.bess_scanning {
                    ui.label("No BESS exports found here. Export a complete vehicle ZIP from BESS, then refresh.");
                }
                for update in updates {
                    ui.push_id(&update.export_path, |ui| {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&update.vehicle_name).strong());
                                let (label, color) = match update.status {
                                    UpdateStatus::Ready => ("Ready to apply", Color32::LIGHT_GREEN),
                                    UpdateStatus::AlreadyApplied => ("Already applied", Color32::GRAY),
                                    UpdateStatus::Stale => ("Older export", Color32::GRAY),
                                    UpdateStatus::Conflict => ("Needs attention", Color32::LIGHT_RED),
                                    UpdateStatus::Unavailable => ("Target unavailable", Color32::YELLOW),
                                };
                                ui.colored_label(color, label);
                                if ui.add_enabled(self.can_apply_bess_update(&update), egui::Button::new("Apply BESS sounds")).clicked() {
                                    self.apply_bess_export(update.clone());
                                }
                            });
                            ui.label(&update.detail);
                            ui.label(format!("{} sound files", update.sounds));
                            if let Some(target) = &update.target_path { ui.label(format!("Vehicle to update: {}", target.display())); }
                            ui.label(RichText::new(update.export_path.display().to_string()).small());
                        });
                    });
                    ui.add_space(6.0);
                }
            });
        });
    }

    fn render_bess_sources(&mut self, ui: &mut egui::Ui, vehicle: &VehicleMod) {
        let Some(sources) = self.bess_sources.get(&vehicle.file_path).cloned() else {
            return;
        };
        match sources {
            Ok(sources) => {
                for source in sources {
                    ui.horizontal(|ui| {
                        let name = source.file_name().unwrap_or_default().to_string_lossy();
                        ui.label(format!("Original for BESS: {name}"));
                        let idle = !self.bess_applying && self.running_chassis_tasks.is_empty();
                        if ui.add_enabled(idle, egui::Button::new("Edit sound in BESS")).clicked() && let Some(mods_dir) = self.active_dir.as_deref() {
                            self.status_message = Some(match open_in_bess(&source, mods_dir, self.bess_exports_dir.as_deref()) {
                                Ok(()) => ("Original opened in BESS. Export the complete ZIP, then use the BESS sounds tab.".to_string(), false),
                                Err(error) => (error, true),
                            });
                        }
                        if ui.button("Show original").clicked() {
                            let _ = std::process::Command::new("explorer").arg(format!("/select,{}", source.display())).spawn();
                        }
                    });
                }
            }
            Err(error) => {
                ui.label(
                    RichText::new(format!("BESS source unavailable: {error}"))
                        .small()
                        .color(Color32::GRAY),
                );
            }
        }
    }

    fn render_chassis_tab(&mut self, ctx: &egui::Context) {
        let filtered_indices: Vec<usize> = self
            .groups
            .iter()
            .enumerate()
            .filter(|(_, g)| {
                if !self.filter_search.is_empty() {
                    let search = self.filter_search.to_lowercase();
                    let match_chassis = g.chassis_name.to_lowercase().contains(&search);
                    let match_variant = g
                        .variants
                        .iter()
                        .any(|v| v.display_name.to_lowercase().contains(&search));
                    if !(match_chassis || match_variant) {
                        return false;
                    }
                }
                true
            })
            .map(|(i, _)| i)
            .collect();

        egui::SidePanel::left("chassis_list_panel")
            .default_width(330.0)
            .width_range(240.0..=480.0)
            .show(ctx, |ui| {
                ui.heading(RichText::new("Chassis & Families").size(16.0));
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    if filtered_indices.is_empty() {
                        ui.label(RichText::new("No chassis found.").italics());
                    }

                    for &idx in &filtered_indices {
                        let g = &self.groups[idx];
                        let is_selected = self.selected_chassis_index == Some(idx);
                        let is_running = self.running_chassis_tasks.contains(&g.chassis_slug);

                        let icon = if is_running {
                            "⏳"
                        } else if g.is_merged {
                            "📦"
                        } else {
                            "🚗"
                        };

                        let label = format!(
                            "{} {} ({} var.){}",
                            icon,
                            g.chassis_name,
                            g.variants.len(),
                            if is_running { " [in progress...]" } else { "" }
                        );

                        if ui.selectable_label(is_selected, label).clicked() {
                            self.selected_chassis_index = Some(idx);
                        }
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(idx) = self.selected_chassis_index {
                if let Some(g) = self.groups.get(idx).cloned() {
                    let is_running =
                        self.bess_applying || self.running_chassis_tasks.contains(&g.chassis_slug);

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.heading(RichText::new(&g.chassis_name).strong().size(22.0));
                            if is_running {
                                ui.colored_label(
                                    Color32::from_rgb(255, 170, 0),
                                    "⏳ Processing in background...",
                                );
                            } else if g.is_merged {
                                ui.colored_label(Color32::from_rgb(0, 200, 100), "✔ Merged Mod");
                            } else {
                                ui.colored_label(
                                    Color32::from_rgb(255, 170, 0),
                                    "Separate variants",
                                );
                            }
                        });

                        ui.label(format!("BeamNG Slug: vehicles/{}/", g.chassis_slug));

                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if let Some(ref mods_dir) = self.active_dir {
                                if is_running {
                                    ui.spinner();
                                    ui.label(
                                        RichText::new(
                                            "Merge / Operation in progress in background thread...",
                                        )
                                        .color(Color32::from_rgb(255, 170, 0))
                                        .italics(),
                                    );
                                } else if !g.is_merged {
                                    let btn = ui.button(
                                        RichText::new("⚡ Merge variants into 1 BeamNG vehicle")
                                            .strong()
                                            .color(Color32::WHITE),
                                    );
                                    if btn.clicked() {
                                        let slug = g.chassis_slug.clone();
                                        self.running_chassis_tasks.insert(slug);
                                        self.status_message = Some((
                                            format!(
                                                "⏳ Merging '{}' in background...",
                                                g.chassis_name
                                            ),
                                            false,
                                        ));

                                        let tx = self.task_tx.clone();
                                        let c_name = g.chassis_name.clone();
                                        let c_vars = g.variants.clone();
                                        let m_dir = mods_dir.clone();

                                        std::thread::spawn(move || {
                                            match Merger::merge_variants(&c_name, &c_vars, &m_dir) {
                                                Ok(path) => {
                                                    let _ = tx.send(TaskResult::MergeSuccess {
                                                        chassis_name: c_name,
                                                        path,
                                                    });
                                                }
                                                Err(error) => {
                                                    let _ = tx.send(TaskResult::MergeError {
                                                        chassis_name: c_name,
                                                        error,
                                                    });
                                                }
                                            }
                                        });
                                    }
                                } else {
                                    let btn = ui.button(
                                        RichText::new("↩ Unmerge (Restore separate originals)")
                                            .color(Color32::LIGHT_RED),
                                    );
                                    if btn.clicked() {
                                        let slug = g.chassis_slug.clone();
                                        self.running_chassis_tasks.insert(slug);
                                        self.status_message = Some((
                                            format!(
                                                "⏳ Unmerging '{}' in background...",
                                                g.chassis_name
                                            ),
                                            false,
                                        ));

                                        let tx = self.task_tx.clone();
                                        let c_name = g.chassis_name.clone();
                                        let m_dir = mods_dir.clone();

                                        std::thread::spawn(move || {
                                            match Merger::unmerge_chassis(&c_name, &m_dir) {
                                                Ok(()) => {
                                                    let _ = tx.send(TaskResult::UnmergeSuccess {
                                                        chassis_name: c_name,
                                                    });
                                                }
                                                Err(error) => {
                                                    let _ = tx.send(TaskResult::UnmergeError {
                                                        chassis_name: c_name,
                                                        error,
                                                    });
                                                }
                                            }
                                        });
                                    }
                                }
                            }
                        });

                        ui.separator();
                        ui.heading(
                            RichText::new(format!("Included variants ({})", g.variants.len()))
                                .size(16.0),
                        );

                        for v in &g.variants {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&v.display_name).strong().size(15.0));
                                    ui.monospace(format!("({})", v.file_name));
                                });

                                if let Some(cfg) = v.main_config() {
                                    ui.horizontal(|ui| {
                                        if let Some(p) = cfg.power_hp {
                                            ui.label(format!("⚡ {:.0} hp", p));
                                        }
                                        if let Some(t) = cfg.torque_nm {
                                            ui.label(format!("| 🔧 {:.0} Nm", t));
                                        }
                                        if let Some(w) = cfg.weight_kg {
                                            ui.label(format!("| ⚖ {:.0} kg", w));
                                        }
                                        if let Some(dt) = &cfg.drivetrain {
                                            ui.label(format!("| ⚙ {}", dt));
                                        }
                                        if let Some(tr) = &cfg.transmission {
                                            ui.label(format!("| 🕹 {}", tr));
                                        }
                                    });
                                }
                                self.render_bess_sources(ui, v);
                            });
                            ui.add_space(4.0);
                        }
                    });
                }
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(50.0);
                    ui.label(RichText::new("Select a chassis to manage its variants").italics());
                });
            }
        });
    }

    fn render_vehicles_tab(&mut self, ctx: &egui::Context) {
        let filtered_indices: Vec<usize> = self
            .vehicles
            .iter()
            .enumerate()
            .filter(|(_, v)| {
                if self.filter_automation_only && !v.is_automation {
                    return false;
                }
                if !self.filter_search.is_empty() {
                    let search = self.filter_search.to_lowercase();
                    let match_name = v.display_name.to_lowercase().contains(&search);
                    let match_internal = v.internal_name.to_lowercase().contains(&search);
                    let match_author = v.author.to_lowercase().contains(&search);
                    let match_config = v
                        .configs
                        .iter()
                        .any(|c| c.name.to_lowercase().contains(&search));
                    if !(match_name || match_internal || match_author || match_config) {
                        return false;
                    }
                }
                true
            })
            .map(|(i, _)| i)
            .collect();

        egui::SidePanel::left("vehicles_list_panel")
            .default_width(320.0)
            .width_range(240.0..=480.0)
            .show(ctx, |ui| {
                ui.heading(RichText::new("Vehicles").size(16.0));
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    if filtered_indices.is_empty() {
                        ui.label(RichText::new("No vehicles found.").italics());
                    }

                    for &idx in &filtered_indices {
                        let v = &self.vehicles[idx];
                        let is_selected = self.selected_index == Some(idx);

                        let response = ui.selectable_label(
                            is_selected,
                            format!(
                                "{} {}{}",
                                if v.is_automation { "🏎" } else { "🚗" },
                                v.display_name,
                                if v.configs.len() > 1 {
                                    format!(" ({} configs)", v.configs.len())
                                } else {
                                    String::new()
                                }
                            ),
                        );

                        if response.clicked() {
                            self.selected_index = Some(idx);
                        }
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(idx) = self.selected_index {
                if let Some(v) = self.vehicles.get(idx) {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.heading(RichText::new(&v.display_name).strong().size(22.0));
                            if v.is_automation {
                                ui.colored_label(
                                    Color32::from_rgb(255, 140, 0),
                                    "⚡ Automation Mod",
                                );
                            }
                        });

                        ui.label(
                            RichText::new(format!("Internal name: {}", v.internal_name))
                                .monospace()
                                .small(),
                        );
                        ui.label(RichText::new(format!("Author: {}", v.author)).small());

                        ui.separator();

                        // Thumbnail
                        if let Some(ref png_bytes) = v.thumbnail_png {
                            if !self.textures.contains_key(&v.file_path)
                                && let Ok(img) = image::load_from_memory(png_bytes)
                            {
                                let size = [img.width() as usize, img.height() as usize];
                                let rgba = img.to_rgba8();
                                let color_image =
                                    egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                                let tex = ctx.load_texture(
                                    format!("thumb_{}", v.internal_name),
                                    color_image,
                                    egui::TextureOptions::LINEAR,
                                );
                                self.textures.insert(v.file_path.clone(), tex);
                            }

                            if let Some(tex) = self.textures.get(&v.file_path) {
                                let max_w = 400.0;
                                let aspect = tex.aspect_ratio();
                                let size = egui::vec2(max_w, max_w / aspect);
                                ui.image((tex.id(), size));
                                ui.separator();
                            }
                        }

                        // Engine specs
                        if let Some(ref engine) = v.engine {
                            ui.group(|ui| {
                                ui.heading(RichText::new("⚙ Engine (JBeam Extraction)").size(15.0));
                                ui.horizontal(|ui| {
                                    if let Some(cyl) = engine.cylinders {
                                        ui.label(format!("Cylinders: {cyl}"));
                                    }
                                    if let Some(idle) = engine.idle_rpm {
                                        ui.label(format!("| Idle: {:.0} RPM", idle));
                                    }
                                    if let Some(max) = engine.max_rpm {
                                        ui.label(format!("| Redline: {:.0} RPM", max));
                                    }
                                });
                            });
                            ui.add_space(6.0);
                        }

                        // Configurations
                        ui.heading(
                            RichText::new(format!("Configurations ({})", v.configs.len()))
                                .size(16.0),
                        );
                        for cfg in &v.configs {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&cfg.name).strong());
                                    if let Some(drivetrain) = &cfg.drivetrain {
                                        ui.colored_label(Color32::LIGHT_BLUE, drivetrain);
                                    }
                                    if let Some(trans) = &cfg.transmission {
                                        ui.colored_label(Color32::LIGHT_GREEN, trans);
                                    }
                                });

                                ui.columns(3, |cols| {
                                    cols[0].label(format!(
                                        "Power: {}",
                                        cfg.power_hp
                                            .map(|p| format!("{:.0} hp", p))
                                            .unwrap_or_else(|| "N/A".into())
                                    ));
                                    cols[0].label(format!(
                                        "Torque: {}",
                                        cfg.torque_nm
                                            .map(|t| format!("{:.0} Nm", t))
                                            .unwrap_or_else(|| "N/A".into())
                                    ));

                                    cols[1].label(format!(
                                        "Weight: {}",
                                        cfg.weight_kg
                                            .map(|w| format!("{:.0} kg", w))
                                            .unwrap_or_else(|| "N/A".into())
                                    ));
                                    cols[1].label(format!(
                                        "0-100 km/h: {}",
                                        cfg.accel_0_100
                                            .map(|a| format!("{:.1} s", a))
                                            .unwrap_or_else(|| "N/A".into())
                                    ));

                                    cols[2].label(format!(
                                        "Top speed: {}",
                                        cfg.top_speed_kmh
                                            .map(|s| format!("{:.0} km/h", s))
                                            .unwrap_or_else(|| "N/A".into())
                                    ));
                                    cols[2].label(format!(
                                        "Fuel: {}",
                                        cfg.fuel_type.as_deref().unwrap_or("N/A")
                                    ));
                                });
                            });
                            ui.add_space(4.0);
                        }

                        ui.separator();
                        // File details
                        ui.label(RichText::new("Mod file:").strong());
                        ui.label(
                            RichText::new(v.file_path.display().to_string())
                                .monospace()
                                .small(),
                        );
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "Size: {:.2} MB",
                                v.file_size_bytes as f64 / (1024.0 * 1024.0)
                            ));
                            if ui.button("Show in Explorer").clicked() {
                                let _ = std::process::Command::new("explorer")
                                    .arg(format!("/select,{}", v.file_path.display()))
                                    .spawn();
                            }
                        });
                    });
                }
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(50.0);
                    ui.label(
                        RichText::new("Select a vehicle from the list to view its details")
                            .italics(),
                    );
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_bess_carries_selected_mods_and_export_folder_as_separate_arguments() {
        let command = bess_open_command(
            Path::new("BESS.exe"),
            Path::new("original backup.zip"),
            Path::new("custom mods"),
            Some(Path::new("custom exports")),
        );
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--open",
                "original backup.zip",
                "--beamng-mods",
                "custom mods",
                "--bess-exports",
                "custom exports"
            ]
        );
        let command = bess_open_command(
            Path::new("BESS.exe"),
            Path::new("source.zip"),
            Path::new("mods"),
            None,
        );
        assert_eq!(command.get_args().count(), 4);
    }

    fn update(name: &str) -> BessUpdate {
        BessUpdate {
            export_path: PathBuf::from(format!("{name}.zip")),
            vehicle_name: name.to_string(),
            target_path: None,
            exported_at_unix_ms: 1,
            sounds: 2,
            status: UpdateStatus::Ready,
            detail: String::new(),
        }
    }

    #[test]
    fn late_bess_scan_cannot_replace_a_newer_folder_selection() {
        let mut app = BabmApp::with_paths(DetectedPaths::default());
        app.bess_scan_generation = 2;
        app.bess_scanning = true;
        app.accept_bess_scan(1, Ok(vec![update("old-folder")]));
        assert!(app.bess_scanning);
        assert!(app.bess_updates.is_empty());
        app.accept_bess_scan(2, Ok(vec![update("selected-folder")]));
        assert!(!app.bess_scanning);
        assert_eq!(app.bess_updates[0].vehicle_name, "selected-folder");
        app.accept_bess_scan(1, Err("late failure".to_string()));
        assert!(app.bess_scan_error.is_none());
        assert_eq!(app.bess_updates[0].vehicle_name, "selected-folder");
    }

    #[test]
    fn bess_apply_does_not_start_during_merge_or_scan() {
        let mut app = BabmApp::with_paths(DetectedPaths::default());
        // A present directory makes this exercise the operation guard, not a missing selection.
        app.active_dir = Some(std::env::temp_dir());
        app.running_chassis_tasks.insert("active-merge".to_string());
        app.apply_bess_export(update("blocked-during-merge"));
        assert!(!app.bess_applying);
        app.running_chassis_tasks.clear();
        app.bess_scanning = true;
        app.apply_bess_export(update("blocked-during-scan"));
        assert!(!app.bess_applying);
        assert!(app.status_message.is_none());
    }

    #[test]
    fn bess_tab_shows_each_update_state_and_only_ready_updates_can_apply() {
        let mut app = BabmApp::with_paths(DetectedPaths::default());
        app.active_dir = Some(PathBuf::from("mods"));
        for (index, status) in [
            UpdateStatus::Ready,
            UpdateStatus::AlreadyApplied,
            UpdateStatus::Stale,
            UpdateStatus::Conflict,
            UpdateStatus::Unavailable,
        ]
        .into_iter()
        .enumerate()
        {
            let mut item = update(&format!("vehicle-{index}"));
            item.status = status;
            item.target_path = Some(PathBuf::from(format!("mods/target-{index}.zip")));
            assert_eq!(app.can_apply_bess_update(&item), index == 0);
            app.bess_updates.push(item);
        }
        let context = egui::Context::default();
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 1200.0),
                )),
                ..Default::default()
            },
            |ctx| app.render_bess_tab(ctx),
        );
        fn texts(shape: &egui::Shape, out: &mut String) {
            match shape {
                egui::Shape::Text(text) => {
                    out.push_str(&text.galley.job.text);
                    out.push('\n');
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        texts(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut rendered = String::new();
        for shape in output.shapes {
            texts(&shape.shape, &mut rendered);
        }
        for label in [
            "BESS sound updates",
            "Ready to apply",
            "Already applied",
            "Older export",
            "Needs attention",
            "Target unavailable",
        ] {
            assert!(rendered.contains(label), "Missing {label}: {rendered}");
        }
        assert_eq!(rendered.matches("Apply BESS sounds").count(), 5);
        assert!(rendered.contains("mods/target-0.zip"));
        app.bess_applying = true;
        assert!(!app.can_apply_bess_update(&app.bess_updates[0]));
        app.bess_applying = false;
        app.active_dir = None;
        assert!(!app.can_apply_bess_update(&app.bess_updates[0]));
    }
}
