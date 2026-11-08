use super::remap_path_bytes;
use crate::{bess, vehicle::VehicleMod};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use zip::ZipArchive;

pub(crate) const STATE: &str = "babm-merge-remaps.json";

#[derive(Clone, Default, Deserialize, Serialize)]
pub(crate) struct MergeRemap {
    ids: BTreeMap<String, String>,
    names: BTreeMap<String, String>,
    paths: BTreeMap<String, String>,
}

impl MergeRemap {
    pub(crate) fn has_rules(&self) -> bool {
        !self.ids.is_empty() || !self.names.is_empty() || !self.paths.is_empty()
    }

    pub(crate) fn path(&self, name: &str, chassis: &str) -> String {
        let name = self.asset(name);
        match name
            .strip_prefix("vehicles/")
            .and_then(|n| n.split_once('/'))
        {
            Some((_, rest)) => format!("vehicles/{chassis}/{rest}"),
            None => name,
        }
    }

    fn asset(&self, name: &str) -> String {
        self.paths
            .get(name)
            .cloned()
            .unwrap_or_else(|| self.identifiers(name))
    }

    fn identifiers(&self, name: &str) -> String {
        let mut result = String::with_capacity(name.len());
        for word in name.split_inclusive(|c: char| !c.is_ascii_alphanumeric()) {
            let id = word.trim_end_matches(|c: char| !c.is_ascii_alphanumeric());
            result.push_str(self.ids.get(id).map_or(id, String::as_str));
            result.push_str(&word[id.len()..]);
        }
        result
    }

    fn reference(&self, name: &str, root: &str) -> String {
        if let Some(mapped) = self.names.get(name) {
            return mapped.clone();
        }
        let unrooted = name.trim_start_matches('/');
        let mapped = if let Some(mapped) = self.paths.get(unrooted) {
            mapped.clone()
        } else if let Some(rest) = unrooted.strip_prefix(root) {
            format!("{root}{}", self.identifiers(rest))
        } else {
            self.identifiers(unrooted)
        };
        format!("{}{mapped}", &name[..name.len() - unrooted.len()])
    }

    pub(crate) fn text(&self, bytes: &[u8], root: &str, chassis: &str) -> Vec<u8> {
        let new_root = format!("vehicles/{chassis}/");
        if !self.has_rules() {
            return remap_path_bytes(bytes, root, &new_root);
        }
        // Apply asset rewrites before the vehicle root changes, so absolute
        // references and relative names can both use the original path map.
        let mut mapped = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if matches!(bytes[i], b'/' | b'<' | b'-')
                && let Some(end) = comment_end(&bytes[i..])
            {
                mapped.extend_from_slice(&bytes[i..i + end]);
                i += end;
                continue;
            }
            let quote = bytes[i];
            if !matches!(quote, b'"' | b'\'') {
                mapped.push(quote);
                i += 1;
                continue;
            }
            mapped.push(quote);
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    2
                } else {
                    1
                };
            }
            let name = &bytes[start..i];
            match std::str::from_utf8(name) {
                Ok(name) => mapped.extend_from_slice(self.reference(name, root).as_bytes()),
                Err(_) => mapped.extend_from_slice(name),
            }
            if i < bytes.len() {
                mapped.push(quote);
                i += 1;
            }
        }
        remap_path_bytes(&mapped, root, &new_root)
    }

    fn rename_path(&mut self, old: &str, new: String) {
        let basename = |path: &str| path.rsplit('/').next().unwrap_or("").to_owned();
        self.names.insert(basename(old), basename(&new));
        self.paths.insert(old.to_owned(), new);
    }

    fn isolate_audio(&mut self, path: &str, files: &BTreeSet<String>, suffix: &str) {
        let bank = path
            .strip_prefix("art/sound/engine/")
            .and_then(|rest| rest.split_once('/').map(|(bank, _)| bank))
            .or_else(|| blend_id(path.strip_prefix("art/sound/blends/")?));
        if let Some(bank) = bank {
            let new_bank = format!("{bank}_{suffix}");
            let prefix = format!("art/sound/engine/{bank}/");
            for name in files {
                if let Some(rest) = name.strip_prefix(&prefix) {
                    self.paths
                        .insert(name.clone(), format!("art/sound/engine/{new_bank}/{rest}"));
                }
            }
            for name in files {
                if name.starts_with("art/sound/blends/") && blend_id(name) == Some(bank) {
                    self.rename_blend(name, bank, suffix);
                }
            }
            self.names.insert(bank.to_owned(), new_bank);
        } else if let Some(id) = blend_id(path) {
            self.rename_blend(path, id, suffix);
        } else {
            self.rename_path(path, renamed_file(path, suffix));
        }
    }

    fn rename_blend(&mut self, path: &str, id: &str, suffix: &str) {
        let new_id = format!("{id}_{suffix}");
        let at = path.rfind('/').map_or(0, |index| index + 1);
        let (parent, name) = path.split_at(at);
        self.rename_path(path, format!("{parent}{new_id}{}", &name[id.len()..]));
        self.names.insert(id.to_owned(), new_id);
    }
}

fn blend_id(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next()?;
    let end = name.len().checked_sub(".sfxBlend2D.json".len())?;
    let (id, extension) = name.split_at_checked(end)?;
    extension
        .eq_ignore_ascii_case(".sfxBlend2D.json")
        .then_some(id)
}

fn comment_end(bytes: &[u8]) -> Option<usize> {
    for (open, close) in [
        (&b"/*"[..], &b"*/"[..]),
        (&b"<!--"[..], &b"-->"[..]),
        (&b"--[["[..], &b"]]"[..]),
    ] {
        if let Some(rest) = bytes.strip_prefix(open) {
            return Some(
                rest.windows(close.len())
                    .position(|w| w == close)
                    .map_or(bytes.len(), |i| open.len() + i + close.len()),
            );
        }
    }
    if bytes.starts_with(b"//") || bytes.starts_with(b"--") {
        return Some(
            bytes
                .iter()
                .position(|byte| *byte == b'\n')
                .unwrap_or(bytes.len()),
        );
    }
    None
}

fn renamed_file(path: &str, suffix: &str) -> String {
    let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
    let (stem, extension) = name.split_once('.').unwrap_or((name, ""));
    let parent = if parent.is_empty() {
        String::new()
    } else {
        format!("{parent}/")
    };
    let extension = if extension.is_empty() {
        String::new()
    } else {
        format!(".{extension}")
    };
    format!("{parent}{stem}_{suffix}{extension}")
}

fn is_audio(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".wav") || name.ends_with(".sfxblend2d.json")
}

type MergePlans = (Vec<MergeRemap>, BTreeMap<String, MergeRemap>);

pub(super) fn plan(variants: &[VehicleMod], chassis: &str) -> Result<MergePlans, String> {
    let mut occupied = BTreeMap::<String, Option<String>>::new();
    let mut audio = BTreeMap::new();
    let mut output_audio = BTreeMap::new();
    let mut remaps = Vec::new();
    let mut by_source = BTreeMap::<String, MergeRemap>::new();
    for vehicle in variants {
        let mut archive = bess::archive(&vehicle.file_path)?;
        let mut files = BTreeSet::new();
        let mut sounds = BTreeMap::new();
        for i in 0..archive.len() {
            let entry = archive.by_index(i).map_err(|e| e.to_string())?;
            if !entry.is_dir() {
                let name = entry.name().to_owned();
                if is_audio(&name) {
                    sounds.insert(name.clone(), bess::hash_reader(entry)?);
                }
                files.insert(name);
            }
        }
        let mut remap = MergeRemap::default();
        let conflicts: Vec<_> = sounds
            .iter()
            .filter(|(name, hash)| {
                audio
                    .get(&remap.path(name, chassis).to_ascii_lowercase())
                    .is_some_and(|old| old != *hash)
            })
            .map(|(name, _)| name)
            .collect();
        let mut owner = None;
        if !conflicts.is_empty() {
            let source = if let Some(marker) = bess::read_variant_marker(&vehicle.file_path)? {
                marker.source_archive_sha256
            } else if let Some(marker) = bess::read_marker(&vehicle.file_path)? {
                marker.source_archive_sha256
            } else {
                bess::sha256_file(&vehicle.file_path)?
            };
            let suffix = format!("babm_{}_{}", chassis, &source[..12]);
            for name in conflicts {
                if !remap.paths.contains_key(name) {
                    remap.isolate_audio(name, &files, &suffix);
                }
            }
            // A shared blend can have identical bytes in both sources while
            // pointing at samples that need separate paths.
            let root = format!("vehicles/{}/", vehicle.internal_name);
            for name in sounds.keys() {
                if let Some(id) = blend_id(name)
                    && !remap.paths.contains_key(name)
                {
                    let mut bytes = Vec::new();
                    archive
                        .by_name(name)
                        .map_err(|e| e.to_string())?
                        .read_to_end(&mut bytes)
                        .map_err(|e| e.to_string())?;
                    if remap.text(&bytes, &root, &vehicle.internal_name) != bytes {
                        remap.rename_blend(name, id, &suffix);
                    }
                }
            }
            isolate_vehicle(
                &mut remap,
                &mut archive,
                &files,
                |target| {
                    occupied
                        .get(target)
                        .is_some_and(|existing| existing.as_deref() != Some(&source))
                },
                chassis,
                &suffix,
            )?;
            let combined = by_source.entry(source.clone()).or_default();
            combined.ids.extend(remap.ids.clone());
            combined.names.extend(remap.names.clone());
            combined.paths.extend(remap.paths.clone());
            owner = Some(source);
        }
        for (name, hash) in sounds {
            audio
                .entry(
                    MergeRemap::default()
                        .path(&name, chassis)
                        .to_ascii_lowercase(),
                )
                .or_insert_with(|| hash.clone());
            let path = remap.path(&name, chassis).to_ascii_lowercase();
            if output_audio.get(&path).is_some_and(|old| old != &hash) {
                return Err(format!("Audio paths still collide after remapping: {path}"));
            }
            output_audio.insert(path, hash);
        }
        for name in files {
            occupied
                .entry(remap.path(&name, chassis))
                .or_insert_with(|| owner.clone());
        }
        remaps.push(remap);
    }
    Ok((remaps, by_source))
}

fn isolate_vehicle(
    remap: &mut MergeRemap,
    archive: &mut ZipArchive<std::fs::File>,
    files: &BTreeSet<String>,
    collides: impl Fn(&str) -> bool,
    chassis: &str,
    suffix: &str,
) -> Result<(), String> {
    // Automation embeds the five-character trim identifier in filenames,
    // mesh and material names, and references between them.
    for path in files.iter().filter(|name| name.starts_with("vehicles/")) {
        let target = remap.path(path, chassis);
        if collides(&target)
            && let Some(stem) = path.rsplit('/').next().and_then(|n| n.strip_suffix(".pc"))
            && stem.len() == 5
            && stem.bytes().all(|b| b.is_ascii_hexdigit())
        {
            remap
                .ids
                .insert(stem.to_owned(), format!("{stem}_{suffix}"));
        }
    }
    for path in files.iter().filter(|name| name.starts_with("vehicles/")) {
        if path.ends_with("/camso_core.jbeam") || path.ends_with("/info.json") {
            continue;
        }
        if collides(&remap.path(path, chassis)) {
            let mapped = renamed_file(&remap.asset(path), suffix);
            remap.rename_path(path, mapped);
        }
        if path.ends_with(".jbeam") {
            let mut text = String::new();
            archive
                .by_name(path)
                .map_err(|e| e.to_string())?
                .read_to_string(&mut text)
                .map_err(|e| e.to_string())?;
            let parts = super::jbeam::parse(text.trim_start_matches('\u{feff}'))
                .map_err(|e| format!("Cannot separate parts in {path}: {e}"))?;
            if let Some(parts) = parts.as_object() {
                for name in parts.keys() {
                    let mapped = remap.identifiers(name);
                    let mapped = if mapped == *name {
                        format!("{name}_{suffix}")
                    } else {
                        mapped
                    };
                    remap.names.insert(name.clone(), mapped);
                }
            }
        }
    }
    Ok(())
}
