use crate::grouper::slugify;
use crate::vehicle::VehicleMod;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

#[derive(Debug, Serialize, Deserialize)]
pub struct MergeManifest {
    pub chassis_name: String,
    pub chassis_slug: String,
    pub merged_mod_file: String,
    pub original_files: Vec<OriginalFileBackup>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OriginalFileBackup {
    pub original_filename: String,
    pub original_path: PathBuf,
    pub backup_filename: String,
}

pub struct Merger;

impl Merger {
    /// Merges multiple vehicle variant ZIPs into a single chassis mod ZIP.
    /// Safely backups original files into `.babm_backup/<chassis_slug>/` for instant unmerging.
    pub fn merge_variants(
        chassis_name: &str,
        variants: &[VehicleMod],
        mods_dir: &Path,
    ) -> Result<PathBuf, String> {
        if variants.is_empty() {
            return Err("No variants selected for merging.".into());
        }

        let _lock = crate::bess::ModsLock::acquire(mods_dir)?;
        let all_inputs = crate::bess::merge_inputs(variants, mods_dir)?;
        let preferred = crate::bess::prefer_bess_variants(&all_inputs)?;
        let variants = preferred.as_slice();

        let chassis_slug = slugify(chassis_name);
        let output_filename = format!("babm_{}.zip", chassis_slug);
        let output_path = mods_dir.join(&output_filename);
        if output_path.exists() {
            return Err(
                "This merged pack already exists. Apply its BESS audio update or unmerge it first."
                    .into(),
            );
        }
        crate::bess::check_merge_audio(variants, &chassis_slug)?;
        let staged_output = crate::bess::unique_path(mods_dir, ".babm-merge", "tmp");

        let backup_dir = mods_dir.join(".babm_backup").join(&chassis_slug);
        crate::bess::no_links(&backup_dir)?;
        fs::create_dir_all(&backup_dir)
            .map_err(|e| format!("Error creating backup directory: {e}"))?;

        // 1. Collect and backup original files
        let mut original_backups = Vec::new();
        for v in &all_inputs {
            let backup_dest = backup_dir.join(&v.file_name);
            if backup_dest.exists()
                && crate::bess::sha256_file(&backup_dest)?
                    != crate::bess::sha256_file(&v.file_path)?
            {
                return Err(format!(
                    "A different original backup already exists for {}",
                    v.file_name
                ));
            }
            fs::copy(&v.file_path, &backup_dest)
                .map_err(|e| format!("Error backing up {}: {e}", v.file_name))?;

            original_backups.push(OriginalFileBackup {
                original_filename: v.file_name.clone(),
                original_path: v.file_path.clone(),
                backup_filename: v.file_name.clone(),
            });
        }

        // 2. Prepare merged archive
        let out_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged_output)
            .map_err(|e| format!("Impossible de créer {}: {e}", output_path.display()))?;
        let _staged_guard = crate::bess::StagedFile(staged_output.clone());
        let mut zip_writer = ZipWriter::new(out_file);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o755);

        let mut written_entries: HashSet<String> = HashSet::new();
        let mut combined_paints = json!({});
        let mut default_pc = None;
        let mut first_author = "Bunchy".to_string();
        let mut first_trim_part = None;

        for (variant_idx, v) in variants.iter().enumerate() {
            let zip_file = File::open(&v.file_path)
                .map_err(|e| format!("Impossible d'ouvrir {}: {e}", v.file_path.display()))?;
            let mut archive = ZipArchive::new(zip_file)
                .map_err(|e| format!("Archive ZIP invalide {}: {e}", v.file_name))?;

            let old_vehicle_prefix = format!("vehicles/{}/", v.internal_name);
            let new_vehicle_prefix = format!("vehicles/{}/", chassis_slug);

            if variant_idx == 0 {
                first_author = v.author.clone();
                default_pc = v.default_config.clone();
            }

            // Determine variant trim name and BESS status for tagging parts in BeamNG configurator
            let trim_name = v
                .main_config()
                .map(|c| {
                    c.name
                        .trim_end_matches(" (BESS)")
                        .trim_end_matches(" [BESS]")
                        .trim()
                        .to_string()
                })
                .unwrap_or_else(|| {
                    v.display_name
                        .trim_end_matches(" [BESS]")
                        .split_whitespace()
                        .last()
                        .unwrap_or("Var")
                        .to_string()
                });

            let is_bess = v.file_name.starts_with("bess-variant-")
                || v.display_name.contains("[BESS]")
                || v.display_name.contains("(BESS)")
                || crate::bess::read_marker(&v.file_path)?.is_some();

            for i in 0..archive.len() {
                let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
                let original_name = entry.name().to_string();
                // Export provenance belongs to one original, not the combined
                // vehicle; source backups retain the complete marker.
                if original_name == crate::bess::MARKER || original_name == "babm-bess-updates.json"
                {
                    continue;
                }

                if entry.is_dir() {
                    continue;
                }

                // Determine new path
                let new_entry_name = if original_name.starts_with(&old_vehicle_prefix) {
                    let sub = original_name.strip_prefix(&old_vehicle_prefix).unwrap();
                    format!("{}{}", new_vehicle_prefix, sub)
                } else if original_name.starts_with("vehicles/") {
                    // Other vehicle path inside
                    let mut parts: Vec<&str> = original_name.split('/').collect();
                    if parts.len() > 2 {
                        parts[1] = &chassis_slug;
                        parts.join("/")
                    } else {
                        original_name.clone()
                    }
                } else {
                    original_name.clone()
                };

                // Skip special files that we will generate unified versions of
                let _basename = new_entry_name.rsplit('/').next().unwrap_or("");
                if new_entry_name == format!("{}info.json", new_vehicle_prefix) {
                    // Extract paints from info.json
                    let mut text = String::new();
                    if entry.read_to_string(&mut text).is_ok()
                        && let Ok(v) = serde_json::from_str::<Value>(&text)
                        && let Some(paints) = v.get("paints").and_then(|p| p.as_object())
                        && let Some(comb) = combined_paints.as_object_mut()
                    {
                        for (k, val) in paints {
                            if !comb.contains_key(k) {
                                comb.insert(k.clone(), val.clone());
                            }
                        }
                    }
                    continue;
                }

                if new_entry_name == format!("{}camso_core.jbeam", new_vehicle_prefix) {
                    // Extract the default trim part for slot
                    let mut text = String::new();
                    if entry.read_to_string(&mut text).is_ok()
                        && first_trim_part.is_none()
                        && let Some(pos) = text.find("\"Camso_Trim\"")
                    {
                        let after = &text[pos..];
                        if let Some(line) = after.lines().next() {
                            for word in line.split('"') {
                                if word.starts_with("Camso_Trim_") {
                                    first_trim_part = Some(word.to_string());
                                    break;
                                }
                            }
                        }
                    }
                    continue;
                }

                if written_entries.contains(&new_entry_name) {
                    // Already written (shared common file like lua, dds, etc.)
                    continue;
                }

                // Check if we need to replace vehicle path references inside text files
                let is_text_file = new_entry_name.ends_with(".jbeam")
                    || new_entry_name.ends_with(".json")
                    || new_entry_name.ends_with(".materials.json")
                    || new_entry_name.ends_with(".lua")
                    || new_entry_name.ends_with(".pc");

                if is_text_file && entry.size() < 10_000_000 {
                    let mut content = Vec::new();
                    entry.read_to_end(&mut content).map_err(|e| e.to_string())?;

                    if let Ok(text) = String::from_utf8(content) {
                        // Replace "vehicles/<old>/" with "vehicles/<new>/"
                        let old_ref = format!("vehicles/{}/", v.internal_name);
                        let new_ref = format!("vehicles/{}/", chassis_slug);
                        let mut modified_text = text.replace(&old_ref, &new_ref);

                        // Tag part names in JBeam information blocks with variant and BESS
                        if new_entry_name.ends_with(".jbeam")
                            && !new_entry_name.ends_with("camso_core.jbeam")
                        {
                            modified_text =
                                tag_jbeam_part_names(&modified_text, &trim_name, is_bess);
                        }

                        zip_writer
                            .start_file(&new_entry_name, options)
                            .map_err(|e| e.to_string())?;
                        zip_writer
                            .write_all(modified_text.as_bytes())
                            .map_err(|e| e.to_string())?;
                        written_entries.insert(new_entry_name);
                        continue;
                    }
                }

                // Binary or unchanged file: copy directly
                zip_writer
                    .start_file(&new_entry_name, options)
                    .map_err(|e| e.to_string())?;
                std::io::copy(&mut entry, &mut zip_writer).map_err(|e| e.to_string())?;
                written_entries.insert(new_entry_name);
            }
        }

        // 3. Write unified info.json
        let unified_info = json!({
            "Name": chassis_name,
            "Author": first_author,
            "Type": "Automation",
            "Brand": "",
            "ExporterVersion": 40000,
            "AutomationVersion": 0,
            "default_pc": default_pc.unwrap_or_else(|| "default".into()),
            "paints": combined_paints
        });
        let info_path = format!("vehicles/{}/info.json", chassis_slug);
        zip_writer
            .start_file(&info_path, options)
            .map_err(|e| e.to_string())?;
        zip_writer
            .write_all(
                serde_json::to_string_pretty(&unified_info)
                    .unwrap()
                    .as_bytes(),
            )
            .map_err(|e| e.to_string())?;

        // 4. Write unified camso_core.jbeam
        let trim_part = first_trim_part.unwrap_or_else(|| "Camso_Trim_default".into());
        let core_part_name = format!("Camso_{}_core", chassis_slug);
        let core_jbeam = format!(
            r#"{{
	"{}":
	{{
		"information":{{
			"authors":"Camshaft Software",
			"name":"{}",
			"value":1
		}},
		"slotType" : "main",
		"slots":[
			["type", "default", "description"],
			["Camso_Mod", "", "Additional Modification"],
			["Camso_Trim", "{}", "Body", {{"coreSlot":true}}],
			["paint_design", "", "Paint Design"]
		]
	}}
}}
"#,
            core_part_name, chassis_name, trim_part
        );
        let core_path = format!("vehicles/{}/camso_core.jbeam", chassis_slug);
        zip_writer
            .start_file(&core_path, options)
            .map_err(|e| e.to_string())?;
        zip_writer
            .write_all(core_jbeam.as_bytes())
            .map_err(|e| e.to_string())?;

        zip_writer
            .finish()
            .map_err(|e| format!("Error finalizing ZIP: {e}"))?
            .sync_all()
            .map_err(|e| e.to_string())?;

        // 5. Write Manifest
        let manifest = MergeManifest {
            chassis_name: chassis_name.to_string(),
            chassis_slug: chassis_slug.clone(),
            merged_mod_file: output_filename.clone(),
            original_files: original_backups,
            created_at: format!("{:?}", std::time::SystemTime::now()),
        };
        let manifest_path = backup_dir.join("manifest.json");
        crate::bess::no_links(&manifest_path)?;
        let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
        fs::write(manifest_path, manifest_json).map_err(|e| e.to_string())?;
        fs::rename(&staged_output, &output_path)
            .map_err(|e| format!("Cannot publish merged ZIP: {e}"))?;

        // 6. Move/disable original files to avoid duplicate vehicles in BeamNG menu
        for v in &all_inputs {
            let disabled_name = format!("{}.merged_backup", v.file_name);
            let disabled_path = v.file_path.with_file_name(disabled_name);
            let result = if disabled_path.exists() {
                crate::bess::replace_file(&v.file_path, &disabled_path)
            } else {
                fs::rename(&v.file_path, disabled_path).map_err(|e| e.to_string())
            };
            result.map_err(|e| {
                format!(
                    "Merged archive saved but could not disable {}: {e}",
                    v.file_name
                )
            })?;
        }

        Ok(output_path)
    }

    /// Unmerges/Degroups a chassis mod: restores original variant ZIPs and removes the merged mod.
    pub fn unmerge_chassis(chassis_name: &str, mods_dir: &Path) -> Result<(), String> {
        let _lock = crate::bess::ModsLock::acquire(mods_dir)?;
        let chassis_slug = slugify(chassis_name);
        let backup_dir = mods_dir.join(".babm_backup").join(&chassis_slug);
        let manifest_path = backup_dir.join("manifest.json");
        crate::bess::no_links(&manifest_path)?;

        if !manifest_path.is_file() {
            return Err(format!("No merge manifest found for {}", chassis_name));
        }

        let manifest_data = fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
        let manifest: MergeManifest =
            serde_json::from_str(&manifest_data).map_err(|e| format!("Corrupted manifest: {e}"))?;

        // Validate every restoration source before changing the active pack.
        if !crate::bess::safe_leaf(&manifest.merged_mod_file) {
            return Err("Unsafe merged archive path".into());
        }
        let merged_zip_path = mods_dir.join(&manifest.merged_mod_file);
        crate::bess::no_links(&merged_zip_path)?;
        if !merged_zip_path.is_file() {
            return Err("The merged archive is missing".into());
        }
        let mut restore = Vec::new();
        let mut names = HashSet::new();
        for backup in &manifest.original_files {
            if !crate::bess::safe_leaf(&backup.original_filename)
                || !crate::bess::safe_leaf(&backup.backup_filename)
                || !names.insert(backup.original_filename.clone())
            {
                return Err("Unsafe or duplicate original filename".into());
            }
            let destination = mods_dir.join(&backup.original_filename);
            if destination.exists() {
                return Err(format!(
                    "An original already exists: {}",
                    destination.display()
                ));
            }
            let backup_file = backup_dir.join(&backup.backup_filename);
            let disabled_file =
                mods_dir.join(format!("{}.merged_backup", backup.original_filename));
            let source = if disabled_file.is_file() {
                disabled_file
            } else {
                backup_file
            };
            if !source.is_file() {
                return Err(format!(
                    "Original backup is missing: {}",
                    backup.original_filename
                ));
            }
            let hash = crate::bess::sha256_file(&source)?;
            restore.push((source, destination, hash));
        }
        let mut staged = Vec::new();
        let mut restored = Vec::new();
        let result = (|| {
            for (source, destination, hash) in &restore {
                let temporary = crate::bess::unique_path(mods_dir, ".babm-restore", "tmp");
                staged.push((temporary.clone(), destination.clone()));
                let mut input = File::open(source).map_err(|e| e.to_string())?;
                let mut out = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)
                    .map_err(|e| e.to_string())?;
                std::io::copy(&mut input, &mut out).map_err(|e| e.to_string())?;
                out.sync_all().map_err(|e| e.to_string())?;
                drop(out);
                if crate::bess::sha256_file(&temporary)? != *hash {
                    return Err("Original restoration verification failed".into());
                }
            }
            for (temporary, destination) in &staged {
                fs::rename(temporary, destination).map_err(|e| e.to_string())?;
                restored.push(destination.clone());
            }
            // Retain the pack and all original snapshots for recovery. No
            // sound/variable changes are discarded by deleting the backup tree.
            let archived = crate::bess::unique_path(&backup_dir, "before-unmerge", "zip");
            fs::rename(&merged_zip_path, archived).map_err(|e| e.to_string())?;
            Ok(())
        })();
        for (temporary, _) in staged {
            if temporary.exists() {
                let _ = fs::remove_file(temporary);
            }
        }
        if result.is_err() {
            for destination in restored {
                let _ = fs::remove_file(destination);
            }
        }
        result
    }

    /// Isolates a single variant into a standalone Automation ZIP suitable for BESS synthesis.
    pub fn isolate_variant_for_bess(
        variant_zip: &Path,
        output_dir: &Path,
    ) -> Result<PathBuf, String> {
        crate::bess::validate_single_source(variant_zip)?;
        let out_file = output_dir.join(format!(
            "bess_ready_{}",
            variant_zip.file_name().unwrap().to_string_lossy()
        ));
        fs::copy(variant_zip, &out_file).map_err(|e| e.to_string())?;
        Ok(out_file)
    }
}

/// Appends variant name tag (e.g. `[A]`) and/or BESS tag (`[BESS]`)
/// to all part display names inside JBeam `"information": { "name": "..." }` blocks.
pub fn tag_jbeam_part_names(text: &str, trim_name: &str, is_bess: bool) -> String {
    if trim_name.is_empty() && !is_bess {
        return text.to_string();
    }

    let bytes = text.as_bytes();
    let mut result = String::with_capacity(text.len() + 256);
    let mut i = 0;

    let trim_tag = if !trim_name.is_empty() {
        format!("[{}]", trim_name)
    } else {
        String::new()
    };

    while i < bytes.len() {
        // Match `"information"`
        if bytes[i..].starts_with(b"\"information\"") {
            result.push_str("\"information\"");
            i += "\"information\"".len();

            // Match through whitespace and `:`
            while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b':') {
                result.push(bytes[i] as char);
                i += 1;
            }

            // Match whitespace until `{`
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                result.push(bytes[i] as char);
                i += 1;
            }

            if i < bytes.len() && bytes[i] == b'{' {
                result.push('{');
                i += 1;

                // Inside `"information": { ... }`
                let mut depth = 1;
                while i < bytes.len() && depth > 0 {
                    if bytes[i..].starts_with(b"//") {
                        while i < bytes.len() && bytes[i] != b'\n' {
                            result.push(bytes[i] as char);
                            i += 1;
                        }
                        continue;
                    }
                    if bytes[i..].starts_with(b"/*") {
                        result.push_str("/*");
                        i += 2;
                        while i < bytes.len() && !bytes[i..].starts_with(b"*/") {
                            result.push(bytes[i] as char);
                            i += 1;
                        }
                        if i < bytes.len() {
                            result.push_str("*/");
                            i += 2;
                        }
                        continue;
                    }

                    // Check for `"name"`
                    if bytes[i..].starts_with(b"\"name\"") {
                        result.push_str("\"name\"");
                        i += "\"name\"".len();

                        // Consume whitespace and `:`
                        while i < bytes.len()
                            && (bytes[i].is_ascii_whitespace() || bytes[i] == b':')
                        {
                            result.push(bytes[i] as char);
                            i += 1;
                        }
                        // Consume whitespace until `"`
                        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                            result.push(bytes[i] as char);
                            i += 1;
                        }

                        if i < bytes.len() && bytes[i] == b'"' {
                            result.push('"');
                            i += 1;
                            let val_start = i;
                            while i < bytes.len() && bytes[i] != b'"' {
                                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                                    i += 2;
                                } else {
                                    i += 1;
                                }
                            }
                            let val = &text[val_start..i];
                            let mut new_val = val.to_string();

                            // Add trim tag if not present
                            if !trim_tag.is_empty() && !new_val.contains(&trim_tag) {
                                new_val = format!("{} {}", new_val.trim_end(), trim_tag);
                            }

                            // Add BESS tag if BESS and not present
                            if is_bess && !new_val.contains("[BESS]") {
                                new_val = format!("{} [BESS]", new_val.trim_end());
                            }

                            result.push_str(&new_val);

                            if i < bytes.len() && bytes[i] == b'"' {
                                result.push('"');
                                i += 1;
                            }
                            continue;
                        }
                    }

                    if bytes[i] == b'{' {
                        depth += 1;
                    } else if bytes[i] == b'}' {
                        depth -= 1;
                    }

                    result.push(bytes[i] as char);
                    i += 1;
                }
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_jbeam_part_names() {
        let sample = r#"{
    "Camso_Engine_87fb0": {
        "information":{
            "authors":"Camshaft Software",
            "name":"I1000 4B",
            "value":6000
        },
        "slotType": "Camso_Engine"
    },
    "Camso_Transmission_19efe": {
        "information": {
            "name": "A 4-Speed Manual Transmission",
            "value": 1100
        }
    }
}"#;

        let tagged = tag_jbeam_part_names(sample, "A", false);
        assert!(tagged.contains("\"name\":\"I1000 4B [A]\""));
        assert!(tagged.contains("\"name\": \"A 4-Speed Manual Transmission [A]\""));

        let tagged_bess = tag_jbeam_part_names(sample, "A", true);
        assert!(tagged_bess.contains("\"name\":\"I1000 4B [A] [BESS]\""));
        assert!(tagged_bess.contains("\"name\": \"A 4-Speed Manual Transmission [A] [BESS]\""));
    }
}
