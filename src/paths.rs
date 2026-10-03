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
        Self::detect_at(dirs::data_local_dir().as_deref())
    }

    fn detect_at(local_appdata: Option<&Path>) -> Self {
        let mut result = Self::default();

        if let Some(appdata) = local_appdata {
            // 1. Check BeamNG ini: %LOCALAPPDATA%\BeamNG\BeamNG.drive.ini
            let ini_path = appdata.join("BeamNG").join("BeamNG.drive.ini");
            if ini_path.is_file()
                && let Ok(content) = fs::read_to_string(&ini_path)
            {
                for line in content.lines() {
                    let line = line.trim();
                    if let Some((key, val)) = line.split_once('=')
                        && key.trim() == "userFolder"
                    {
                        let user_dir = PathBuf::from(val.trim().trim_matches('"'));
                        if user_dir.exists() {
                            result.beamng_user_dir = Some(user_dir.clone());
                            // Check candidate mods subfolders
                            check_and_add_mods_dir(
                                &user_dir.join("current").join("mods"),
                                &mut result.beamng_mods_dirs,
                            );
                            check_and_add_mods_dir(
                                &user_dir.join("mods"),
                                &mut result.beamng_mods_dirs,
                            );

                            // Check any version-numbered subfolders (e.g. 0.39, 0.38)
                            if let Ok(entries) = fs::read_dir(&user_dir) {
                                for entry in entries.flatten() {
                                    let p = entry.path();
                                    if p.is_dir() {
                                        check_and_add_mods_dir(
                                            &p.join("mods"),
                                            &mut result.beamng_mods_dirs,
                                        );
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
                check_and_add_mods_dir(
                    &beamng_drive_dir.join("mods"),
                    &mut result.beamng_mods_dirs,
                );
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
            let auto_dir = appdata
                .join("AutomationGame")
                .join("Saved")
                .join("UserData");
            if auto_dir.is_dir() {
                result.automation_user_dir = Some(auto_dir.clone());
                let exports = auto_dir.join("CarSaveExport");
                if exports.is_dir() {
                    result.automation_exports_dir = Some(exports);
                }
            }
        }

        // check_and_add_mods_dir already deduplicates. Preserve discovery priority:
        // the launcher's current user folder must precede old version folders.

        result
    }
}

fn check_and_add_mods_dir(path: &Path, list: &mut Vec<PathBuf>) {
    if path.is_dir() && !list.contains(&path.to_path_buf()) {
        list.push(path.to_path_buf());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_current_mods_stays_first_before_legacy_folders() {
        let temp = std::env::temp_dir().join(format!(
            "babm-paths-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let appdata = temp.join("appdata");
        let user = temp.join("custom-user");
        let current = user.join("current/mods");
        for dir in [
            appdata.join("BeamNG"),
            appdata.join("BeamNG.drive/0.30/mods"),
            current.clone(),
            user.join("0.39/mods"),
            user.join("mods"),
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(
            appdata.join("BeamNG/BeamNG.drive.ini"),
            format!("userFolder=\"{}\"\n", user.display()),
        )
        .unwrap();
        let found = DetectedPaths::detect_at(Some(&appdata));
        assert_eq!(found.beamng_user_dir, Some(user));
        assert_eq!(found.beamng_mods_dirs.first(), Some(&current));
        assert_eq!(
            found
                .beamng_mods_dirs
                .iter()
                .filter(|p| *p == &current)
                .count(),
            1
        );
        fs::remove_dir_all(temp).unwrap();
    }
}
