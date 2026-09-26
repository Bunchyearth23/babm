use crate::paths::DetectedPaths;
use crate::vehicle::VehicleMod;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ChassisGroup {
    pub chassis_name: String,
    pub chassis_slug: String,
    pub variants: Vec<VehicleMod>,
    pub is_merged: bool,
    pub merged_mod_path: Option<PathBuf>,
}

pub struct Grouper;

impl Grouper {
    /// Groups a list of vehicles by chassis/family.
    /// Uses Automation SQLite database if available, otherwise heuristic name matching.
    pub fn group_vehicles(vehicles: &[VehicleMod], detected_paths: &DetectedPaths) -> Vec<ChassisGroup> {
        let db_mapping = detected_paths
            .automation_user_dir
            .as_ref()
            .and_then(|dir| load_automation_db_mapping(dir));

        let mut groups: HashMap<String, Vec<VehicleMod>> = HashMap::new();

        for v in vehicles {
            // Find chassis name
            let chassis = if let Some(ref map) = db_mapping {
                find_chassis_from_db(v, map)
            } else {
                None
            }
            .unwrap_or_else(|| deduce_chassis_name(&v.display_name, &v.internal_name));

            groups.entry(chassis).or_default().push(v.clone());
        }

        let mut result = Vec::new();
        for (chassis_name, mut variants) in groups {
            // Sort variants by display name
            variants.sort_by(|a, b| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()));
            let slug = slugify(&chassis_name);

            // Check if there is already a merged mod
            let is_merged = variants.iter().any(|v| v.file_name.starts_with("babm_") || v.configs.len() > 1);

            result.push(ChassisGroup {
                chassis_name,
                chassis_slug: slug,
                variants,
                is_merged,
                merged_mod_path: None,
            });
        }

        result.sort_by(|a, b| a.chassis_name.to_lowercase().cmp(&b.chassis_name.to_lowercase()));
        result
    }
}

/// Tries to query SQLite Sandbox_*.db to get Model -> Trims mappings
fn load_automation_db_mapping(user_dir: &Path) -> Option<HashMap<String, String>> {
    // Look for latest Sandbox_*.db
    let mut db_path = None;
    if let Ok(entries) = std::fs::read_dir(user_dir) {
        let mut candidates: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("Sandbox_") && n.ends_with(".db"))
            })
            .collect();
        candidates.sort();
        db_path = candidates.pop();
    }

    let db_path = db_path?;

    // Query sqlite3 if available via command
    let output = std::process::Command::new("sqlite3")
        .arg(&db_path)
        .arg("SELECT Models.Name, Trims.Name FROM Models JOIN Trims ON Models.UID = Trims.MUID;")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut map = HashMap::new(); // key: "Model Trim" or trim name -> Model

    for line in text.lines() {
        if let Some((model, trim)) = line.split_once('|') {
            let model = model.trim().to_string();
            let trim = trim.trim().to_string();
            let full = format!("{} {}", model, trim).to_lowercase();
            map.insert(full, model.clone());
            map.insert(format!("{}_{}", slugify(&model), slugify(&trim)), model);
        }
    }

    Some(map)
}

fn find_chassis_from_db(v: &VehicleMod, map: &HashMap<String, String>) -> Option<String> {
    let lower_display = v.display_name.to_lowercase();
    if let Some(m) = map.get(&lower_display) {
        return Some(m.clone());
    }

    let lower_internal = v.internal_name.to_lowercase();
    if let Some(m) = map.get(&lower_internal) {
        return Some(m.clone());
    }

    // Try without author prefix (e.g. bunchyearth23_b5_a -> b5_a)
    for prefix in ["bunchyearth23_", "camso_"] {
        if let Some(stripped) = lower_internal.strip_prefix(prefix) {
            if let Some(m) = map.get(stripped) {
                return Some(m.clone());
            }
        }
    }

    None
}

/// Fallback heuristic deduction of chassis name
fn deduce_chassis_name(display_name: &str, internal_name: &str) -> String {
    // If display name has multiple parts, e.g. "B5 A" -> "B5", "Volk Icarus II" -> "Volk Icarus"
    let parts: Vec<&str> = display_name.split_whitespace().collect();
    if parts.len() > 1 {
        // If last part is single letter (A, C, GT, etc.) or Roman numeral
        let last = parts.last().copied().unwrap_or("");
        if last.len() == 1 || is_roman_numeral(last) || ["gt", "tc", "sport", "turbo", "base", "uno", "coupe"].contains(&last.to_lowercase().as_str()) {
            return parts[..parts.len() - 1].join(" ");
        }
    }

    // Check internal name: e.g. bunchyearth23_b5_a -> b5
    let mut clean_internal = internal_name;
    for prefix in ["bunchyearth23_", "camso_"] {
        if let Some(rest) = clean_internal.strip_prefix(prefix) {
            clean_internal = rest;
        }
    }

    let subparts: Vec<&str> = clean_internal.split('_').collect();
    if subparts.len() > 1 {
        let last = subparts.last().copied().unwrap_or("");
        if last.len() <= 2 || is_roman_numeral(last) || ["gt", "tc", "base"].contains(&last.to_lowercase().as_str()) {
            return uppercase_first(&subparts[..subparts.len() - 1].join(" "));
        }
    }

    display_name.to_string()
}

fn is_roman_numeral(s: &str) -> bool {
    let s = s.to_uppercase();
    matches!(s.as_str(), "I" | "II" | "III" | "IV" | "V" | "VI" | "VII" | "VIII" | "IX" | "X")
}

pub fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<&str>>()
        .join("_")
}

fn uppercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}
