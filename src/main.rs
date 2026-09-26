use babm::app::BabmApp;
use babm::cli::{Cli, Commands};
use babm::paths::DetectedPaths;
use babm::scanner;
use clap::Parser;
use std::path::PathBuf;

fn main() -> Result<(), eframe::Error> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Paths) => {
            let paths = DetectedPaths::detect();
            println!("\n=== BABM - Chemins détectés ===");
            if let Some(user) = paths.beamng_user_dir {
                println!("Dossier utilisateur BeamNG : {}", user.display());
            } else {
                println!("Dossier utilisateur BeamNG : non détecté");
            }

            println!("Dossiers de mods BeamNG :");
            if paths.beamng_mods_dirs.is_empty() {
                println!("  (aucun)");
            } else {
                for d in &paths.beamng_mods_dirs {
                    println!("  - {}", d.display());
                }
            }

            if let Some(auto) = paths.automation_user_dir {
                println!("Dossier utilisateur Automation : {}", auto.display());
            }
            if let Some(exp) = paths.automation_exports_dir {
                println!("Dossier exports Automation (.car) : {}", exp.display());
            }
            println!();
            Ok(())
        }

        Some(Commands::Scan { path, automation_only }) => {
            let paths = DetectedPaths::detect();
            let target_dir = path
                .or_else(|| paths.beamng_mods_dirs.first().cloned())
                .unwrap_or_else(|| PathBuf::from("."));

            println!("\nAnalyse du dossier : {}\n", target_dir.display());
            let mut vehicles = scanner::scan_directory(&target_dir);

            if automation_only {
                vehicles.retain(|v| v.is_automation);
            }

            if vehicles.is_empty() {
                println!("Aucun véhicule trouvé.");
                return Ok(());
            }

            println!("{:<32} {:<24} {:<12} {:<10} {:<10}", "NOM DU VÉHICULE", "FICHIER", "PUISSANCE", "POIDS", "TYPE");
            println!("{:-<95}", "");

            for v in &vehicles {
                let main_cfg = v.main_config();
                let power = main_cfg
                    .and_then(|c| c.power_hp)
                    .map(|p| format!("{:.0} ch", p))
                    .unwrap_or_else(|| "-".into());
                let weight = main_cfg
                    .and_then(|c| c.weight_kg)
                    .map(|w| format!("{:.0} kg", w))
                    .unwrap_or_else(|| "-".into());
                let tag = if v.is_automation { "Automation" } else { "BeamNG" };

                let short_name = if v.display_name.len() > 30 {
                    format!("{}...", &v.display_name[..27])
                } else {
                    v.display_name.clone()
                };

                let short_file = if v.file_name.len() > 22 {
                    format!("{}...", &v.file_name[..19])
                } else {
                    v.file_name.clone()
                };

                println!("{:<32} {:<24} {:<12} {:<10} {:<10}", short_name, short_file, power, weight, tag);
            }

            println!("\nTotal : {} véhicule(s) trouvé(s).\n", vehicles.len());
            Ok(())
        }

        Some(Commands::Info { target }) => {
            let target_path = PathBuf::from(&target);
            let vehicle = if target_path.is_file() {
                scanner::inspect_vehicle_zip(&target_path)
            } else {
                // Search in detected mods directories
                let paths = DetectedPaths::detect();
                let mut found = None;
                for dir in &paths.beamng_mods_dirs {
                    let vehicles = scanner::scan_directory(dir);
                    if let Some(v) = vehicles.into_iter().find(|v| {
                        v.internal_name.eq_ignore_ascii_case(&target)
                            || v.display_name.eq_ignore_ascii_case(&target)
                            || v.file_name.eq_ignore_ascii_case(&target)
                    }) {
                        found = Some(v);
                        break;
                    }
                }
                found.ok_or_else(|| format!("Véhicule '{target}' introuvable."))
            };

            match vehicle {
                Ok(v) => {
                    println!("\n=== Détails du véhicule : {} ===", v.display_name);
                    println!("Nom interne : {}", v.internal_name);
                    println!("Auteur      : {}", v.author);
                    println!("Source      : {}", if v.is_automation { "Export Automation" } else { "Mod BeamNG" });
                    println!("Fichier     : {}", v.file_path.display());
                    println!("Taille      : {:.2} Mo", v.file_size_bytes as f64 / (1024.0 * 1024.0));

                    if let Some(ref eng) = v.engine {
                        println!("\n[Moteur (Extraction JBeam)]");
                        if let Some(cyl) = eng.cylinders {
                            println!("  Cylindres   : {}", cyl);
                        }
                        if let Some(idle) = eng.idle_rpm {
                            println!("  Ralenti     : {:.0} RPM", idle);
                        }
                        if let Some(max) = eng.max_rpm {
                            println!("  Régime max  : {:.0} RPM", max);
                        }
                    }

                    println!("\n[Configurations ({})]", v.configs.len());
                    for cfg in &v.configs {
                        println!("  * {}", cfg.name);
                        if let Some(p) = cfg.power_hp {
                            println!("    Puissance    : {:.0} ch", p);
                        }
                        if let Some(t) = cfg.torque_nm {
                            println!("    Couple       : {:.0} Nm", t);
                        }
                        if let Some(w) = cfg.weight_kg {
                            println!("    Poids        : {:.0} kg", w);
                        }
                        if let Some(s) = cfg.top_speed_kmh {
                            println!("    Vitesse max  : {:.0} km/h", s);
                        }
                        if let Some(a) = cfg.accel_0_100 {
                            println!("    0-100 km/h   : {:.1} s", a);
                        }
                        if let Some(dt) = &cfg.drivetrain {
                            println!("    Transmission : {}", dt);
                        }
                    }
                    println!();
                }
                Err(e) => {
                    eprintln!("Erreur : {e}");
                }
            }
            Ok(())
        }

        None | Some(Commands::Gui) => {
            let native_options = eframe::NativeOptions {
                viewport: eframe::egui::ViewportBuilder::default()
                    .with_title("BABM — Bunchy's Automation BeamNG Management")
                    .with_inner_size([1120.0, 780.0])
                    .with_min_inner_size([800.0, 600.0]),
                ..Default::default()
            };

            eframe::run_native(
                "BABM — Bunchy's Automation BeamNG Management",
                native_options,
                Box::new(|cc| Ok(Box::new(BabmApp::new(cc)))),
            )
        }
    }
}
