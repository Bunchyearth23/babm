use babm::app::BabmApp;
use babm::cli::{Cli, Commands};
use babm::grouper::Grouper;
use babm::merger::Merger;
use babm::paths::DetectedPaths;
use babm::scanner;
use clap::Parser;
use std::path::PathBuf;

fn main() -> Result<(), eframe::Error> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Paths) => {
            let paths = DetectedPaths::detect();
            println!("\n=== BABM - Detected Paths ===");
            if let Some(user) = paths.beamng_user_dir {
                println!("BeamNG user folder: {}", user.display());
            } else {
                println!("BeamNG user folder: not detected");
            }

            println!("BeamNG mod folders:");
            if paths.beamng_mods_dirs.is_empty() {
                println!("  (none)");
            } else {
                for d in &paths.beamng_mods_dirs {
                    println!("  - {}", d.display());
                }
            }

            if let Some(auto) = paths.automation_user_dir {
                println!("Automation user folder: {}", auto.display());
            }
            if let Some(exp) = paths.automation_exports_dir {
                println!("Automation exports folder (.car): {}", exp.display());
            }
            println!();
            Ok(())
        }

        Some(Commands::Scan { path, automation_only }) => {
            let paths = DetectedPaths::detect();
            let target_dir = path
                .or_else(|| paths.beamng_mods_dirs.first().cloned())
                .unwrap_or_else(|| PathBuf::from("."));

            println!("\nScanning folder: {}\n", target_dir.display());
            let mut vehicles = scanner::scan_directory(&target_dir);

            if automation_only {
                vehicles.retain(|v| v.is_automation);
            }

            if vehicles.is_empty() {
                println!("No vehicles found.");
                return Ok(());
            }

            println!("{:<32} {:<24} {:<12} {:<10} {:<10}", "VEHICLE NAME", "FILE", "POWER", "WEIGHT", "TYPE");
            println!("{:-<95}", "");

            for v in &vehicles {
                let main_cfg = v.main_config();
                let power = main_cfg
                    .and_then(|c| c.power_hp)
                    .map(|p| format!("{:.0} hp", p))
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

            println!("\nTotal: {} vehicle(s) found.\n", vehicles.len());
            Ok(())
        }

        Some(Commands::Groups { path }) => {
            let paths = DetectedPaths::detect();
            let target_dir = path
                .or_else(|| paths.beamng_mods_dirs.first().cloned())
                .unwrap_or_else(|| PathBuf::from("."));

            let vehicles = scanner::scan_directory(&target_dir);
            let groups = Grouper::group_vehicles(&vehicles, &paths);

            println!("\n=== Detected Chassis Groups in {} ===\n", target_dir.display());
            for g in &groups {
                println!("🚗 Chassis: {} ({} variant(s))", g.chassis_name, g.variants.len());
                for v in &g.variants {
                    let main_cfg = v.main_config();
                    let power = main_cfg
                        .and_then(|c| c.power_hp)
                        .map(|p| format!("{:.0} hp", p))
                        .unwrap_or_else(|| "-".into());
                    println!("    ├── {} [{}] (file: {})", v.display_name, power, v.file_name);
                }
                println!();
            }
            Ok(())
        }

        Some(Commands::Merge { chassis, path }) => {
            let paths = DetectedPaths::detect();
            let target_dir = path
                .or_else(|| paths.beamng_mods_dirs.first().cloned())
                .unwrap_or_else(|| PathBuf::from("."));

            let vehicles = scanner::scan_directory(&target_dir);
            let groups = Grouper::group_vehicles(&vehicles, &paths);

            let group = groups
                .into_iter()
                .find(|g| g.chassis_name.eq_ignore_ascii_case(&chassis) || g.chassis_slug.eq_ignore_ascii_case(&chassis))
                .ok_or_else(|| format!("Chassis '{}' not found in {}.", chassis, target_dir.display()));

            match group {
                Ok(g) => {
                    println!("\nMerging {} variants for chassis '{}'...", g.variants.len(), g.chassis_name);
                    for v in &g.variants {
                        println!("  + Including: {}", v.display_name);
                    }

                    match Merger::merge_variants(&g.chassis_name, &g.variants, &target_dir) {
                        Ok(out) => {
                            println!("\n✅ Success! Unified mod created: {}", out.display());
                            println!("Original variants safely backed up in .babm_backup/{} (100% reversible).\n", g.chassis_slug);
                        }
                        Err(e) => {
                            eprintln!("\n❌ Merge error: {e}\n");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
                }
            }
            Ok(())
        }

        Some(Commands::Unmerge { chassis, path }) => {
            let paths = DetectedPaths::detect();
            let target_dir = path
                .or_else(|| paths.beamng_mods_dirs.first().cloned())
                .unwrap_or_else(|| PathBuf::from("."));

            println!("\nUnmerging chassis '{}' in {}...", chassis, target_dir.display());
            match Merger::unmerge_chassis(&chassis, &target_dir) {
                Ok(()) => {
                    println!("✅ Success! Merged mod removed and all original variant files restored.\n");
                }
                Err(e) => {
                    eprintln!("❌ Error: {e}\n");
                }
            }
            Ok(())
        }

        Some(Commands::Isolate { target, out }) => {
            let paths = DetectedPaths::detect();
            let target_dir = paths.beamng_mods_dirs.first().cloned().unwrap_or_else(|| PathBuf::from("."));
            let out_dir = out.unwrap_or_else(|| PathBuf::from("."));

            let target_path = PathBuf::from(&target);
            let zip_to_isolate = if target_path.is_file() {
                target_path
            } else {
                let vehicles = scanner::scan_directory(&target_dir);
                let found = vehicles.into_iter().find(|v| {
                    v.display_name.eq_ignore_ascii_case(&target)
                        || v.internal_name.eq_ignore_ascii_case(&target)
                        || v.file_name.eq_ignore_ascii_case(&target)
                });
                found.map(|v| v.file_path).unwrap_or(PathBuf::new())
            };

            if !zip_to_isolate.is_file() {
                eprintln!("Variant '{}' not found.", target);
                return Ok(());
            }

            match Merger::isolate_variant_for_bess(&zip_to_isolate, &out_dir) {
                Ok(path) => {
                    println!("\n✅ Isolated variant ready for BESS created: {}\n", path.display());
                }
                Err(e) => {
                    eprintln!("❌ Error: {e}\n");
                }
            }
            Ok(())
        }

        Some(Commands::Info { target }) => {
            let target_path = PathBuf::from(&target);
            let vehicle = if target_path.is_file() {
                scanner::inspect_vehicle_zip(&target_path)
            } else {
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
                found.ok_or_else(|| format!("Vehicle '{target}' not found."))
            };

            match vehicle {
                Ok(v) => {
                    println!("\n=== Vehicle Details: {} ===", v.display_name);
                    println!("Internal name : {}", v.internal_name);
                    println!("Author        : {}", v.author);
                    println!("Source        : {}", if v.is_automation { "Automation Export" } else { "BeamNG Mod" });
                    println!("File          : {}", v.file_path.display());
                    println!("Size          : {:.2} MB", v.file_size_bytes as f64 / (1024.0 * 1024.0));

                    if let Some(ref eng) = v.engine {
                        println!("\n[Engine (JBeam Extraction)]");
                        if let Some(cyl) = eng.cylinders {
                            println!("  Cylinders    : {}", cyl);
                        }
                        if let Some(idle) = eng.idle_rpm {
                            println!("  Idle         : {:.0} RPM", idle);
                        }
                        if let Some(max) = eng.max_rpm {
                            println!("  Redline      : {:.0} RPM", max);
                        }
                    }

                    println!("\n[Configurations ({})]", v.configs.len());
                    for cfg in &v.configs {
                        println!("  * {}", cfg.name);
                        if let Some(p) = cfg.power_hp {
                            println!("    Power        : {:.0} hp", p);
                        }
                        if let Some(t) = cfg.torque_nm {
                            println!("    Torque       : {:.0} Nm", t);
                        }
                        if let Some(w) = cfg.weight_kg {
                            println!("    Weight       : {:.0} kg", w);
                        }
                        if let Some(s) = cfg.top_speed_kmh {
                            println!("    Top speed    : {:.0} km/h", s);
                        }
                        if let Some(a) = cfg.accel_0_100 {
                            println!("    0-100 km/h   : {:.1} s", a);
                        }
                        if let Some(dt) = &cfg.drivetrain {
                            println!("    Drivetrain   : {}", dt);
                        }
                    }
                    println!();
                }
                Err(e) => {
                    eprintln!("Error: {e}");
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
