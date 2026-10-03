//! BESS full-vehicle interchange. Discovery is read-only; applying replaces only
//! the declared audio entries and this module's revision metadata.
use crate::merger::MergeManifest;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

pub const MARKER: &str = "bess-export.json";
const STATE: &str = "babm-bess-updates.json";
const MAX_ENTRIES: usize = 30_000;
const MAX_METADATA: u64 = 1_048_576;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Sound {
    pub path: String,
    pub original_sha256: String,
    pub rendered_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ExportMarker {
    pub version: u32,
    pub kind: String,
    pub source_archive_sha256: String,
    pub source_archive_name: String,
    pub vehicle_root: String,
    pub blend_path: String,
    pub exported_at_unix_ms: u64,
    pub sounds: Vec<Sound>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    Ready,
    AlreadyApplied,
    Stale,
    Conflict,
    Unavailable,
}

#[derive(Clone, Debug, Serialize)]
pub struct BessUpdate {
    pub export_path: PathBuf,
    pub vehicle_name: String,
    pub target_path: Option<PathBuf>,
    pub exported_at_unix_ms: u64,
    pub sounds: usize,
    pub status: UpdateStatus,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApplyReceipt {
    pub target_path: PathBuf,
    pub backup_path: PathBuf,
    pub sounds: usize,
    pub vehicle_name: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct AppliedState {
    version: u32,
    #[serde(default)]
    sources: BTreeMap<String, AppliedSource>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AppliedSource {
    exported_at_unix_ms: u64,
    sounds: BTreeMap<String, String>,
}

#[derive(Clone)]
struct Target {
    path: PathBuf,
    chassis: Option<String>,
    other_sources: Vec<PathBuf>,
    baseline: Option<ExportMarker>,
}

struct Plan {
    update: BessUpdate,
    marker: ExportMarker,
    target: Target,
    state: AppliedState,
    target_hash: String,
    export_hash: String,
    replacements: BTreeMap<String, Sound>,
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    no_links(path)?;
    hash_reader(File::open(path).map_err(|e| e.to_string())?)
}

fn hash_reader(mut reader: impl Read) -> Result<String, String> {
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn safe_member(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':', '\0'])
        && !path.starts_with('/')
        && path.split('/').all(|c| {
            !c.is_empty()
                && c != "."
                && c != ".."
                && !c.ends_with(['.', ' '])
                && !c.chars().any(char::is_control)
        })
}

fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            m.file_type().is_symlink() || m.file_attributes() & 0x400 != 0
        }
        #[cfg(not(windows))]
        {
            m.file_type().is_symlink()
        }
    })
}

pub(crate) fn no_links(path: &Path) -> Result<(), String> {
    if path.ancestors().any(is_link) {
        Err(format!(
            "Linked or redirected paths are not accepted here: {}",
            path.display()
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn safe_leaf(value: &str) -> bool {
    safe_member(value) && !value.contains('/')
}

fn archive(path: &Path) -> Result<ZipArchive<File>, String> {
    no_links(path)?;
    let z =
        ZipArchive::new(File::open(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if z.len() > MAX_ENTRIES {
        return Err("Archive contains too many entries".into());
    }
    let mut names = BTreeSet::new();
    for name in z.file_names() {
        if !names.insert(name.to_string()) {
            return Err("Duplicate archive entry".into());
        }
    }
    Ok(z)
}

fn read_json<T: for<'a> Deserialize<'a>>(
    z: &mut ZipArchive<File>,
    name: &str,
) -> Result<T, String> {
    let entry = z.by_name(name).map_err(|e| e.to_string())?;
    if entry.size() > MAX_METADATA {
        return Err("Metadata exceeds size limit".into());
    }
    serde_json::from_reader(entry).map_err(|e| e.to_string())
}

pub fn read_marker(path: &Path) -> Result<Option<ExportMarker>, String> {
    let mut z = archive(path)?;
    if !z.file_names().any(|n| n == MARKER) {
        return Ok(None);
    }
    let marker: ExportMarker = read_json(&mut z, MARKER)?;
    let root = marker
        .vehicle_root
        .strip_prefix("vehicles/")
        .and_then(|s| s.strip_suffix('/'))
        .filter(|s| safe_leaf(s));
    if marker.version != 1
        || marker.kind != "bess-full-vehicle"
        || root.is_none()
        || !valid_hash(&marker.source_archive_sha256)
        || !safe_leaf(&marker.source_archive_name)
        || !safe_member(&marker.blend_path)
        || marker.exported_at_unix_ms == 0
        || marker.sounds.is_empty()
        || marker.sounds.len() > 4096
    {
        return Err("Unsupported or invalid BESS full-vehicle marker".into());
    }
    if !z
        .file_names()
        .any(|n| n == format!("{}info.json", marker.vehicle_root))
    {
        return Err("BESS vehicle root is missing".into());
    }
    let mut paths = BTreeSet::new();
    if !z.file_names().any(|n| n == marker.blend_path) {
        return Err("BESS blend provenance is missing".into());
    }
    for sound in &marker.sounds {
        if !safe_member(&sound.path)
            || !sound.path.to_ascii_lowercase().ends_with(".wav")
            || !paths.insert(sound.path.clone())
            || !valid_hash(&sound.original_sha256)
            || !valid_hash(&sound.rendered_sha256)
        {
            return Err("Invalid BESS sound declaration".into());
        }
        let entry = z
            .by_name(&sound.path)
            .map_err(|_| format!("Missing sound: {}", sound.path))?;
        if entry.size() > 512 * 1024 * 1024 || hash_reader(entry)? != sound.rendered_sha256 {
            return Err(format!("BESS sound checksum mismatch: {}", sound.path));
        }
    }
    Ok(Some(marker))
}

fn entries(dir: &Path) -> Result<Vec<PathBuf>, String> {
    no_links(dir)?;
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())?.take(4097) {
        let entry = entry.map_err(|e| e.to_string())?;
        if paths.len() >= 4096 {
            return Err("Directory entry limit exceeded".into());
        }
        if !is_link(&entry.path()) {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn is_zip(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

fn manifests(mods_dir: &Path) -> Result<Vec<(PathBuf, MergeManifest)>, String> {
    let mut result = Vec::new();
    for folder in entries(&mods_dir.join(".babm_backup"))? {
        let path = folder.join("manifest.json");
        if path.is_file() {
            no_links(&path)?;
            if fs::metadata(&path).map_err(|e| e.to_string())?.len() > MAX_METADATA {
                return Err("Merge manifest exceeds size limit".into());
            }
            let m: MergeManifest =
                serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| format!("Invalid merge manifest {}: {e}", path.display()))?;
            if !safe_leaf(&m.chassis_slug)
                || !safe_leaf(&m.merged_mod_file)
                || m.original_files
                    .iter()
                    .any(|f| !safe_leaf(&f.backup_filename) || !safe_leaf(&f.original_filename))
            {
                return Err("Unsafe merge manifest path".into());
            }
            result.push((folder, m));
        }
    }
    Ok(result)
}

fn original_backup(
    mods_dir: &Path,
    folder: &Path,
    item: &crate::merger::OriginalFileBackup,
) -> Option<PathBuf> {
    let stored = folder.join(&item.backup_filename);
    if stored.is_file() {
        return Some(stored);
    }
    let disabled = mods_dir.join(format!("{}.merged_backup", item.original_filename));
    disabled.is_file().then_some(disabled)
}

fn find_target(mods_dir: &Path, marker: &ExportMarker) -> Result<Target, String> {
    let mut targets = Vec::new();
    for (folder, manifest) in manifests(mods_dir)? {
        let target = mods_dir.join(&manifest.merged_mod_file);
        if !target.is_file() {
            continue;
        }
        let mut matched = false;
        let mut others = Vec::new();
        let mut baseline = None;
        let mut incomplete = false;
        for item in &manifest.original_files {
            if let Some(source) = original_backup(mods_dir, &folder, item) {
                let hash = sha256_file(&source)?;
                let source_marker = read_marker(&source)?;
                let identity = source_marker
                    .as_ref()
                    .map_or(hash.as_str(), |m| m.source_archive_sha256.as_str());
                if identity == marker.source_archive_sha256 {
                    matched = true;
                    if source_marker.as_ref().is_some_and(|m| {
                        baseline.as_ref().is_none_or(|old: &ExportMarker| {
                            m.exported_at_unix_ms > old.exported_at_unix_ms
                        })
                    }) {
                        baseline = source_marker;
                    }
                } else {
                    others.push(source);
                }
            } else {
                incomplete = true;
            }
        }
        if matched {
            if incomplete {
                return Err(
                    "A source backup is missing; ownership of all pack sounds cannot be verified"
                        .into(),
                );
            }
            let standalone = folder.file_name().is_some_and(|n| {
                n == format!("bess-original-{}", marker.source_archive_sha256).as_str()
            });
            targets.push(Target {
                path: target,
                chassis: if standalone {
                    None
                } else {
                    Some(manifest.chassis_slug)
                },
                other_sources: others,
                baseline,
            });
        }
    }
    for path in entries(mods_dir)? {
        if !is_zip(&path)
            || path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("babm_"))
        {
            continue;
        }
        let hash = sha256_file(&path)?;
        let source_marker = read_marker(&path)?;
        let identity = source_marker
            .as_ref()
            .map_or(hash.as_str(), |m| m.source_archive_sha256.as_str());
        if identity == marker.source_archive_sha256 {
            if targets.iter().any(|t| t.path == path) {
                continue;
            }
            targets.push(Target {
                path,
                chassis: None,
                other_sources: Vec::new(),
                baseline: source_marker,
            });
        }
    }
    match targets.len() {
        0 => Err("No installed original or BABM pack matches this source; the complete ZIP can also be used in a new merge".into()),
        1 => Ok(targets.remove(0)),
        _ => Err("Multiple installed targets match this source; keep a single active original or merged pack".into()),
    }
}

fn mapped_path(path: &str, root: &str, chassis: Option<&str>) -> String {
    match (path.strip_prefix(root), chassis) {
        (Some(rest), Some(chassis)) => format!("vehicles/{chassis}/{rest}"),
        _ => path.to_string(),
    }
}

fn state(z: &mut ZipArchive<File>) -> Result<AppliedState, String> {
    if !z.file_names().any(|n| n == STATE) {
        return Ok(AppliedState {
            version: 1,
            ..Default::default()
        });
    }
    let state: AppliedState = read_json(z, STATE)?;
    if state.version != 1 {
        return Err("Unsupported BABM audio revision state".into());
    }
    Ok(state)
}

fn prepare(mods_dir: &Path, export_path: &Path) -> Result<Plan, String> {
    let marker = read_marker(export_path)?
        .ok_or("This archive is not a marked BESS complete-vehicle export")?;
    let target = find_target(mods_dir, &marker)?;
    let mut z = archive(&target.path)?;
    let applied = state(&mut z)?;
    let baseline = target.baseline.as_ref().map(|m| AppliedSource {
        exported_at_unix_ms: m.exported_at_unix_ms,
        sounds: m
            .sounds
            .iter()
            .map(|s| {
                (
                    mapped_path(&s.path, &m.vehicle_root, target.chassis.as_deref()),
                    s.rendered_sha256.clone(),
                )
            })
            .collect(),
    });
    let previous = applied
        .sources
        .get(&marker.source_archive_sha256)
        .or(baseline.as_ref());
    let mut replacements = BTreeMap::new();
    for sound in &marker.sounds {
        let path = mapped_path(&sound.path, &marker.vehicle_root, target.chassis.as_deref());
        if replacements.insert(path, sound.clone()).is_some() {
            return Err("Conflicting mapped sounds".into());
        }
    }
    // A global audio resource may belong to several trims. Replacing it would
    // alter another variant, so require a unique owner instead of first-wins.
    for source in &target.other_sources {
        let other = archive(source)?;
        let roots: BTreeSet<String> = other
            .file_names()
            .filter_map(|name| {
                let rest = name.strip_prefix("vehicles/")?;
                Some(format!("vehicles/{}/", rest.split('/').next()?))
            })
            .collect();
        for name in other
            .file_names()
            .filter(|n| n.to_ascii_lowercase().ends_with(".wav"))
        {
            let mapped = roots
                .iter()
                .find(|r| name.starts_with(r.as_str()))
                .map_or_else(
                    || name.to_string(),
                    |root| mapped_path(name, root, target.chassis.as_deref()),
                );
            if replacements.contains_key(&mapped) {
                return Err(format!("Shared sound belongs to another variant: {mapped}"));
            }
        }
    }
    let mut all_applied = true;
    for (path, sound) in &replacements {
        let current = hash_reader(
            z.by_name(path)
                .map_err(|_| format!("Target sound is missing: {path}"))?,
        )?;
        let expected = previous
            .and_then(|s| s.sounds.get(path))
            .unwrap_or(&sound.original_sha256);
        if current != *expected && current != sound.rendered_sha256 {
            return Err(format!(
                "Target sound changed outside this BESS workflow: {path}"
            ));
        }
        all_applied &= current == sound.rendered_sha256;
    }
    let (status, detail) =
        if previous.is_some_and(|s| marker.exported_at_unix_ms < s.exported_at_unix_ms) {
            (
                UpdateStatus::Stale,
                "A newer audio revision is already installed".to_string(),
            )
        } else if previous.is_some_and(|s| marker.exported_at_unix_ms == s.exported_at_unix_ms)
            && !all_applied
        {
            (
                UpdateStatus::Conflict,
                "Different audio revisions share the same timestamp".to_string(),
            )
        } else if all_applied {
            (
                UpdateStatus::AlreadyApplied,
                "These sounds are already installed".to_string(),
            )
        } else {
            (
            UpdateStatus::Ready,
            "Only the exported sounds will be replaced; other variants and settings are preserved"
                .to_string(),
        )
        };
    let vehicle_name = marker
        .vehicle_root
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("Vehicle")
        .to_string();
    let update = BessUpdate {
        export_path: export_path.to_path_buf(),
        vehicle_name,
        target_path: Some(target.path.clone()),
        exported_at_unix_ms: marker.exported_at_unix_ms,
        sounds: marker.sounds.len(),
        status,
        detail,
    };
    let target_hash = sha256_file(&target.path)?;
    Ok(Plan {
        update,
        marker,
        target,
        state: applied,
        target_hash,
        export_hash: sha256_file(export_path)?,
        replacements,
    })
}

pub fn inspect_update(mods_dir: &Path, export_path: &Path) -> Result<BessUpdate, String> {
    match prepare(mods_dir, export_path) {
        Ok(plan) => Ok(plan.update),
        Err(reason) => {
            let marker = read_marker(export_path)?;
            let Some(marker) = marker else {
                return Err(reason);
            };
            let unavailable = reason.starts_with("No installed original");
            Ok(BessUpdate {
                export_path: export_path.to_path_buf(),
                vehicle_name: marker
                    .vehicle_root
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("Vehicle")
                    .to_string(),
                target_path: None,
                exported_at_unix_ms: marker.exported_at_unix_ms,
                sounds: marker.sounds.len(),
                status: if unavailable {
                    UpdateStatus::Unavailable
                } else {
                    UpdateStatus::Conflict
                },
                detail: reason,
            })
        }
    }
}

fn candidate_paths(
    mods_dir: &Path,
    exports_dir: Option<&Path>,
) -> Result<BTreeSet<PathBuf>, String> {
    let mut roots = BTreeSet::new();
    if let Some(dir) = exports_dir {
        roots.insert(dir.to_path_buf());
    } else {
        if let Some(parent) = mods_dir.parent() {
            roots.insert(parent.join("BESS-exports"));
        }
        if let Some(local) = dirs::data_local_dir() {
            let prefs = local.join("BESS/beamng-folder.json");
            if let Ok(bytes) = fs::read(prefs)
                && bytes.len() <= MAX_METADATA as usize
                && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
                && let Some(path) = value.get("export_dir").and_then(|v| v.as_str())
            {
                roots.insert(PathBuf::from(path));
            }
        }
    }
    let mut candidates = BTreeSet::new();
    for root in roots {
        for path in entries(&root)? {
            if is_zip(&path) {
                candidates.insert(path);
            } else if path.is_dir() {
                for child in entries(&path)? {
                    if is_zip(&child) {
                        candidates.insert(child);
                    }
                }
            }
        }
    }
    Ok(candidates)
}

pub fn discover(mods_dir: &Path, exports_dir: Option<&Path>) -> Result<Vec<BessUpdate>, String> {
    let candidates = candidate_paths(mods_dir, exports_dir)?;
    let mut found: Vec<(ExportMarker, BessUpdate)> = Vec::new();
    let mut invalid = Vec::new();
    for path in candidates {
        match read_marker(&path) {
            Ok(Some(marker)) => found.push((marker, inspect_update(mods_dir, &path)?)),
            Ok(None) => {}
            Err(error) => invalid.push(BessUpdate {
                export_path: path,
                vehicle_name: "Invalid BESS export".into(),
                target_path: None,
                exported_at_unix_ms: 0,
                sounds: 0,
                status: UpdateStatus::Conflict,
                detail: error,
            }),
        }
    }
    let mut latest: BTreeMap<String, u64> = BTreeMap::new();
    for (marker, _) in &found {
        latest
            .entry(marker.source_archive_sha256.clone())
            .and_modify(|t| *t = (*t).max(marker.exported_at_unix_ms))
            .or_insert(marker.exported_at_unix_ms);
    }
    for i in 0..found.len() {
        let (marker, _) = &found[i];
        let conflict = found.iter().any(|(other, _)| {
            other.source_archive_sha256 == marker.source_archive_sha256
                && other.exported_at_unix_ms == marker.exported_at_unix_ms
                && other.sounds != marker.sounds
        });
        let stale = latest[&marker.source_archive_sha256] > marker.exported_at_unix_ms;
        if conflict {
            found[i].1.status = UpdateStatus::Conflict;
            found[i].1.detail = "Conflicting exports share the same source and timestamp".into();
        } else if stale {
            found[i].1.status = UpdateStatus::Stale;
            found[i].1.detail = "A newer export exists for this source".into();
        }
    }
    let mut result: Vec<_> = found
        .into_iter()
        .map(|(_, update)| update)
        .chain(invalid)
        .collect();
    result.sort_by(|a, b| {
        a.vehicle_name
            .cmp(&b.vehicle_name)
            .then(b.exported_at_unix_ms.cmp(&a.exported_at_unix_ms))
            .then(a.export_path.cmp(&b.export_path))
    });
    Ok(result)
}

pub(crate) fn unique_path(parent: &Path, prefix: &str, extension: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    parent.join(format!(
        "{prefix}-{}-{nanos}.{extension}",
        std::process::id()
    ))
}

pub(crate) struct ModsLock(PathBuf);
impl ModsLock {
    pub(crate) fn acquire(mods_dir: &Path) -> Result<Self, String> {
        no_links(mods_dir)?;
        let path = mods_dir.join(".babm-write.lock");
        let mut lock = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("Another BABM write is active ({}): {e}", path.display()))?;
        if let Err(error) = writeln!(lock, "{}", std::process::id()) {
            let _ = fs::remove_file(&path);
            return Err(error.to_string());
        }
        Ok(Self(path))
    }
}
impl Drop for ModsLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn preserve_standalone_original(mods_dir: &Path, plan: &Plan) -> Result<(), String> {
    if plan.target.chassis.is_some() {
        return Ok(());
    }
    let folder = mods_dir.join(".babm_backup").join(format!(
        "bess-original-{}",
        plan.marker.source_archive_sha256
    ));
    let manifest_path = folder.join("manifest.json");
    if manifest_path.is_file() {
        no_links(&manifest_path)?;
        return Ok(());
    }
    if plan.target_hash != plan.marker.source_archive_sha256 {
        return Err(
            "The pristine standalone original is missing; restore its original ZIP before applying"
                .into(),
        );
    }
    no_links(&folder)?;
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let filename = plan
        .target
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| safe_leaf(s))
        .ok_or("Invalid target filename")?;
    let backup = folder.join(filename);
    if !backup.exists() {
        let mut input = File::open(&plan.target.path).map_err(|e| e.to_string())?;
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup)
            .map_err(|e| e.to_string())?;
        std::io::copy(&mut input, &mut out).map_err(|e| e.to_string())?;
        out.sync_all().map_err(|e| e.to_string())?;
    }
    if sha256_file(&backup)? != plan.marker.source_archive_sha256 {
        return Err("Pristine standalone backup does not match the source".into());
    }
    let manifest = MergeManifest {
        chassis_name: plan.update.vehicle_name.clone(),
        chassis_slug: plan.update.vehicle_name.clone(),
        merged_mod_file: filename.to_string(),
        original_files: vec![crate::merger::OriginalFileBackup {
            original_filename: filename.to_string(),
            original_path: plan.target.path.clone(),
            backup_filename: filename.to_string(),
        }],
        created_at: "BESS pristine standalone source".into(),
    };
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(manifest_path)
        .map_err(|e| e.to_string())?;
    out.write_all(&serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    out.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

/// Replace an existing file atomically. On Windows ReplaceFileW leaves the
/// destination in place on failure; no delete-first fallback is allowed.
pub(crate) fn replace_file(staged: &Path, target: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn ReplaceFileW(
                replaced: *const u16,
                replacement: *const u16,
                backup: *const u16,
                flags: u32,
                exclude: *mut std::ffi::c_void,
                reserved: *mut std::ffi::c_void,
            ) -> i32;
        }
        let to: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        let from: Vec<u16> = staged.as_os_str().encode_wide().chain(Some(0)).collect();
        // Both paths refer to closed, fully validated files on the same volume.
        let ok = unsafe {
            ReplaceFileW(
                to.as_ptr(),
                from.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(format!(
                "Cannot replace target safely: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(staged, target).map_err(|e| e.to_string())
    }
}

pub fn apply_update(mods_dir: &Path, export_path: &Path) -> Result<ApplyReceipt, String> {
    let _lock = ModsLock::acquire(mods_dir)?;
    let mut plan = prepare(mods_dir, export_path)?;
    if plan.update.status != UpdateStatus::Ready {
        return Err(plan.update.detail);
    }
    let mut known = candidate_paths(mods_dir, None)?;
    if let Some(parent) = export_path.parent() {
        known.extend(candidate_paths(mods_dir, Some(parent))?);
        if let Some(parent) = parent.parent() {
            known.extend(candidate_paths(mods_dir, Some(parent))?);
        }
    }
    for path in known {
        if path == export_path {
            continue;
        }
        if let Ok(Some(other)) = read_marker(&path)
            && other.source_archive_sha256 == plan.marker.source_archive_sha256
        {
            if other.exported_at_unix_ms > plan.marker.exported_at_unix_ms {
                return Err(
                    "A newer export is available for this source; apply the latest revision".into(),
                );
            }
            if other.exported_at_unix_ms == plan.marker.exported_at_unix_ms
                && other.sounds != plan.marker.sounds
            {
                return Err("Conflicting exports share the same source and timestamp".into());
            }
        }
    }
    let folder = mods_dir
        .join(".babm_backup")
        .join(plan.target.chassis.as_deref().unwrap_or("bess-standalone"))
        .join("bess-history");
    let staged = unique_path(mods_dir, ".bess-stage", "tmp");
    let result = (|| {
        let mut original = archive(&plan.target.path)?;
        let mut export = archive(export_path)?;
        let out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
            .map_err(|e| e.to_string())?;
        let mut writer = ZipWriter::new(out);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for i in 0..original.len() {
            let entry = original.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.name().to_string();
            if name == STATE || (name == MARKER && plan.target.chassis.is_none()) {
                continue;
            }
            if let Some(sound) = plan.replacements.get(&name) {
                writer
                    .start_file(&name, options)
                    .map_err(|e| e.to_string())?;
                std::io::copy(
                    &mut export.by_name(&sound.path).map_err(|e| e.to_string())?,
                    &mut writer,
                )
                .map_err(|e| e.to_string())?;
            } else {
                writer.raw_copy_file(entry).map_err(|e| e.to_string())?;
            }
        }
        plan.state.sources.insert(
            plan.marker.source_archive_sha256.clone(),
            AppliedSource {
                exported_at_unix_ms: plan.marker.exported_at_unix_ms,
                sounds: plan
                    .replacements
                    .iter()
                    .map(|(p, s)| (p.clone(), s.rendered_sha256.clone()))
                    .collect(),
            },
        );
        writer
            .start_file(STATE, options)
            .map_err(|e| e.to_string())?;
        writer
            .write_all(&serde_json::to_vec(&plan.state).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if plan.target.chassis.is_none() {
            writer
                .start_file(MARKER, options)
                .map_err(|e| e.to_string())?;
            writer
                .write_all(&serde_json::to_vec(&plan.marker).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        }
        writer
            .finish()
            .map_err(|e| e.to_string())?
            .sync_all()
            .map_err(|e| e.to_string())?;
        drop(original);
        drop(export);
        let mut check = archive(&staged)?;
        for (path, sound) in &plan.replacements {
            if hash_reader(check.by_name(path).map_err(|e| e.to_string())?)?
                != sound.rendered_sha256
            {
                return Err("Staged sound verification failed".into());
            }
        }
        drop(check);
        if sha256_file(&plan.target.path)? != plan.target_hash
            || sha256_file(export_path)? != plan.export_hash
        {
            return Err("Source or target changed during preparation; refresh and retry".into());
        }
        no_links(&folder)?;
        fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let backup = unique_path(&folder, "before-bess", "zip");
        fs::copy(&plan.target.path, &backup).map_err(|e| e.to_string())?;
        if sha256_file(&backup)? != plan.target_hash {
            return Err("Backup verification failed; target was not changed".into());
        }
        preserve_standalone_original(mods_dir, &plan)?;
        if sha256_file(&plan.target.path)? != plan.target_hash {
            return Err("Target changed before replacement; target was not changed by BABM".into());
        }
        replace_file(&staged, &plan.target.path)?;
        Ok(ApplyReceipt {
            target_path: plan.target.path,
            backup_path: backup,
            sounds: plan.replacements.len(),
            vehicle_name: plan.update.vehicle_name,
        })
    })();
    if staged.exists() {
        let _ = fs::remove_file(staged);
    }
    result
}

pub fn sources_for_bess(mods_dir: &Path, vehicle_zip: &Path) -> Result<Vec<PathBuf>, String> {
    let mut sources = Vec::new();
    let all_manifests = manifests(mods_dir)?;
    let mut wanted = BTreeSet::new();
    let mut grouped = false;
    for (folder, manifest) in &all_manifests {
        if mods_dir.join(&manifest.merged_mod_file) != vehicle_zip {
            continue;
        }
        grouped = true;
        for item in &manifest.original_files {
            if item.original_filename.starts_with("bess-variant-") {
                continue;
            }
            if let Some(path) = original_backup(mods_dir, folder, item)
                && validate_single_source(&path).is_ok()
            {
                if let Some(marker) = read_marker(&path)? {
                    wanted.insert(marker.source_archive_sha256);
                } else {
                    sources.push(path);
                }
            }
        }
    }
    if !grouped {
        validate_single_source(vehicle_zip)?;
        if let Some(marker) = read_marker(vehicle_zip)? {
            wanted.insert(marker.source_archive_sha256);
        } else {
            sources.push(vehicle_zip.to_path_buf());
        }
    }
    // A merged input may itself have been a full BESS export. Its untouched A
    // can live in a prior standalone registry, never use the processed B as A.
    if !wanted.is_empty() {
        for (folder, manifest) in &all_manifests {
            for item in &manifest.original_files {
                if let Some(path) = original_backup(mods_dir, folder, item)
                    && read_marker(&path)?.is_none()
                    && wanted.contains(&sha256_file(&path)?)
                    && validate_single_source(&path).is_ok()
                {
                    sources.push(path);
                }
            }
        }
    }
    sources.sort();
    sources.dedup();
    if sources.is_empty() {
        Err("No original single-vehicle source is available for this pack".into())
    } else {
        Ok(sources)
    }
}

pub(crate) fn validate_single_source(path: &Path) -> Result<(), String> {
    let z = archive(path)?;
    let roots: BTreeSet<_> = z
        .file_names()
        .filter_map(|p| {
            p.strip_prefix("vehicles/")
                .and_then(|p| p.split('/').next())
        })
        .collect();
    let blends = z
        .file_names()
        .filter(|p| p.ends_with(".sfxBlend2D.json"))
        .count();
    let legacy_addon = z.file_names().any(|p| {
        p.rsplit('/')
            .next()
            .is_some_and(|name| name.starts_with("bess_engine_") && name.ends_with(".jbeam"))
    }) && z.file_names().any(|p| {
        p.rsplit('/')
            .next()
            .is_some_and(|name| name.starts_with("bess_") && name.ends_with(".pc"))
    }) && z
        .file_names()
        .any(|p| p.ends_with(".sfxBlend2D.json") && p.contains("_BESS_"));
    if path
        .file_name()
        .is_some_and(|p| p.to_string_lossy().starts_with("babm_"))
        || roots.len() != 1
        || blends != 1
        || legacy_addon
    {
        return Err("Choose an original vehicle from the BABM backups; a grouped pack cannot be edited as one BESS engine".into());
    }
    Ok(())
}

/// A processed full export replaces its exact original in a new merge. Old
/// additive BESS configurations keep their own identity and remain available.
pub fn prefer_bess_variants(
    variants: &[crate::vehicle::VehicleMod],
) -> Result<Vec<crate::vehicle::VehicleMod>, String> {
    let mut selected: BTreeMap<String, (crate::vehicle::VehicleMod, Option<ExportMarker>)> =
        BTreeMap::new();
    for vehicle in variants {
        let marker = read_marker(&vehicle.file_path)?;
        let identity = match &marker {
            Some(m) => m.source_archive_sha256.clone(),
            None => sha256_file(&vehicle.file_path)?,
        };
        if let Some((old, old_marker)) = selected.get(&identity) {
            match (&marker, old_marker) {
                (Some(new), Some(previous)) => {
                    if new.exported_at_unix_ms == previous.exported_at_unix_ms
                        && new.sounds != previous.sounds
                    {
                        return Err(
                            "Conflicting BESS revisions for the same original vehicle".into()
                        );
                    }
                    if new.exported_at_unix_ms <= previous.exported_at_unix_ms {
                        continue;
                    }
                }
                (None, Some(_)) => continue,
                (None, None) if old.file_path <= vehicle.file_path => continue,
                _ => {}
            }
        }
        selected.insert(identity, (vehicle.clone(), marker));
    }
    let mut result: Vec<_> = selected.into_values().map(|(vehicle, _)| vehicle).collect();
    result.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
            .then(a.file_path.cmp(&b.file_path))
    });
    Ok(result)
}

pub(crate) fn check_merge_audio(
    variants: &[crate::vehicle::VehicleMod],
    chassis: &str,
) -> Result<(), String> {
    let mut sounds = BTreeMap::new();
    for vehicle in variants {
        let mut z = archive(&vehicle.file_path)?;
        for i in 0..z.len() {
            let entry = z.by_index(i).map_err(|e| e.to_string())?;
            if !entry.name().to_ascii_lowercase().ends_with(".wav") {
                continue;
            }
            let name = mapped_path(
                entry.name(),
                &format!("vehicles/{}/", vehicle.internal_name),
                Some(chassis),
            );
            let hash = hash_reader(entry)?;
            if sounds.get(&name).is_some_and(|old| old != &hash) {
                return Err(format!(
                    "Different variants share conflicting audio at {name}; no archive was changed"
                ));
            }
            sounds.insert(name, hash);
        }
    }
    Ok(())
}

pub(crate) fn merge_inputs(
    variants: &[crate::vehicle::VehicleMod],
    mods_dir: &Path,
) -> Result<Vec<crate::vehicle::VehicleMod>, String> {
    let mut result: BTreeMap<PathBuf, crate::vehicle::VehicleMod> = variants
        .iter()
        .map(|v| (v.file_path.clone(), v.clone()))
        .collect();
    let mut identities = BTreeSet::new();
    for v in variants {
        if let Some(m) = read_marker(&v.file_path)? {
            identities.insert(m.source_archive_sha256);
        }
    }
    if !identities.is_empty() {
        for path in entries(mods_dir)? {
            if !is_zip(&path) || result.contains_key(&path) {
                continue;
            }
            let marker = read_marker(&path)?;
            let identity = match marker {
                Some(m) => m.source_archive_sha256,
                None => sha256_file(&path)?,
            };
            if identities.contains(&identity) {
                result.insert(path.clone(), crate::scanner::inspect_vehicle_zip(&path)?);
            }
        }
    }
    let result: Vec<_> = result.into_values().collect();
    let mut names = BTreeSet::new();
    for v in &result {
        if !safe_leaf(&v.file_name) || !names.insert(v.file_name.to_lowercase()) {
            return Err("Merge inputs have duplicate or unsafe ZIP filenames; give each input a unique filename".into());
        }
        let disabled = v
            .file_path
            .with_file_name(format!("{}.merged_backup", v.file_name));
        if disabled.exists() && sha256_file(&disabled)? != sha256_file(&v.file_path)? {
            return Err(format!(
                "A previous disabled backup exists for {}; resolve it before merging",
                v.file_name
            ));
        }
    }
    Ok(result)
}

pub(crate) struct StagedFile(pub PathBuf);
impl Drop for StagedFile {
    fn drop(&mut self) {
        if self.0.exists() {
            let _ = fs::remove_file(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merger::{MergeManifest, OriginalFileBackup};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = unique_path(&std::env::temp_dir(), "babm-interop-test", "dir");
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn write_zip(path: &Path, items: &BTreeMap<String, Vec<u8>>) {
        let mut z = ZipWriter::new(File::create(path).unwrap());
        for (name, data) in items {
            z.start_file(
                name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }
    fn contents(path: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut z = archive(path).unwrap();
        let mut items = BTreeMap::new();
        for i in 0..z.len() {
            let mut entry = z.by_index(i).unwrap();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            items.insert(entry.name().to_string(), data);
        }
        items
    }
    fn original(dir: &Path, name: &str, sound_path: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(format!("{name}.zip"));
        let items = BTreeMap::from([
            (
                format!("vehicles/{name}/info.json"),
                br#"{"Name":"Model A","Type":"Automation"}"#.to_vec(),
            ),
            (
                format!("vehicles/{name}/info_A.json"),
                br#"{"Configuration":"A"}"#.to_vec(),
            ),
            (
                format!("vehicles/{name}/tuned.jbeam"),
                br#"{"variables":{"torque":197,"user":true}}"#.to_vec(),
            ),
            (
                format!("vehicles/{name}/sound.sfxBlend2D.json"),
                b"{}".to_vec(),
            ),
            (sound_path.to_string(), b"original-wave".to_vec()),
        ]);
        write_zip(&path, &items);
        path
    }
    fn exported(
        source: &Path,
        out: &Path,
        root: &str,
        sound_path: &str,
        audio: &[u8],
        time: u64,
    ) -> PathBuf {
        fs::create_dir_all(out).unwrap();
        let path = out.join(format!("bess-{time}.zip"));
        let mut items = contents(source);
        let old = items.get(sound_path).unwrap();
        let marker = ExportMarker {
            version: 1,
            kind: "bess-full-vehicle".into(),
            source_archive_sha256: sha256_file(source).unwrap(),
            source_archive_name: source.file_name().unwrap().to_string_lossy().to_string(),
            vehicle_root: format!("vehicles/{root}/"),
            blend_path: format!("vehicles/{root}/sound.sfxBlend2D.json"),
            exported_at_unix_ms: time,
            sounds: vec![Sound {
                path: sound_path.into(),
                original_sha256: hash_reader(old.as_slice()).unwrap(),
                rendered_sha256: hash_reader(audio).unwrap(),
            }],
        };
        items.insert(sound_path.into(), audio.to_vec());
        items.insert(MARKER.into(), serde_json::to_vec(&marker).unwrap());
        write_zip(&path, &items);
        path
    }
    fn pack(mods: &Path, originals: &[PathBuf]) -> PathBuf {
        fs::create_dir_all(mods).unwrap();
        let backup = mods.join(".babm_backup/model");
        fs::create_dir_all(&backup).unwrap();
        let path = mods.join("babm_model.zip");
        let mut items = BTreeMap::new();
        let mut original_files = Vec::new();
        for original in originals {
            let name = original.file_name().unwrap().to_string_lossy().to_string();
            let root = original.file_stem().unwrap().to_string_lossy();
            fs::copy(original, backup.join(&name)).unwrap();
            for (name, data) in contents(original) {
                items.insert(
                    mapped_path(&name, &format!("vehicles/{root}/"), Some("model")),
                    data,
                );
            }
            original_files.push(OriginalFileBackup {
                original_filename: name.clone(),
                original_path: mods.join(&name),
                backup_filename: name,
            });
        }
        items.insert(
            "vehicles/model/legacy_bess.jbeam".into(),
            b"old additive engine part".to_vec(),
        );
        items.insert("art/sound/legacy.wav".into(), b"legacy-audio".to_vec());
        write_zip(&path, &items);
        let manifest = MergeManifest {
            chassis_name: "Model".into(),
            chassis_slug: "model".into(),
            merged_mod_file: "babm_model.zip".into(),
            original_files,
            created_at: "test".into(),
        };
        fs::write(
            backup.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        path
    }
    #[test]
    fn grouped_update_changes_only_declared_audio_and_is_repeatable() {
        let t = Temp::new();
        let source = original(&t.0.join("sources"), "first", "vehicles/first/engine.wav");
        let mods = t.0.join("mods");
        let target = pack(&mods, std::slice::from_ref(&source));
        let before = contents(&target);
        let source_hash = sha256_file(&source).unwrap();
        let export = exported(
            &source,
            &t.0.join("BESS-exports/run"),
            "first",
            "vehicles/first/engine.wav",
            b"rendered-1",
            100,
        );
        assert_eq!(
            inspect_update(&mods, &export).unwrap().status,
            UpdateStatus::Ready
        );
        let receipt = apply_update(&mods, &export).unwrap();
        assert_eq!(contents(&receipt.backup_path), before);
        let after = contents(&target);
        for (name, data) in &before {
            if name == "vehicles/model/engine.wav" {
                assert_eq!(after[name], b"rendered-1");
            } else {
                assert_eq!(&after[name], data, "{name}");
            }
        }
        assert_eq!(
            inspect_update(&mods, &export).unwrap().status,
            UpdateStatus::AlreadyApplied
        );
        assert!(apply_update(&mods, &export).is_err());
        assert_eq!(sha256_file(&source).unwrap(), source_hash);
        let next = exported(
            &source,
            &t.0.join("BESS-exports/new"),
            "first",
            "vehicles/first/engine.wav",
            b"rendered-2",
            101,
        );
        apply_update(&mods, &next).unwrap();
        assert_eq!(
            contents(&target)["vehicles/model/engine.wav"],
            b"rendered-2"
        );
        assert_eq!(
            inspect_update(&mods, &export).unwrap().status,
            UpdateStatus::Stale
        );
    }
    #[test]
    fn standalone_update_keeps_non_audio_and_detects_external_changes() {
        let t = Temp::new();
        let mods = t.0.join("mods");
        let source = original(&mods, "first", "art/engine.wav");
        let before = contents(&source);
        let export = exported(
            &source,
            &t.0.join("exports"),
            "first",
            "art/engine.wav",
            b"new-audio",
            100,
        );
        let receipt = apply_update(&mods, &export).unwrap();
        assert_eq!(contents(&receipt.backup_path), before);
        let mut changed = contents(&source);
        for (name, data) in before {
            if name != "art/engine.wav" {
                assert_eq!(changed[&name], data);
            }
        }
        assert_eq!(
            inspect_update(&mods, &export).unwrap().status,
            UpdateStatus::AlreadyApplied
        );
        changed.insert("art/engine.wav".into(), b"external-change".to_vec());
        write_zip(&source, &changed);
        let hash = sha256_file(&source).unwrap();
        assert!(apply_update(&mods, &export).is_err());
        assert_eq!(sha256_file(&source).unwrap(), hash);
    }
    #[test]
    fn discovery_is_read_only_and_marks_old_and_conflicting_exports() {
        let t = Temp::new();
        let mods = t.0.join("mods");
        let source = original(&mods, "first", "art/engine.wav");
        let hash = sha256_file(&source).unwrap();
        let exports = t.0.join("BESS-exports");
        exported(
            &source,
            &exports.join("old"),
            "first",
            "art/engine.wav",
            b"old",
            1,
        );
        exported(
            &source,
            &exports.join("new"),
            "first",
            "art/engine.wav",
            b"new",
            2,
        );
        let found = discover(&mods, Some(&exports)).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|u| u.status == UpdateStatus::Stale));
        assert!(found.iter().any(|u| u.status == UpdateStatus::Ready));
        assert!(!mods.join(".babm_backup").exists());
        exported(
            &source,
            &exports.join("conflict"),
            "first",
            "art/engine.wav",
            b"other",
            2,
        );
        let found = discover(&mods, Some(&exports)).unwrap();
        assert_eq!(
            found
                .iter()
                .filter(|u| u.status == UpdateStatus::Conflict)
                .count(),
            2
        );
        assert_eq!(sha256_file(&source).unwrap(), hash);
    }
    #[test]
    fn shared_audio_and_missing_backups_never_mutate_pack() {
        let t = Temp::new();
        let sources = t.0.join("sources");
        let a = original(&sources, "first", "art/shared.wav");
        let b = original(&sources, "second", "art/shared.wav");
        let mods = t.0.join("mods");
        let target = pack(&mods, &[a.clone(), b]);
        let hash = sha256_file(&target).unwrap();
        let export = exported(
            &a,
            &t.0.join("exports"),
            "first",
            "art/shared.wav",
            b"new",
            4,
        );
        assert!(
            inspect_update(&mods, &export)
                .unwrap()
                .detail
                .contains("Shared sound")
        );
        assert!(apply_update(&mods, &export).is_err());
        assert_eq!(sha256_file(&target).unwrap(), hash);
        assert!(!mods.join(".babm_backup/model/bess-history").exists());
        fs::remove_file(mods.join(".babm_backup/model/first.zip")).unwrap();
        assert_eq!(
            inspect_update(&mods, &export).unwrap().status,
            UpdateStatus::Unavailable
        );
        assert_eq!(sha256_file(&target).unwrap(), hash);
    }
    #[test]
    fn malformed_marker_and_unsafe_sound_are_rejected() {
        let t = Temp::new();
        let mods = t.0.join("mods");
        let source = original(&mods, "first", "art/engine.wav");
        let export = exported(
            &source,
            &t.0.join("exports"),
            "first",
            "art/engine.wav",
            b"new",
            3,
        );
        let mut data = contents(&export);
        let mut marker: ExportMarker = serde_json::from_slice(&data[MARKER]).unwrap();
        marker.sounds[0].path = "../escape.wav".into();
        data.insert(MARKER.into(), serde_json::to_vec(&marker).unwrap());
        write_zip(&export, &data);
        assert!(read_marker(&export).is_err());
        assert!(apply_update(&mods, &export).is_err());
        assert!(!mods.join(".babm_backup").exists());
    }
    #[test]
    fn new_merge_prefers_processed_original_and_rejects_audio_collisions() {
        let t = Temp::new();
        let source = original(&t.0, "first", "art/shared.wav");
        let export = exported(
            &source,
            &t.0.join("exports"),
            "first",
            "art/shared.wav",
            b"new",
            3,
        );
        let a = crate::scanner::inspect_vehicle_zip(&source).unwrap();
        let b = crate::scanner::inspect_vehicle_zip(&export).unwrap();
        let selected = prefer_bess_variants(&[a, b.clone()]).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].file_path, b.file_path);
        let second = original(&t.0, "second", "art/shared.wav");
        let c = crate::scanner::inspect_vehicle_zip(&second).unwrap();
        assert!(check_merge_audio(&[b, c], "model").is_err());
    }
    #[test]
    fn source_list_uses_original_backups_and_rejects_grouped_isolation() {
        let t = Temp::new();
        let source = original(&t.0.join("sources"), "first", "art/engine.wav");
        let mods = t.0.join("mods");
        let target = pack(&mods, std::slice::from_ref(&source));
        let sources = sources_for_bess(&mods, &target).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(
            sha256_file(&sources[0]).unwrap(),
            sha256_file(&source).unwrap()
        );
        assert!(validate_single_source(&target).is_err());
        assert!(validate_single_source(&sources[0]).is_ok());
    }
    #[test]
    fn unmerge_preflights_all_originals_before_touching_pack() {
        let t = Temp::new();
        let source = original(&t.0.join("sources"), "first", "art/engine.wav");
        let mods = t.0.join("mods");
        let target = pack(&mods, std::slice::from_ref(&source));
        let hash = sha256_file(&target).unwrap();
        fs::remove_file(mods.join(".babm_backup/model/first.zip")).unwrap();
        assert!(crate::merger::Merger::unmerge_chassis("Model", &mods).is_err());
        assert_eq!(sha256_file(&target).unwrap(), hash);
        assert!(!mods.join("first.zip").exists());
    }

    #[test]
    fn standalone_keeps_pristine_source_for_another_export() {
        let t = Temp::new();
        let mods = t.0.join("mods");
        let source = original(&mods, "first", "art/engine.wav");
        let pristine = sha256_file(&source).unwrap();
        let first = exported(
            &source,
            &t.0.join("BESS-exports/one"),
            "first",
            "art/engine.wav",
            b"new1",
            10,
        );
        apply_update(&mods, &first).unwrap();
        let sources = sources_for_bess(&mods, &source).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sha256_file(&sources[0]).unwrap(), pristine);
        assert!(read_marker(&sources[0]).unwrap().is_none());
        let second = exported(
            &sources[0],
            &t.0.join("BESS-exports/two"),
            "first",
            "art/engine.wav",
            b"new2",
            11,
        );
        apply_update(&mods, &second).unwrap();
        assert_eq!(contents(&source)["art/engine.wav"], b"new2");
        let marker = read_marker(&source).unwrap().unwrap();
        assert_eq!(marker.source_archive_name, "first.zip");
        assert_eq!(marker.blend_path, "vehicles/first/sound.sfxBlend2D.json");
        assert_eq!(sha256_file(&sources[0]).unwrap(), pristine);
    }

    #[test]
    fn merging_processed_and_hidden_original_preserves_both_backups() {
        let t = Temp::new();
        let mods = t.0.join("mods");
        let source = original(&mods, "first", "art/engine.wav");
        let original_hash = sha256_file(&source).unwrap();
        let export = exported(&source, &mods, "first", "art/engine.wav", b"new", 10);
        let scanned = crate::scanner::scan_directory(&mods);
        assert_eq!(scanned.len(), 1);
        let merged = crate::merger::Merger::merge_variants("Model", &scanned, &mods).unwrap();
        assert!(!source.exists());
        assert!(!export.exists());
        assert_eq!(contents(&merged)["art/engine.wav"], b"new");
        let sources = sources_for_bess(&mods, &merged).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sha256_file(&sources[0]).unwrap(), original_hash);
        let manifest: MergeManifest = serde_json::from_slice(
            &fs::read(mods.join(".babm_backup/model/manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.original_files.len(), 2);
    }

    #[test]
    fn missing_other_variant_blocks_ownership_verification() {
        let t = Temp::new();
        let sources = t.0.join("sources");
        let first = original(&sources, "first", "art/shared.wav");
        let second = original(&sources, "second", "art/shared.wav");
        let mods = t.0.join("mods");
        let target = pack(&mods, &[first.clone(), second]);
        let before = sha256_file(&target).unwrap();
        fs::remove_file(mods.join(".babm_backup/model/second.zip")).unwrap();
        let export = exported(
            &first,
            &t.0.join("exports"),
            "first",
            "art/shared.wav",
            b"new",
            10,
        );
        assert!(
            inspect_update(&mods, &export)
                .unwrap()
                .detail
                .contains("source backup is missing")
        );
        assert!(apply_update(&mods, &export).is_err());
        assert_eq!(sha256_file(&target).unwrap(), before);
        assert!(!mods.join(".babm_backup/model/bess-history").exists());
    }

    #[test]
    fn renamed_single_emitter_addon_is_not_an_original_source() {
        let t = Temp::new();
        let path = t.0.join("renamed.zip");
        write_zip(
            &path,
            &BTreeMap::from([
                ("vehicles/first/info.json".into(), b"{}".to_vec()),
                ("vehicles/first/bess_engine_a.jbeam".into(), b"{}".to_vec()),
                ("vehicles/first/bess_a.pc".into(), b"{}".to_vec()),
                (
                    "art/sound/engine_BESS_a.sfxBlend2D.json".into(),
                    b"{}".to_vec(),
                ),
            ]),
        );
        assert!(validate_single_source(&path).is_err());
    }
}
