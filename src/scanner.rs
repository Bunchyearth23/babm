use crate::vehicle::{EngineSpecs, VehicleConfig, VehicleMod};
use serde_json::Value;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub fn scan_directory(dir: &Path) -> Vec<VehicleMod> {
    let mut vehicles = Vec::new();
    if !dir.is_dir() {
        return vehicles;
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "zip") {
                if let Ok(vehicle) = inspect_vehicle_zip(&path) {
                    vehicles.push(vehicle);
                }
            }
        }
    }

    // Build map of base vehicle display names and engine specs (internal_name -> ...)
    let mut base_names = std::collections::HashMap::new();
    let mut base_engines = std::collections::HashMap::new();
    for v in &vehicles {
        if !v.file_name.starts_with("bess-variant-") {
            base_names.insert(v.internal_name.clone(), v.display_name.clone());
            if let Some(ref eng) = v.engine {
                base_engines.insert(v.internal_name.clone(), eng.clone());
            }
        }
    }

    // Second pass: refine display names and engine specs for BESS variants
    for v in &mut vehicles {
        if v.file_name.starts_with("bess-variant-") {
            if let Some(base_name) = base_names.get(&v.internal_name) {
                v.display_name = format!("{} [BESS]", base_name);
            } else if !v.display_name.ends_with("[BESS]") {
                v.display_name = format!("{} [BESS]", pretty_title_from_slug(&v.internal_name));
            }

            if let Some(base_eng) = base_engines.get(&v.internal_name) {
                if let Some(ref mut eng) = v.engine {
                    if eng.max_rpm.is_none() || eng.max_rpm == Some(12000.0) {
                        eng.max_rpm = base_eng.max_rpm;
                    }
                    if eng.idle_rpm.is_none() {
                        eng.idle_rpm = base_eng.idle_rpm;
                    }
                    if eng.cylinders.is_none() {
                        eng.cylinders = base_eng.cylinders;
                    }
                } else {
                    v.engine = Some(base_eng.clone());
                }
            }
        }
    }

    vehicles.sort_by(|a, b| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()));
    vehicles
}

pub fn inspect_vehicle_zip(path: &Path) -> Result<VehicleMod, String> {
    let file = File::open(path).map_err(|e| format!("Cannot open file: {e}"))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    let file_size_bytes = metadata.len();
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("Invalid zip archive: {e}"))?;

    let mut vehicle_names = Vec::new();
    let mut info_json_index = None;
    let mut config_indexes = Vec::new();
    let mut thumbnail_index = None;
    let mut engine_jbeam_index = None;

    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();

        if name.starts_with("vehicles/") {
            let parts: Vec<&str> = name.split('/').collect();
            if parts.len() > 1 && !parts[1].is_empty() && !vehicle_names.contains(&parts[1].to_string()) {
                vehicle_names.push(parts[1].to_string());
            }

            let basename = parts.last().copied().unwrap_or("");
            if basename == "info.json" {
                info_json_index = Some(i);
            } else if basename.starts_with("info_") && basename.ends_with(".json") {
                let config_key = basename
                    .trim_start_matches("info_")
                    .trim_end_matches(".json")
                    .to_string();
                config_indexes.push((config_key, i));
            } else if (basename == "default.png" || basename.ends_with(".png")) && thumbnail_index.is_none() {
                thumbnail_index = Some(i);
            }

            if (name.contains("/eng_")
                && basename.starts_with("camso_engine_")
                && basename.ends_with(".jbeam"))
                || (basename.starts_with("bess_engine_") && basename.ends_with(".jbeam"))
            {
                engine_jbeam_index = Some(i);
            }
        }
    }

    let internal_name = vehicle_names.first().cloned().unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    });

    let is_bess = file_name.starts_with("bess-variant-")
        || config_indexes.iter().any(|(k, _)| k.starts_with("bess_"));

    let mut display_name = internal_name.clone();
    let mut author = if is_bess { "BESS / Automation".to_string() } else { "Unknown".to_string() };
    let mut is_automation = is_bess;
    let mut default_config = None;

    if let Some(idx) = info_json_index {
        let mut entry = zip.by_index(idx).map_err(|e| e.to_string())?;
        let mut content = String::new();
        if entry.read_to_string(&mut content).is_ok() {
            if let Ok(val) = serde_json::from_str::<Value>(&content) {
                if let Some(name) = val.get("Name").and_then(|v| v.as_str()) {
                    display_name = name.to_string();
                }
                if let Some(auth) = val.get("Author").and_then(|v| v.as_str()) {
                    author = auth.to_string();
                }
                if let Some(t) = val.get("Type").and_then(|v| v.as_str()) {
                    is_automation = t.eq_ignore_ascii_case("automation");
                }
                if let Some(pc) = val.get("default_pc").and_then(|v| v.as_str()) {
                    default_config = Some(pc.to_string());
                }
            }
        }
    } else if is_bess {
        display_name = format!("{} [BESS]", pretty_title_from_slug(&internal_name));
    }

    // Parse configs
    let mut configs = Vec::new();
    for (key, idx) in config_indexes {
        if let Ok(mut entry) = zip.by_index(idx) {
            let mut content = String::new();
            if entry.read_to_string(&mut content).is_ok() {
                if let Ok(v) = serde_json::from_str::<Value>(&content) {
                    let config_name = v
                        .get("Configuration")
                        .and_then(|s| s.as_str())
                        .unwrap_or(&key)
                        .to_string();

                    let weight_kg = v.get("Weight").and_then(|s| s.as_f64()).map(|f| f as f32);
                    let power_hp = v.get("Power").and_then(|s| s.as_f64()).map(|f| f as f32);
                    let torque_nm = v.get("Torque").and_then(|s| s.as_f64()).map(|f| f as f32);
                    let power_peak_rpm = v.get("PowerPeakRPM").and_then(|s| s.as_f64()).map(|f| f as f32);
                    let torque_peak_rpm = v.get("TorquePeakRPM").and_then(|s| s.as_f64()).map(|f| f as f32);
                    let top_speed_kmh = v.get("Top Speed").and_then(|s| s.as_f64()).map(|f| (f * 3.6) as f32);
                    let accel_0_100 = v.get("0-100 km/h").and_then(|s| s.as_f64()).map(|f| f as f32);
                    let drivetrain = v.get("Drivetrain").and_then(|s| s.as_str()).map(|s| s.to_string());
                    let transmission = v.get("Transmission").and_then(|s| s.as_str()).map(|s| s.to_string());
                    let fuel_type = v.get("Fuel Type").and_then(|s| s.as_str()).map(|s| s.to_string());
                    let induction = v.get("Induction Type").and_then(|s| s.as_str()).map(|s| s.to_string());

                    let (year_min, year_max) = if let Some(years) = v.get("Years") {
                        (
                            years.get("min").and_then(|n| n.as_u64()).map(|n| n as u32),
                            years.get("max").and_then(|n| n.as_u64()).map(|n| n as u32),
                        )
                    } else {
                        (None, None)
                    };

                    configs.push(VehicleConfig {
                        config_key: key,
                        name: config_name,
                        weight_kg,
                        power_hp,
                        torque_nm,
                        power_peak_rpm,
                        torque_peak_rpm,
                        top_speed_kmh,
                        accel_0_100,
                        drivetrain,
                        transmission,
                        fuel_type,
                        induction,
                        year_min,
                        year_max,
                    });
                }
            }
        }
    }

    // Engine specs from camso_engine_*.jbeam
    let mut engine = None;
    if let Some(idx) = engine_jbeam_index {
        is_automation = true;
        if let Ok(entry) = zip.by_index(idx) {
            let mut text = String::new();
            if entry.take(2_000_000).read_to_string(&mut text).is_ok() {
                let mut idle = Vec::new();
                let mut rev_limiter = Vec::new();
                let mut shift_up = Vec::new();
                let mut max = Vec::new();
                let mut cyl = Vec::new();
                parse_numbers(&text, "idleRPM", &mut idle);
                parse_numbers(&text, "revLimiterRPM", &mut rev_limiter);
                parse_numbers(&text, "highShiftUpRPM", &mut shift_up);
                parse_numbers(&text, "maxRPM", &mut max);
                parse_numbers(&text, "fundamentalFrequencyCylinderCount", &mut cyl);

                let redline = rev_limiter
                    .first()
                    .copied()
                    .or_else(|| shift_up.first().copied())
                    .or_else(|| max.first().copied().filter(|&m| m < 11999.0))
                    .or_else(|| max.first().copied());

                engine = Some(EngineSpecs {
                    idle_rpm: idle.first().copied(),
                    max_rpm: redline,
                    cylinders: cyl.first().map(|c| *c as u32),
                });
            }
        }
    }

    // Read thumbnail bytes
    let mut thumbnail_png = None;
    if let Some(idx) = thumbnail_index {
        if let Ok(mut entry) = zip.by_index(idx) {
            if entry.size() < 10_000_000 {
                let mut bytes = Vec::new();
                if entry.read_to_end(&mut bytes).is_ok() {
                    thumbnail_png = Some(bytes);
                }
            }
        }
    }

    Ok(VehicleMod {
        file_path: path.to_path_buf(),
        file_name,
        file_size_bytes,
        internal_name,
        display_name,
        author,
        is_automation,
        default_config,
        configs,
        engine,
        thumbnail_png,
    })
}

fn parse_numbers(text: &str, key: &str, out: &mut Vec<f32>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut matched = false;
    let mut colon = false;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            i += 2;
            while i < bytes.len() && !bytes[i..].starts_with(b"*/") {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == b'"' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            matched = serde_json::from_str::<String>(&text[start..i]).is_ok_and(|s| s == key);
            colon = false;
            continue;
        }
        if bytes[i] == b':' && matched {
            colon = true;
            i += 1;
            continue;
        }
        if colon {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || b"+-.eE".contains(&bytes[i])) {
                i += 1;
            }
            let boundary =
                i == bytes.len() || bytes[i].is_ascii_whitespace() || b",}]/".contains(&bytes[i]);
            if boundary
                && let Ok(n) = text[start..i].parse::<f32>()
                && n.is_finite()
                && !out.contains(&n)
            {
                out.push(n);
            }
            if i == start {
                i += 1;
            }
        } else {
            i += 1;
        }
        matched = false;
        colon = false;
    }
}

pub fn pretty_title_from_slug(slug: &str) -> String {
    slug.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            if is_roman_numeral(w) || w.len() <= 2 {
                w.to_uppercase()
            } else {
                let mut chars = w.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

fn is_roman_numeral(s: &str) -> bool {
    let s = s.to_uppercase();
    matches!(s.as_str(), "I" | "II" | "III" | "IV" | "V" | "VI" | "VII" | "VIII" | "IX" | "X")
}
