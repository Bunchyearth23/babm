use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct DetectedPaths {
    pub beamng_mods_dirs: Vec<PathBuf>,
    pub beamng_user_dir: Option<PathBuf>,
    pub automation_user_dir: Option<PathBuf>,
    pub automation_exports_dir: Option<PathBuf>,
}

impl DetectedPaths {
    pub fn detect() -> Self {
        let mut result = Self::default();
        let local_appdata = dirs::data_local_dir();

        if let Some(ref appdata) = local_appdata {
            // 1. Check BeamNG ini: %LOCALAPPDATA%\BeamNG\BeamNG.drive.ini
            let ini_path = appdata.join("BeamNG").join("BeamNG.drive.ini");
            if ini_path.is_file() {
                if let Ok(content) = fs::read_to_string(&ini_path) {
                    for line in content.lines() {
                        let line = line.trim();
                        if let Some((key, val)) = line.split_once('=') {
                            if key.trim() == "userFolder" {
                                let user_dir = PathBuf::from(val.trim());
                                if user_dir.exists() {
                                    result.beamng_user_dir = Some(user_dir.clone());
                                    // Check candidate mods subfolders
                                    check_and_add_mods_dir(&user_dir.join("current").join("mods"), &mut result.beamng_mods_dirs);
                                    check_and_add_mods_dir(&user_dir.join("mods"), &mut result.beamng_mods_dirs);

                                    // Check any version-numbered subfolders (e.g. 0.39, 0.38)
                                    if let Ok(entries) = fs::read_dir(&user_dir) {
                                        for entry in entries.flatten() {
                                            let p = entry.path();
                                            if p.is_dir() {
                                                check_and_add_mods_dir(&p.join("mods"), &mut result.beamng_mods_dirs);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 2. Check %LOCALAPPDATA%\BeamNG.drive\
            let beamng_drive_dir = appdata.join("BeamNG.drive");
            if beamng_drive_dir.is_dir() {
                check_and_add_mods_dir(&beamng_drive_dir.join("mods"), &mut result.beamng_mods_dirs);
                if let Ok(entries) = fs::read_dir(&beamng_drive_dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_dir() {
                            check_and_add_mods_dir(&p.join("mods"), &mut result.beamng_mods_dirs);
                        }
                    }
                }
            }

            // 3. Check Automation: %LOCALAPPDATA%\AutomationGame\Saved\UserData
            let auto_dir = appdata.join("AutomationGame").join("Saved").join("UserData");
            if auto_dir.is_dir() {
                result.automation_user_dir = Some(auto_dir.clone());
                let exports = auto_dir.join("CarSaveExport");
                if exports.is_dir() {
                    result.automation_exports_dir = Some(exports);
                }
            }
        }

        // Deduplicate paths
        result.beamng_mods_dirs.sort();
        result.beamng_mods_dirs.dedup();

        result
    }
}

fn check_and_add_mods_dir(path: &Path, list: &mut Vec<PathBuf>) {
    if path.is_dir() && !list.contains(&path.to_path_buf()) {
        list.push(path.to_path_buf());
    }
}
