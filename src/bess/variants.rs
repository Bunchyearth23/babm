//! Import selectable BESS components without changing the original configuration.
use super::*;

pub const KIND: &str = "bess-variant-vehicle";
pub(super) const VARIANT_STATE: &str = "babm-bess-variants.json";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AddedFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct VariantMarker {
    pub version: u32,
    pub kind: String,
    pub source_archive_sha256: String,
    pub source_archive_name: String,
    pub vehicle_root: String,
    pub blend_path: String,
    pub exported_at_unix_ms: u64,
    pub variant_id: String,
    pub configuration_path: String,
    pub added_files: Vec<AddedFile>,
}

impl VariantMarker {
    fn source_marker(&self) -> ExportMarker {
        ExportMarker {
            version: 1,
            kind: "bess-full-vehicle".into(),
            source_archive_sha256: self.source_archive_sha256.clone(),
            source_archive_name: self.source_archive_name.clone(),
            vehicle_root: self.vehicle_root.clone(),
            blend_path: self.blend_path.clone(),
            exported_at_unix_ms: self.exported_at_unix_ms,
            sounds: Vec::new(),
        }
    }
    fn key(&self) -> String {
        format!("{}:{}", self.source_archive_sha256, self.variant_id)
    }
}

pub fn read_variant_marker(path: &Path) -> Result<Option<VariantMarker>, String> {
    let mut z = archive(path)?;
    if !z.file_names().any(|p| p == MARKER) {
        return Ok(None);
    }
    let value: serde_json::Value = read_json(&mut z, MARKER)?;
    if value["kind"] != KIND {
        return Ok(None);
    }
    let marker: VariantMarker = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let root = marker
        .vehicle_root
        .strip_prefix("vehicles/")
        .and_then(|p| p.strip_suffix('/'));
    if marker.version != 1
        || !root.is_some_and(safe_leaf)
        || !safe_leaf(&marker.variant_id)
        || !safe_leaf(&marker.source_archive_name)
        || !valid_hash(&marker.source_archive_sha256)
        || marker.exported_at_unix_ms == 0
        || !safe_member(&marker.blend_path)
        || !safe_member(&marker.configuration_path)
        || !marker.configuration_path.starts_with(&marker.vehicle_root)
        || !marker.configuration_path.ends_with(".pc")
        || marker.added_files.is_empty()
        || marker.added_files.len() > 8192
    {
        return Err("Invalid BESS selectable-variant marker".into());
    }
    for required in [
        format!("{}info.json", marker.vehicle_root),
        marker.blend_path.clone(),
    ] {
        z.by_name(&required)
            .map_err(|_| format!("Missing original vehicle member: {required}"))?;
    }
    let mut names = BTreeSet::new();
    let mut bytes = 0u64;
    for file in &marker.added_files {
        if !safe_member(&file.path)
            || !valid_hash(&file.sha256)
            || !names.insert(file.path.to_lowercase())
            || file.path == marker.blend_path
            || file.path == format!("{}info.json", marker.vehicle_root)
            || !(file.path.starts_with(&marker.vehicle_root) || file.path.starts_with("art/sound/"))
            || ![".wav", ".pc", ".json", ".jbeam", ".png", ".jpg", ".jpeg"]
                .iter()
                .any(|extension| file.path.ends_with(extension))
        {
            return Err(format!("Invalid added BESS member: {}", file.path));
        }
        let entry = z
            .by_name(&file.path)
            .map_err(|_| format!("Missing BESS member: {}", file.path))?;
        bytes = bytes.saturating_add(entry.size());
        if entry.size() > 512 * 1024 * 1024
            || bytes > 2_000_000_000
            || hash_reader(entry)? != file.sha256
        {
            return Err(format!("BESS variant checksum mismatch: {}", file.path));
        }
    }
    if !marker
        .added_files
        .iter()
        .any(|f| f.path == marker.configuration_path)
        || !marker
            .added_files
            .iter()
            .any(|f| f.path.ends_with(".jbeam"))
        || !marker.added_files.iter().any(|f| f.path.ends_with(".wav"))
    {
        return Err("BESS variant has incomplete configuration or sound routing".into());
    }
    Ok(Some(marker))
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Registry {
    version: u32,
    variants: BTreeMap<String, Revision>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Revision {
    exported_at_unix_ms: u64,
    files: BTreeMap<String, String>,
    export_files: BTreeMap<String, String>,
}

fn registry(z: &mut ZipArchive<File>) -> Result<Registry, String> {
    if !z.file_names().any(|p| p == VARIANT_STATE) {
        return Ok(Registry {
            version: 1,
            ..Default::default()
        });
    }
    let result: Registry = read_json(z, VARIANT_STATE)?;
    if result.version != 1
        || result.variants.values().any(|revision| {
            revision
                .files
                .iter()
                .any(|(path, hash)| !safe_member(path) || !valid_hash(hash))
        })
    {
        return Err("Invalid BABM selectable-variant state".into());
    }
    Ok(result)
}

pub(crate) fn variant_file_paths(path: &Path) -> Result<BTreeSet<String>, String> {
    let mut z = archive(path)?;
    let mut paths: BTreeSet<_> = registry(&mut z)?
        .variants
        .values()
        .flat_map(|r| r.files.keys().cloned())
        .collect();
    if let Some(marker) = read_variant_marker(path)? {
        paths.extend(marker.added_files.into_iter().map(|file| file.path));
    }
    Ok(paths)
}

/// A complete package can be checked against the pristine ZIP whenever BABM
/// has it. Do this before selecting the complete package in place of that ZIP.
pub(crate) fn verify_variant_originals(
    vehicles: &[crate::vehicle::VehicleMod],
    mods: &Path,
) -> Result<(), String> {
    let mut exports = Vec::new();
    for vehicle in vehicles {
        if let Some(marker) = read_variant_marker(&vehicle.file_path)? {
            // Installed packages can contain several imported profiles and
            // BABM's own receipt; their ownership was already checked on import.
            let mut z = archive(&vehicle.file_path)?;
            if !registry(&mut z)?.variants.is_empty() {
                continue;
            }
            exports.push((vehicle.file_path.clone(), marker));
        }
    }
    if exports.is_empty() {
        return Ok(());
    }
    let mut candidates: BTreeSet<PathBuf> = vehicles.iter().map(|v| v.file_path.clone()).collect();
    for (folder, manifest) in manifests(mods)? {
        for item in &manifest.original_files {
            if let Some(path) = original_backup(mods, &folder, item) {
                candidates.insert(path);
            }
        }
    }
    let mut sources = BTreeMap::new();
    for path in candidates {
        sources.entry(sha256_file(&path)?).or_insert(path);
    }
    for (path, marker) in exports {
        let Some(source) = sources.get(&marker.source_archive_sha256) else {
            continue;
        };
        let mut original = archive(source)?;
        let mut complete = archive(&path)?;
        let names: BTreeSet<_> = original.file_names().map(str::to_owned).collect();
        let folded: BTreeSet<_> = names.iter().map(|p| p.to_lowercase()).collect();
        let added: BTreeSet<_> = marker.added_files.iter().map(|f| f.path.as_str()).collect();
        if added.iter().any(|p| folded.contains(&p.to_lowercase())) {
            return Err("A BESS added member collides with its pristine original".into());
        }
        for name in &names {
            let expected = hash_reader(original.by_name(name).map_err(|e| e.to_string())?)?;
            let actual = complete
                .by_name(name)
                .map_err(|_| format!("BESS complete export removed an original member: {name}"))?;
            if hash_reader(actual)? != expected {
                return Err(format!(
                    "BESS complete export changed an original member: {name}"
                ));
            }
        }
        if complete
            .file_names()
            .any(|name| name != MARKER && !names.contains(name) && !added.contains(name))
        {
            return Err("BESS complete export contains an undeclared extra member".into());
        }
    }
    Ok(())
}

pub(crate) fn merge_variant_state(
    vehicles: &[crate::vehicle::VehicleMod],
    chassis: &str,
) -> Result<Option<Vec<u8>>, String> {
    let mut merged = Registry {
        version: 1,
        ..Default::default()
    };
    for vehicle in vehicles {
        let mut z = archive(&vehicle.file_path)?;
        let mut state = registry(&mut z)?;
        if let Some(marker) = read_variant_marker(&vehicle.file_path)? {
            state
                .variants
                .entry(marker.key())
                .or_insert_with(|| Revision {
                    exported_at_unix_ms: marker.exported_at_unix_ms,
                    files: marker
                        .added_files
                        .iter()
                        .map(|f| (f.path.clone(), f.sha256.clone()))
                        .collect(),
                    export_files: marker
                        .added_files
                        .iter()
                        .map(|f| (f.path.clone(), f.sha256.clone()))
                        .collect(),
                });
        }
        let root = format!("vehicles/{}/", vehicle.internal_name);
        let trim = vehicle
            .main_config()
            .map(|c| {
                c.name
                    .trim_end_matches(" (BESS)")
                    .trim_end_matches(" [BESS]")
                    .trim()
                    .to_owned()
            })
            .unwrap_or_else(|| {
                vehicle
                    .display_name
                    .trim_end_matches(" [BESS]")
                    .split_whitespace()
                    .last()
                    .unwrap_or("Var")
                    .to_owned()
            });
        for (key, mut revision) in state.variants {
            let mut files = BTreeMap::new();
            for (name, expected) in revision.files {
                let mut bytes = Vec::new();
                z.by_name(&name)
                    .map_err(|e| e.to_string())?
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if format!("{:x}", Sha256::digest(&bytes)) != expected {
                    return Err(format!("BESS variant changed before merging: {name}"));
                }
                let target = mapped_path(&name, &root, Some(chassis));
                if [".jbeam", ".pc", ".json"]
                    .iter()
                    .any(|ext| name.ends_with(ext))
                {
                    let mut text = String::from_utf8(bytes)
                        .map_err(|e| e.to_string())?
                        .replace(&root, &format!("vehicles/{chassis}/"));
                    if name.ends_with(".jbeam") {
                        text = crate::merger::tag_jbeam_part_names(&text, &trim, true);
                    }
                    bytes = text.into_bytes();
                }
                files.insert(target, format!("{:x}", Sha256::digest(&bytes)));
            }
            revision.files = files;
            if let Some(old) = merged.variants.get(&key) {
                if old.files != revision.files
                    || old.exported_at_unix_ms != revision.exported_at_unix_ms
                {
                    return Err(
                        "Different inputs carry conflicting revisions of one BESS variant".into(),
                    );
                }
            } else {
                merged.variants.insert(key, revision);
            }
        }
    }
    if merged.variants.is_empty() {
        Ok(None)
    } else {
        serde_json::to_vec(&merged)
            .map(Some)
            .map_err(|e| e.to_string())
    }
}

fn payload(
    marker: &VariantMarker,
    export: &mut ZipArchive<File>,
    chassis: Option<&str>,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut result = BTreeMap::new();
    for file in &marker.added_files {
        let path = mapped_path(&file.path, &marker.vehicle_root, chassis);
        let mut bytes = Vec::new();
        export
            .by_name(&file.path)
            .map_err(|e| e.to_string())?
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if let Some(chassis) = chassis
            && [".jbeam", ".pc", ".json"]
                .iter()
                .any(|ext| path.ends_with(ext))
        {
            let text =
                String::from_utf8(bytes).map_err(|_| format!("Invalid variant text: {path}"))?;
            bytes = text
                .replace(&marker.vehicle_root, &format!("vehicles/{chassis}/"))
                .into_bytes();
        }
        if result.insert(path, bytes).is_some() {
            return Err("Conflicting mapped BESS variant files".into());
        }
    }
    Ok(result)
}

fn hashes(payload: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, String> {
    payload
        .iter()
        .map(|(path, data)| (path.clone(), format!("{:x}", Sha256::digest(data))))
        .collect()
}

fn latest(mods: &Path, export: &Path, marker: &VariantMarker) -> Result<(), String> {
    let mut paths = candidate_paths(mods, None)?;
    if let Some(parent) = export.parent() {
        paths.extend(candidate_paths(mods, Some(parent))?);
        if let Some(parent) = parent.parent() {
            paths.extend(candidate_paths(mods, Some(parent))?);
        }
    }
    for path in paths {
        if path == export {
            continue;
        }
        if let Ok(Some(other)) = read_variant_marker(&path)
            && other.key() == marker.key()
        {
            if other.exported_at_unix_ms > marker.exported_at_unix_ms {
                return Err("A newer export exists for this BESS variant".into());
            }
            if other.exported_at_unix_ms == marker.exported_at_unix_ms
                && other.added_files != marker.added_files
            {
                return Err("Conflicting exports share this BESS variant and timestamp".into());
            }
        }
    }
    Ok(())
}

struct VariantPlan {
    target: Target,
    registry: Registry,
    old_files: BTreeSet<String>,
    payload: BTreeMap<String, Vec<u8>>,
    target_hash: String,
    export_hash: String,
    installed: bool,
    restored_originals: BTreeMap<String, Vec<u8>>,
    legacy_state: Option<AppliedState>,
}

type OriginalWavs = BTreeMap<String, Vec<u8>>;
type LegacyRestoration = (OriginalWavs, Option<AppliedState>);

fn restore_owned_legacy_sounds(
    mods: &Path,
    marker: &VariantMarker,
    target: &Target,
    current: &mut ZipArchive<File>,
    has_selectable: bool,
) -> Result<LegacyRestoration, String> {
    let mut legacy = state(current)?;
    let recorded = legacy
        .sources
        .get(&marker.source_archive_sha256)
        .map(|r| r.sounds.clone())
        .or_else(|| {
            if has_selectable {
                return None;
            }
            target.baseline.as_ref().map(|m| {
                m.sounds
                    .iter()
                    .map(|s| {
                        (
                            mapped_path(&s.path, &m.vehicle_root, target.chassis.as_deref()),
                            s.rendered_sha256.clone(),
                        )
                    })
                    .collect()
            })
        });
    let Some(recorded) = recorded else {
        return Ok((BTreeMap::new(), None));
    };
    let mut candidates = BTreeSet::new();
    for path in entries(mods)? {
        if is_zip(&path) {
            candidates.insert(path);
        }
    }
    for (folder, manifest) in manifests(mods)? {
        for item in &manifest.original_files {
            if let Some(path) = original_backup(mods, &folder, item) {
                candidates.insert(path);
            }
        }
    }
    let mut pristine = None;
    for path in candidates {
        if sha256_file(&path)? == marker.source_archive_sha256 {
            pristine = Some(path);
            break;
        }
    }
    let path = pristine.ok_or("The pristine Automation ZIP is required to restore the original sound before importing a selectable BESS variant")?;
    let mut original = archive(&path)?;
    let mut restored = BTreeMap::new();
    for (path, expected) in recorded {
        if !path.to_ascii_lowercase().ends_with(".wav")
            || hash_reader(
                current
                    .by_name(&path)
                    .map_err(|_| format!("Legacy BESS sound is missing: {path}"))?,
            )? != expected
        {
            return Err(format!(
                "Legacy BESS sound changed outside this workflow: {path}; original sound was not restored"
            ));
        }
        let original_path = target
            .chassis
            .as_ref()
            .and_then(|chassis| path.strip_prefix(&format!("vehicles/{chassis}/")))
            .map_or_else(
                || path.clone(),
                |rest| format!("{}{rest}", marker.vehicle_root),
            );
        let mut bytes = Vec::new();
        original
            .by_name(&original_path)
            .map_err(|_| format!("Pristine source sound is missing: {original_path}"))?
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        restored.insert(path, bytes);
    }
    legacy.sources.remove(&marker.source_archive_sha256);
    Ok((restored, Some(legacy)))
}

fn display_name(export: &Path, marker: &VariantMarker) -> String {
    let fallback = marker
        .vehicle_root
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("Vehicle");
    let Ok(mut z) = archive(export) else {
        return fallback.into();
    };
    let vehicle =
        read_json::<serde_json::Value>(&mut z, &format!("{}info.json", marker.vehicle_root))
            .ok()
            .and_then(|v| v["Name"].as_str().map(str::to_owned))
            .unwrap_or_else(|| fallback.into());
    let path = Path::new(&marker.configuration_path);
    let info = path
        .file_stem()
        .and_then(|name| name.to_str())
        .and_then(|name| path.parent().map(|p| p.join(format!("info_{name}.json"))))
        .map(|p| p.to_string_lossy().replace('\\', "/"));
    let configuration = info
        .and_then(|name| read_json::<serde_json::Value>(&mut z, &name).ok())
        .and_then(|v| v["Configuration"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "BESS".into());
    format!("{vehicle} — {configuration}")
        .chars()
        .take(200)
        .collect()
}

fn plan(mods: &Path, export_path: &Path, marker: &VariantMarker) -> Result<VariantPlan, String> {
    latest(mods, export_path, marker)?;
    let target = find_target(mods, &marker.source_marker())?;
    let mut z = archive(&target.path)?;
    let mut state = registry(&mut z)?;
    // Full selectable exports used in a first merge carry provenance in the
    // preserved input. Reconstruct ownership from exactly what that merge wrote.
    if !state.variants.contains_key(&marker.key()) {
        let mut candidates = vec![(target.path.clone(), target.chassis.as_deref())];
        let originals = manifests(mods)?;
        for (folder, manifest) in &originals {
            if mods.join(&manifest.merged_mod_file) != target.path {
                continue;
            }
            for item in &manifest.original_files {
                if let Some(path) = original_backup(mods, folder, item) {
                    candidates.push((path, target.chassis.as_deref()));
                }
            }
        }
        for (path, chassis) in candidates {
            if let Some(old) = read_variant_marker(&path)?
                && old.key() == marker.key()
            {
                let mut source = archive(&path)?;
                let content = payload(&old, &mut source, chassis)?;
                let files = hashes(&content);
                // A first merge tags part display names. Accept its exact current
                // bytes only when the original declared payload, after the same
                // deterministic merge transformation, matches.
                let mut transformed = files;
                if chassis.is_some() {
                    let vehicle = crate::scanner::inspect_vehicle_zip(&path)?;
                    let trim = vehicle
                        .main_config()
                        .map(|c| {
                            c.name
                                .trim_end_matches(" (BESS)")
                                .trim_end_matches(" [BESS]")
                                .trim()
                                .to_owned()
                        })
                        .unwrap_or_default();
                    for (name, bytes) in &content {
                        if name.ends_with(".jbeam") {
                            let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
                            let tagged = crate::merger::tag_jbeam_part_names(text, &trim, true);
                            transformed.insert(
                                name.clone(),
                                format!("{:x}", Sha256::digest(tagged.as_bytes())),
                            );
                        }
                    }
                }
                if state
                    .variants
                    .get(&marker.key())
                    .is_none_or(|previous| previous.exported_at_unix_ms < old.exported_at_unix_ms)
                {
                    state.variants.insert(
                        marker.key(),
                        Revision {
                            exported_at_unix_ms: old.exported_at_unix_ms,
                            files: transformed,
                            export_files: old
                                .added_files
                                .iter()
                                .map(|f| (f.path.clone(), f.sha256.clone()))
                                .collect(),
                        },
                    );
                }
            }
        }
    }
    let mut export = archive(export_path)?;
    let content = payload(marker, &mut export, target.chassis.as_deref())?;
    let previous = state.variants.get(&marker.key());
    if previous.is_some_and(|p| p.exported_at_unix_ms > marker.exported_at_unix_ms) {
        return Err("A newer revision of this BESS variant is already installed".into());
    }
    let old_files: BTreeSet<_> = previous
        .map(|p| p.files.keys().cloned().collect())
        .unwrap_or_default();
    if let Some(previous) = previous {
        for (name, hash) in &previous.files {
            if hash_reader(
                z.by_name(name)
                    .map_err(|_| format!("Installed BESS member is missing: {name}"))?,
            )? != *hash
            {
                return Err(format!(
                    "Installed BESS variant changed outside this workflow: {name}"
                ));
            }
        }
        let export_files: BTreeMap<_, _> = marker
            .added_files
            .iter()
            .map(|f| (f.path.clone(), f.sha256.clone()))
            .collect();
        if previous.exported_at_unix_ms == marker.exported_at_unix_ms
            && previous.export_files != export_files
        {
            return Err("Conflicting revision of this BESS variant".into());
        }
    }
    for name in content.keys() {
        if !old_files.contains(name) && z.file_names().any(|p| p.eq_ignore_ascii_case(name)) {
            return Err(format!(
                "BESS variant would overwrite an existing original or another variant: {name}"
            ));
        }
        if state
            .variants
            .iter()
            .any(|(key, revision)| key != &marker.key() && revision.files.contains_key(name))
        {
            return Err(format!("BESS file belongs to another variant: {name}"));
        }
    }
    let installed = previous.is_some_and(|p| p.exported_at_unix_ms == marker.exported_at_unix_ms);
    let has_selectable = state
        .variants
        .keys()
        .any(|key| key.starts_with(&format!("{}:", marker.source_archive_sha256)));
    let (restored_originals, legacy_state) =
        restore_owned_legacy_sounds(mods, marker, &target, &mut z, has_selectable)?;
    let installed = installed && restored_originals.is_empty();
    Ok(VariantPlan {
        target_hash: sha256_file(&target.path)?,
        export_hash: sha256_file(export_path)?,
        target,
        registry: state,
        old_files,
        payload: content,
        installed,
        restored_originals,
        legacy_state,
    })
}

pub(super) fn inspect(
    mods: &Path,
    export: &Path,
    marker: &VariantMarker,
) -> Result<BessUpdate, String> {
    let (target, status, detail) = match plan(mods, export, marker) {
        Ok(plan) if plan.installed => (
            Some(plan.target.path),
            UpdateStatus::VariantInstalled,
            "This selectable BESS variant is already installed".into(),
        ),
        Ok(plan) => {
            let detail = if plan.restored_originals.is_empty() {
                "Import the separate BESS configuration; the original configuration and sounds stay unchanged"
            } else {
                "Restore the original Automation sounds from the verified backup and add the separate BESS configuration; other variants stay unchanged"
            };
            (
                Some(plan.target.path),
                UpdateStatus::VariantReady,
                detail.into(),
            )
        }
        Err(error) => {
            let status = if error.starts_with("No installed original") {
                UpdateStatus::Unavailable
            } else if error.starts_with("A newer") {
                UpdateStatus::Stale
            } else {
                UpdateStatus::Conflict
            };
            (None, status, error)
        }
    };
    Ok(BessUpdate {
        export_path: export.to_path_buf(),
        vehicle_name: display_name(export, marker),
        target_path: target,
        exported_at_unix_ms: marker.exported_at_unix_ms,
        sounds: marker
            .added_files
            .iter()
            .filter(|f| f.path.ends_with(".wav"))
            .count(),
        selectable_variant: true,
        status,
        detail,
    })
}

pub(super) fn import(
    mods: &Path,
    export: &Path,
    marker: VariantMarker,
) -> Result<ApplyReceipt, String> {
    let mut plan = plan(mods, export, &marker)?;
    if plan.installed {
        return Err("This BESS variant is already installed".into());
    }
    let staged = unique_path(mods, ".bess-variant-stage", "tmp");
    let mut source = archive(&plan.target.path)?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged)
        .map_err(|e| e.to_string())?;
    let _guard = StagedFile(staged.clone());
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for index in 0..source.len() {
        let entry = source.by_index(index).map_err(|e| e.to_string())?;
        if entry.name() == VARIANT_STATE
            || (entry.name() == STATE && plan.legacy_state.is_some())
            || (entry.name() == MARKER && plan.target.chassis.is_none())
            || plan.old_files.contains(entry.name())
            || plan.restored_originals.contains_key(entry.name())
        {
            continue;
        }
        writer.raw_copy_file(entry).map_err(|e| e.to_string())?;
    }
    for (name, bytes) in &plan.payload {
        writer
            .start_file(name, options)
            .map_err(|e| e.to_string())?;
        writer.write_all(bytes).map_err(|e| e.to_string())?;
    }
    for (name, bytes) in &plan.restored_originals {
        writer
            .start_file(name, options)
            .map_err(|e| e.to_string())?;
        writer.write_all(bytes).map_err(|e| e.to_string())?;
    }
    if let Some(legacy) = &plan.legacy_state
        && !legacy.sources.is_empty()
    {
        writer
            .start_file(STATE, options)
            .map_err(|e| e.to_string())?;
        writer
            .write_all(&serde_json::to_vec(legacy).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    }
    plan.registry.variants.insert(
        marker.key(),
        Revision {
            exported_at_unix_ms: marker.exported_at_unix_ms,
            files: hashes(&plan.payload),
            export_files: marker
                .added_files
                .iter()
                .map(|f| (f.path.clone(), f.sha256.clone()))
                .collect(),
        },
    );
    if plan.target.chassis.is_none() {
        writer
            .start_file(MARKER, options)
            .map_err(|e| e.to_string())?;
        writer
            .write_all(&serde_json::to_vec(&marker).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    }
    writer
        .start_file(VARIANT_STATE, options)
        .map_err(|e| e.to_string())?;
    writer
        .write_all(&serde_json::to_vec(&plan.registry).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    writer
        .finish()
        .map_err(|e| e.to_string())?
        .sync_all()
        .map_err(|e| e.to_string())?;
    drop(source);
    let mut check = archive(&staged)?;
    for (name, hash) in hashes(&plan.payload) {
        if hash_reader(check.by_name(&name).map_err(|e| e.to_string())?)? != hash {
            return Err("Staged BESS variant verification failed".into());
        }
    }
    for (name, bytes) in &plan.restored_originals {
        if hash_reader(check.by_name(name).map_err(|e| e.to_string())?)?
            != format!("{:x}", Sha256::digest(bytes))
        {
            return Err("Restored Automation sound verification failed".into());
        }
    }
    drop(check);
    if sha256_file(&plan.target.path)? != plan.target_hash
        || sha256_file(export)? != plan.export_hash
    {
        return Err("Source or target changed during import; refresh and retry".into());
    }
    let folder = mods
        .join(".babm_backup")
        .join(plan.target.chassis.as_deref().unwrap_or("bess-standalone"))
        .join("bess-history");
    no_links(&folder)?;
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let backup = unique_path(&folder, "before-bess-variant", "zip");
    fs::copy(&plan.target.path, &backup).map_err(|e| e.to_string())?;
    if sha256_file(&backup)? != plan.target_hash {
        return Err("Variant backup verification failed".into());
    }
    let legacy_plan = Plan {
        update: inspect(mods, export, &marker)?,
        marker: marker.source_marker(),
        target: plan.target.clone(),
        state: AppliedState::default(),
        target_hash: plan.target_hash.clone(),
        export_hash: plan.export_hash,
        replacements: BTreeMap::new(),
    };
    preserve_standalone_original(mods, &legacy_plan)?;
    if sha256_file(&plan.target.path)? != plan.target_hash {
        return Err("Target changed before variant import".into());
    }
    replace_file(&staged, &plan.target.path)?;
    Ok(ApplyReceipt {
        target_path: plan.target.path,
        backup_path: backup,
        sounds: marker
            .added_files
            .iter()
            .filter(|f| f.path.ends_with(".wav"))
            .count(),
        vehicle_name: display_name(export, &marker),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "babm-selectable-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn write_zip(path: &Path, files: &BTreeMap<String, Vec<u8>>) {
        let mut writer = ZipWriter::new(File::create(path).unwrap());
        for (name, data) in files {
            writer
                .start_file(
                    name,
                    SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
    }
    fn contents(path: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut z = archive(path).unwrap();
        (0..z.len())
            .map(|index| {
                let mut entry = z.by_index(index).unwrap();
                let mut data = Vec::new();
                entry.read_to_end(&mut data).unwrap();
                (entry.name().to_string(), data)
            })
            .collect()
    }
    fn original(folder: &Path, root: &str) -> PathBuf {
        let mut files = BTreeMap::new();
        files.insert(
            format!("vehicles/{root}/info.json"),
            format!(r#"{{"Name":"Car {root}","Type":"Automation","default_pc":"base"}}"#)
                .into_bytes(),
        );
        files.insert(
            format!("vehicles/{root}/info_base.json"),
            br#"{"Configuration":"A"}"#.to_vec(),
        );
        files.insert(
            format!("vehicles/{root}/base.pc"),
            br#"{"parts":{"engine":"original_engine"}}"#.to_vec(),
        );
        files.insert(format!("vehicles/{root}/original.jbeam"), br#"{"original_engine":{"information":{"name":"Original engine"},"soundConfig":{"sampleName":"original"}}}"#.to_vec());
        files.insert(
            format!("art/sound/{root}.wav"),
            format!("original-{root}").into_bytes(),
        );
        files.insert(
            format!("art/sound/{root}.sfxBlend2D.json"),
            br#"{"samples":[]}"#.to_vec(),
        );
        let path = folder.join(format!("{root}.zip"));
        write_zip(&path, &files);
        path
    }
    fn export(
        source: &Path,
        folder: &Path,
        profile: &str,
        revision: u64,
        obsolete: bool,
    ) -> PathBuf {
        let root = source.file_stem().unwrap().to_str().unwrap();
        let vehicle_root = format!("vehicles/{root}/");
        let mut files = contents(source);
        let pc = format!("{vehicle_root}bess_{profile}.pc");
        let mut added = BTreeMap::new();
        added.insert(
            pc.clone(),
            format!(
                r#"{{"parts":{{"engine":"bess_{profile}"}},"path":"vehicles/{root}/texture"}}"#
            )
            .into_bytes(),
        );
        added.insert(
            format!("{vehicle_root}info_bess_{profile}.json"),
            br#"{"Configuration":"A (BESS)"}"#.to_vec(),
        );
        added.insert(format!("{vehicle_root}bess_engine_{profile}.jbeam"), format!(r#"{{"bess_{profile}":{{"information":{{"name":"BESS engine"}},"soundConfig":{{"sampleName":"BESS_{profile}"}},"path":"vehicles/{root}/texture"}}}}"#).into_bytes());
        added.insert(
            format!("art/sound/BESS_{profile}.sfxBlend2D.json"),
            format!(r#"{{"samples":[[["art/sound/BESS_{profile}.wav",800]]]}}"#).into_bytes(),
        );
        added.insert(
            format!("art/sound/BESS_{profile}.wav"),
            format!("processed-{profile}-{revision}").into_bytes(),
        );
        if obsolete {
            added.insert(
                format!("{vehicle_root}bess_{profile}.png"),
                b"old preview".to_vec(),
            );
        }
        let marker = VariantMarker {
            version: 1,
            kind: KIND.into(),
            source_archive_sha256: sha256_file(source).unwrap(),
            source_archive_name: source.file_name().unwrap().to_str().unwrap().into(),
            vehicle_root,
            blend_path: format!("art/sound/{root}.sfxBlend2D.json"),
            exported_at_unix_ms: revision,
            variant_id: profile.into(),
            configuration_path: pc,
            added_files: added
                .iter()
                .map(|(path, data)| AddedFile {
                    path: path.clone(),
                    sha256: format!("{:x}", Sha256::digest(data)),
                })
                .collect(),
        };
        files.extend(added);
        files.insert(MARKER.into(), serde_json::to_vec(&marker).unwrap());
        let path = folder.join(format!("complete-{profile}-{revision}.zip"));
        write_zip(&path, &files);
        path
    }

    fn legacy_export(source: &Path, folder: &Path) -> PathBuf {
        let root = source.file_stem().unwrap().to_str().unwrap();
        let sound = format!("art/sound/{root}.wav");
        let mut files = contents(source);
        let original_sha256 = format!("{:x}", Sha256::digest(&files[&sound]));
        files.insert(sound.clone(), b"legacy processed sound".to_vec());
        let marker = ExportMarker {
            version: 1,
            kind: "bess-full-vehicle".into(),
            source_archive_sha256: sha256_file(source).unwrap(),
            source_archive_name: format!("{root}.zip"),
            vehicle_root: format!("vehicles/{root}/"),
            blend_path: format!("art/sound/{root}.sfxBlend2D.json"),
            exported_at_unix_ms: 1,
            sounds: vec![Sound {
                path: sound.clone(),
                original_sha256,
                rendered_sha256: format!("{:x}", Sha256::digest(&files[&sound])),
            }],
        };
        files.insert(MARKER.into(), serde_json::to_vec(&marker).unwrap());
        let path = folder.join(format!("legacy-{root}.zip"));
        write_zip(&path, &files);
        path
    }

    #[test]
    fn standalone_migration_restores_verified_automation_audio_and_preserves_legacy_backup() {
        let temp = Temp::new();
        let mods = temp.0.join("mods");
        let exports = temp.0.join("exports");
        fs::create_dir(&mods).unwrap();
        fs::create_dir(&exports).unwrap();
        let source = original(&mods, "car");
        let pristine = contents(&source);
        let legacy = legacy_export(&source, &exports);
        apply_update(&mods, &legacy).unwrap();
        let before = fs::read(&source).unwrap();
        let backups = sources_for_bess(&mods, &source).unwrap();
        let selectable = export(&backups[0], &exports, "natural", 2, false);
        let view = inspect_update(&mods, &selectable).unwrap();
        assert!(view.detail.contains("Restore the original Automation"));
        assert!(view.vehicle_name.contains("Car car") && view.vehicle_name.contains("A (BESS)"));
        let receipt = apply_update(&mods, &selectable).unwrap();
        assert_eq!(fs::read(receipt.backup_path).unwrap(), before);
        let after = contents(&source);
        for (name, bytes) in &pristine {
            assert_eq!(after[name], *bytes);
        }
        assert!(!after.contains_key(STATE));
        assert_eq!(after["art/sound/BESS_natural.wav"], b"processed-natural-2");
        let revised = export(&backups[0], &exports, "natural", 3, false);
        apply_update(&mods, &revised).unwrap();
        assert_eq!(
            contents(&source)["art/sound/car.wav"],
            pristine["art/sound/car.wav"]
        );
        let input = crate::scanner::inspect_vehicle_zip(&source).unwrap();
        let pack = crate::merger::Merger::merge_variants("Car", &[input], &mods).unwrap();
        let originals = sources_for_bess(&mods, &pack).unwrap();
        assert_eq!(originals.len(), 1);
        assert_eq!(contents(&originals[0]), pristine);
    }

    #[test]
    fn grouped_migration_preserves_other_legacy_source_and_blocks_unowned_edits() {
        let temp = Temp::new();
        let source = original(&temp.0, "first");
        let other = original(&temp.0, "second");
        let source_hash = sha256_file(&source).unwrap();
        let other_hash = sha256_file(&other).unwrap();
        let exports = temp.0.join("exports");
        fs::create_dir(&exports).unwrap();
        let legacy = legacy_export(&source, &exports);
        let legacy_other = legacy_export(&other, &exports);
        let selectable = export(&source, &exports, "smooth", 2, false);
        let inputs = vec![
            crate::scanner::inspect_vehicle_zip(&source).unwrap(),
            crate::scanner::inspect_vehicle_zip(&other).unwrap(),
        ];
        let pack = crate::merger::Merger::merge_variants("Family", &inputs, &temp.0).unwrap();
        apply_update(&temp.0, &legacy).unwrap();
        apply_update(&temp.0, &legacy_other).unwrap();
        let before = contents(&pack);
        let before_bytes = fs::read(&pack).unwrap();
        let mut edited = before.clone();
        edited.insert("art/sound/first.wav".into(), b"manual sound edit".to_vec());
        write_zip(&pack, &edited);
        let edited_bytes = fs::read(&pack).unwrap();
        assert!(
            apply_update(&temp.0, &selectable)
                .unwrap_err()
                .contains("outside this workflow")
        );
        assert_eq!(fs::read(&pack).unwrap(), edited_bytes);
        fs::write(&pack, &before_bytes).unwrap();
        let receipt = apply_update(&temp.0, &selectable).unwrap();
        assert_eq!(fs::read(receipt.backup_path).unwrap(), before_bytes);
        let after = contents(&pack);
        assert_eq!(after["art/sound/first.wav"], b"original-first");
        assert_eq!(
            after["art/sound/second.wav"],
            before["art/sound/second.wav"]
        );
        for (name, bytes) in &before {
            if name != STATE && name != "art/sound/first.wav" {
                assert_eq!(after[name], *bytes, "unexpected migration change {name}");
            }
        }
        let state: AppliedState = serde_json::from_slice(&after[STATE]).unwrap();
        assert!(!state.sources.contains_key(&source_hash));
        assert!(state.sources.contains_key(&other_hash));
        assert_eq!(after["art/sound/BESS_smooth.wav"], b"processed-smooth-2");
        assert_eq!(
            inspect_update(&temp.0, &selectable).unwrap().status,
            UpdateStatus::VariantInstalled
        );
    }

    #[test]
    fn standalone_variant_import_preserves_original_and_updates_only_owned_members() {
        let temp = Temp::new();
        let mods = temp.0.join("mods");
        let exports = temp.0.join("exports");
        fs::create_dir(&mods).unwrap();
        fs::create_dir(&exports).unwrap();
        let source = original(&mods, "car");
        let before = contents(&source);
        let first = export(&source, &exports, "natural", 1, true);
        assert_eq!(
            inspect_update(&mods, &first).unwrap().status,
            UpdateStatus::VariantReady
        );
        apply_update(&mods, &first).unwrap();
        let after = contents(&source);
        for (path, data) in &before {
            assert_eq!(after[path], *data, "changed original {path}");
        }
        assert_eq!(
            inspect_update(&mods, &first).unwrap().status,
            UpdateStatus::VariantInstalled
        );
        assert!(apply_update(&mods, &first).is_err());
        let pristine = sources_for_bess(&mods, &source).unwrap();
        assert_eq!(pristine.len(), 1);
        assert_eq!(contents(&pristine[0]), before);
        let next = export(&pristine[0], &exports, "natural", 2, false);
        apply_update(&mods, &next).unwrap();
        let updated = contents(&source);
        assert!(!updated.contains_key("vehicles/car/bess_natural.png"));
        for (path, data) in &before {
            assert_eq!(updated[path], *data);
        }
        let mut edited = updated.clone();
        edited.insert("art/sound/BESS_natural.wav".into(), b"user edit".to_vec());
        write_zip(&source, &edited);
        let third = export(&pristine[0], &exports, "natural", 3, false);
        let bytes = fs::read(&source).unwrap();
        assert!(apply_update(&mods, &third).is_err());
        assert_eq!(fs::read(&source).unwrap(), bytes);
    }

    #[test]
    fn initial_merge_retains_both_configurations_and_multiple_sound_profiles() {
        let temp = Temp::new();
        let source = original(&temp.0, "car");
        let a = export(&source, &temp.0, "natural", 1, false);
        let b = export(&source, &temp.0, "raw", 2, false);
        let vehicles = crate::scanner::scan_directory(&temp.0);
        assert_eq!(vehicles.len(), 2);
        assert!(vehicles.iter().all(|v| v.configs.len() == 2));
        let groups = crate::grouper::Grouper::group_vehicles(
            &vehicles,
            &crate::paths::DetectedPaths::default(),
        );
        assert!(groups.iter().all(|g| !g.is_merged));
        let pack = crate::merger::Merger::merge_variants("Car", &vehicles, &temp.0).unwrap();
        let files = contents(&pack);
        for name in [
            "vehicles/car/base.pc",
            "vehicles/car/bess_natural.pc",
            "vehicles/car/bess_raw.pc",
            "art/sound/car.wav",
            "art/sound/BESS_natural.wav",
            "art/sound/BESS_raw.wav",
        ] {
            assert!(files.contains_key(name), "missing {name}");
        }
        assert_eq!(files["art/sound/car.wav"], b"original-car");
        assert!(
            !String::from_utf8(files["vehicles/car/original.jbeam"].clone())
                .unwrap()
                .contains("[BESS]")
        );
        assert!(files.contains_key(VARIANT_STATE));
        assert!(!source.exists() && !a.exists() && !b.exists());
        let sources = sources_for_bess(&temp.0, &pack).unwrap();
        assert_eq!(sources.len(), 1);
        let exports = temp.0.join("exports");
        fs::create_dir(&exports).unwrap();
        let next = export(&sources[0], &exports, "natural", 3, false);
        apply_update(&temp.0, &next).unwrap();
        let after = contents(&pack);
        assert_eq!(
            after["art/sound/BESS_raw.wav"],
            files["art/sound/BESS_raw.wav"]
        );
        assert_eq!(after["vehicles/car/base.pc"], files["vehicles/car/base.pc"]);
        crate::merger::Merger::unmerge_chassis("Car", &temp.0).unwrap();
        assert!(source.exists() && a.exists() && b.exists());
    }

    #[test]
    fn existing_grouped_pack_gets_disjoint_variant_and_root_remapping() {
        let temp = Temp::new();
        let source = original(&temp.0, "first");
        let other = original(&temp.0, "second");
        let exports = temp.0.join("exports");
        fs::create_dir(&exports).unwrap();
        let variant = export(&source, &exports, "smooth", 10, false);
        let inputs = vec![
            crate::scanner::inspect_vehicle_zip(&source).unwrap(),
            crate::scanner::inspect_vehicle_zip(&other).unwrap(),
        ];
        let pack = crate::merger::Merger::merge_variants("Family", &inputs, &temp.0).unwrap();
        let before = contents(&pack);
        apply_update(&temp.0, &variant).unwrap();
        let after = contents(&pack);
        for (path, data) in &before {
            assert_eq!(after[path], *data, "changed pack member {path}");
        }
        let pc = String::from_utf8(after["vehicles/family/bess_smooth.pc"].clone()).unwrap();
        assert!(pc.contains("vehicles/family/texture") && pc.contains("bess_smooth"));
        assert!(!pc.contains("vehicles/first/"));
        let original = sources_for_bess(&temp.0, &pack)
            .unwrap()
            .into_iter()
            .find(|p| p.file_name().unwrap() == "first.zip")
            .unwrap();
        let next = export(&original, &exports, "smooth", 11, false);
        apply_update(&temp.0, &next).unwrap();
        assert_eq!(
            contents(&pack)["art/sound/first.wav"],
            before["art/sound/first.wav"]
        );
    }

    #[test]
    fn declared_collision_and_corrupt_variant_are_rejected_without_mutation() {
        let temp = Temp::new();
        let source = original(&temp.0, "car");
        let exports = temp.0.join("exports");
        fs::create_dir(&exports).unwrap();
        let path = export(&source, &exports, "safe", 1, false);
        let before = fs::read(&source).unwrap();
        let mut files = contents(&path);
        let mut marker: VariantMarker = serde_json::from_slice(&files[MARKER]).unwrap();
        marker.added_files.push(AddedFile {
            path: "art/sound/car.wav".into(),
            sha256: format!("{:x}", Sha256::digest(&files["art/sound/car.wav"])),
        });
        files.insert(MARKER.into(), serde_json::to_vec(&marker).unwrap());
        write_zip(&path, &files);
        assert!(
            apply_update(&temp.0, &path)
                .unwrap_err()
                .contains("overwrite")
        );
        assert_eq!(fs::read(&source).unwrap(), before);
        marker.added_files[0].path = "vehicles/car/../bad.pc".into();
        files.insert(MARKER.into(), serde_json::to_vec(&marker).unwrap());
        write_zip(&path, &files);
        assert!(read_variant_marker(&path).is_err());
        assert_eq!(fs::read(&source).unwrap(), before);
    }

    #[test]
    fn initial_merge_refuses_a_complete_export_that_changed_its_available_original() {
        let temp = Temp::new();
        let source = original(&temp.0, "car");
        let complete = export(&source, &temp.0, "safe", 1, false);
        let original_bytes = fs::read(&source).unwrap();
        let mut files = contents(&complete);
        files.insert(
            "art/sound/car.wav".into(),
            b"unexpected replacement".to_vec(),
        );
        write_zip(&complete, &files);
        let complete_bytes = fs::read(&complete).unwrap();
        let inputs = crate::scanner::scan_directory(&temp.0);
        let error = crate::merger::Merger::merge_variants("Car", &inputs, &temp.0).unwrap_err();
        assert!(error.contains("changed an original member"));
        assert_eq!(fs::read(&source).unwrap(), original_bytes);
        assert_eq!(fs::read(&complete).unwrap(), complete_bytes);
        assert!(!temp.0.join("babm_car.zip").exists());
        assert!(!temp.0.join(".babm_backup").exists());
    }
}
