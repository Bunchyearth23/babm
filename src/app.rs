use crate::paths::DetectedPaths;
use crate::scanner;
use crate::vehicle::VehicleMod;
use eframe::egui::{self, Color32, RichText, TextureHandle};
use std::collections::HashMap;
use std::path::PathBuf;

pub struct BabmApp {
    paths: DetectedPaths,
    active_dir: Option<PathBuf>,
    vehicles: Vec<VehicleMod>,
    selected_index: Option<usize>,
    filter_search: String,
    filter_automation_only: bool,
    textures: HashMap<PathBuf, TextureHandle>,
}

impl BabmApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let paths = DetectedPaths::detect();
        let default_dir = paths.beamng_mods_dirs.first().cloned();
        let mut app = Self {
            paths,
            active_dir: default_dir.clone(),
            vehicles: Vec::new(),
            selected_index: None,
            filter_search: String::new(),
            filter_automation_only: false,
            textures: HashMap::new(),
        };

        if let Some(ref dir) = default_dir {
            app.load_directory(dir.clone());
        }

        app
    }

    pub fn load_directory(&mut self, dir: PathBuf) {
        self.active_dir = Some(dir.clone());
        self.vehicles = scanner::scan_directory(&dir);
        self.selected_index = if self.vehicles.is_empty() { None } else { Some(0) };
        self.textures.clear();
    }
}

impl eframe::App for BabmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("🚗 BABM").strong().color(Color32::from_rgb(255, 170, 0)));
                ui.label(RichText::new("— Bunchy's Automation BeamNG Management").italics().color(Color32::LIGHT_GRAY));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("📁 Choisir dossier...").clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.load_directory(folder);
                        }
                    }

                    if ui.button("🔄 Rafraîchir").clicked() {
                        if let Some(dir) = self.active_dir.clone() {
                            self.load_directory(dir);
                        }
                    }

                    if let Some(ref dir) = self.active_dir {
                        if ui.button("📂 Ouvrir dossier").clicked() {
                            let _ = std::process::Command::new("explorer").arg(dir).spawn();
                        }
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
                        .unwrap_or_else(|| "Sélectionner un dossier...".to_string());

                    egui::ComboBox::from_label("Dossier actif")
                        .selected_text(selected_name)
                        .show_ui(ui, |ui| {
                            for path in &self.paths.beamng_mods_dirs {
                                let label = path.display().to_string();
                                let is_selected = self.active_dir.as_ref() == Some(path);
                                if ui.selectable_label(is_selected, &label).clicked() {
                                    dir_to_load = Some(path.clone());
                                }
                            }
                        });
                } else {
                    let current_dir_text = self
                        .active_dir
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "Aucun dossier sélectionné".to_string());
                    ui.label(RichText::new(format!("Dossier actif : {current_dir_text}")).small().color(Color32::GRAY));
                }

                if let Some(dir) = dir_to_load {
                    self.load_directory(dir);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{} véhicule(s) détecté(s)", self.vehicles.len())).strong());
                });
            });

            ui.separator();

            // Search and filters
            ui.horizontal(|ui| {
                ui.label("🔍 Rechercher :");
                ui.text_edit_singleline(&mut self.filter_search);

                ui.checkbox(&mut self.filter_automation_only, "Automation uniquement");

                if !self.filter_search.is_empty() && ui.button("✖").clicked() {
                    self.filter_search.clear();
                }
            });
            ui.add_space(4.0);
        });

        // Main content: left vehicle list, right details panel
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
                    let match_config = v.configs.iter().any(|c| c.name.to_lowercase().contains(&search));
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
                ui.heading(RichText::new("Véhicules").size(16.0));
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    if filtered_indices.is_empty() {
                        ui.label(RichText::new("Aucun véhicule trouvé.").italics());
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
                                ui.colored_label(Color32::from_rgb(255, 140, 0), "⚡ Automation Mod");
                            }
                        });

                        ui.label(RichText::new(format!("Nom interne : {}", v.internal_name)).monospace().small());
                        ui.label(RichText::new(format!("Auteur : {}", v.author)).small());

                        ui.separator();

                        // Thumbnail
                        if let Some(ref png_bytes) = v.thumbnail_png {
                            if !self.textures.contains_key(&v.file_path) {
                                if let Ok(img) = image::load_from_memory(png_bytes) {
                                    let size = [img.width() as usize, img.height() as usize];
                                    let rgba = img.to_rgba8();
                                    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                                    let tex = ctx.load_texture(
                                        format!("thumb_{}", v.internal_name),
                                        color_image,
                                        egui::TextureOptions::LINEAR,
                                    );
                                    self.textures.insert(v.file_path.clone(), tex);
                                }
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
                                ui.heading(RichText::new("⚙ Moteur (Extraction JBeam)").size(15.0));
                                ui.horizontal(|ui| {
                                    if let Some(cyl) = engine.cylinders {
                                        ui.label(format!("Cylindres : {cyl}"));
                                    }
                                    if let Some(idle) = engine.idle_rpm {
                                        ui.label(format!("| Ralenti : {:.0} RPM", idle));
                                    }
                                    if let Some(max) = engine.max_rpm {
                                        ui.label(format!("| Régime max : {:.0} RPM", max));
                                    }
                                });
                            });
                            ui.add_space(6.0);
                        }

                        // Configurations
                        ui.heading(RichText::new(format!("Configurations ({})", v.configs.len())).size(16.0));
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
                                        "Puissance : {}",
                                        cfg.power_hp
                                            .map(|p| format!("{:.0} ch", p))
                                            .unwrap_or_else(|| "N/A".into())
                                    ));
                                    cols[0].label(format!(
                                        "Couple : {}",
                                        cfg.torque_nm
                                            .map(|t| format!("{:.0} Nm", t))
                                            .unwrap_or_else(|| "N/A".into())
                                    ));

                                    cols[1].label(format!(
                                        "Poids : {}",
                                        cfg.weight_kg
                                            .map(|w| format!("{:.0} kg", w))
                                            .unwrap_or_else(|| "N/A".into())
                                    ));
                                    cols[1].label(format!(
                                        "0-100 km/h : {}",
                                        cfg.accel_0_100
                                            .map(|a| format!("{:.1} s", a))
                                            .unwrap_or_else(|| "N/A".into())
                                    ));

                                    cols[2].label(format!(
                                        "Vitesse max : {}",
                                        cfg.top_speed_kmh
                                            .map(|s| format!("{:.0} km/h", s))
                                            .unwrap_or_else(|| "N/A".into())
                                    ));
                                    cols[2].label(format!(
                                        "Carburant : {}",
                                        cfg.fuel_type.as_deref().unwrap_or("N/A")
                                    ));
                                });
                            });
                            ui.add_space(4.0);
                        }

                        ui.separator();
                        // File details
                        ui.label(RichText::new("Fichier mod :").strong());
                        ui.label(RichText::new(v.file_path.display().to_string()).monospace().small());
                        ui.horizontal(|ui| {
                            ui.label(format!("Taille : {:.2} Mo", v.file_size_bytes as f64 / (1024.0 * 1024.0)));
                            if ui.button("Explorer le fichier").clicked() {
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
                    ui.label(RichText::new("Sélectionnez un véhicule dans la liste pour voir ses détails").italics());
                });
            }
        });
    }
}
