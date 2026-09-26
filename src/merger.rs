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
            return Err("Aucune variante sélectionnée pour la fusion.".into());
        }

        let chassis_slug = slugify(chassis_name);
        let output_filename = format!("babm_{}.zip", chassis_slug);
        let output_path = mods_dir.join(&output_filename);

        let backup_dir = mods_dir.join(".babm_backup").join(&chassis_slug);
        fs::create_dir_all(&backup_dir).map_err(|e| format!("Erreur création dossier backup: {e}"))?;

        // 1. Collect and backup original files
        let mut original_backups = Vec::new();
        for v in variants {
            let backup_dest = backup_dir.join(&v.file_name);
            fs::copy(&v.file_path, &backup_dest)
                .map_err(|e| format!("Erreur backup de {}: {e}", v.file_name))?;

            original_backups.push(OriginalFileBackup {
                original_filename: v.file_name.clone(),
                original_path: v.file_path.clone(),
                backup_filename: v.file_name.clone(),
            });
        }

        // 2. Prepare merged archive
        let out_file = File::create(&output_path)
            .map_err(|e| format!("Impossible de créer {}: {e}", output_path.display()))?;
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

            for i in 0..archive.len() {
                let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
                let original_name = entry.name().to_string();

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
                    if entry.read_to_string(&mut text).is_ok() {
                        if let Ok(v) = serde_json::from_str::<Value>(&text) {
                            if let Some(paints) = v.get("paints").and_then(|p| p.as_object()) {
                                if let Some(comb) = combined_paints.as_object_mut() {
                                    for (k, val) in paints {
                                        if !comb.contains_key(k) {
                                            comb.insert(k.clone(), val.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    continue;
                }

                if new_entry_name == format!("{}camso_core.jbeam", new_vehicle_prefix) {
                    // Extract the default trim part for slot
                    let mut text = String::new();
                    if entry.read_to_string(&mut text).is_ok() && first_trim_part.is_none() {
                        if let Some(pos) = text.find("\"Camso_Trim\"") {
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
                        let modified_text = text.replace(&old_ref, &new_ref);

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
            .write_all(serde_json::to_string_pretty(&unified_info).unwrap().as_bytes())
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

        zip_writer.finish().map_err(|e| format!("Erreur finalisation ZIP: {e}"))?;

        // 5. Write Manifest
        let manifest = MergeManifest {
            chassis_name: chassis_name.to_string(),
            chassis_slug: chassis_slug.clone(),
            merged_mod_file: output_filename.clone(),
            original_files: original_backups,
            created_at: format!("{:?}", std::time::SystemTime::now()),
        };
        let manifest_path = backup_dir.join("manifest.json");
        let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
        fs::write(manifest_path, manifest_json).map_err(|e| e.to_string())?;

        // 6. Move/disable original files to avoid duplicate vehicles in BeamNG menu
        for v in variants {
            let disabled_name = format!("{}.merged_backup", v.file_name);
            let disabled_path = v.file_path.with_file_name(disabled_name);
            let _ = fs::rename(&v.file_path, disabled_path);
        }

        Ok(output_path)
    }

    /// Unmerges/Degroups a chassis mod: restores original variant ZIPs and removes the merged mod.
    pub fn unmerge_chassis(chassis_name: &str, mods_dir: &Path) -> Result<(), String> {
        let chassis_slug = slugify(chassis_name);
        let backup_dir = mods_dir.join(".babm_backup").join(&chassis_slug);
        let manifest_path = backup_dir.join("manifest.json");

        if !manifest_path.is_file() {
            return Err(format!("Aucun manifest de fusion trouvé pour {}", chassis_name));
        }

        let manifest_data = fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
        let manifest: MergeManifest = serde_json::from_str(&manifest_data)
            .map_err(|e| format!("Manifeste corrompu: {e}"))?;

        // 1. Remove merged zip
        let merged_zip_path = mods_dir.join(&manifest.merged_mod_file);
        if merged_zip_path.is_file() {
            fs::remove_file(&merged_zip_path)
                .map_err(|e| format!("Impossible de supprimer {}: {e}", merged_zip_path.display()))?;
        }

        // 2. Restore original files
        for backup in manifest.original_files {
            let backup_file = backup_dir.join(&backup.backup_filename);
            let disabled_file = backup.original_path.with_file_name(format!("{}.merged_backup", backup.original_filename));

            if disabled_file.is_file() {
                // If .merged_backup exists in mods dir, rename back
                let _ = fs::rename(&disabled_file, &backup.original_path);
            } else if backup_file.is_file() {
                // Otherwise copy from backup dir
                let _ = fs::copy(&backup_file, &backup.original_path);
            }
        }

        // 3. Clean up backup directory
        let _ = fs::remove_dir_all(&backup_dir);

        Ok(())
    }

    /// Isolates a single variant into a standalone Automation ZIP suitable for BESS synthesis.
    pub fn isolate_variant_for_bess(
        variant_zip: &Path,
        output_dir: &Path,
    ) -> Result<PathBuf, String> {
        // If variant_zip is already an original variant zip, verify it works for BESS
        if !variant_zip.is_file() {
            return Err("Fichier introuvable".into());
        }
        let out_file = output_dir.join(format!("bess_ready_{}", variant_zip.file_name().unwrap().to_string_lossy()));
        fs::copy(variant_zip, &out_file).map_err(|e| e.to_string())?;
        Ok(out_file)
    }
}
