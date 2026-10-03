# BESS / BABM selectable variants and legacy sound updates

## User workflow

1. In BABM, choose **Edit sound in BESS** for an individual original. Grouped vehicles use their preserved original archives. BESS also lists those originals in its detected BeamNG library, without activating or moving them.
2. In BESS, adjust the engine and export the complete vehicle ZIP. It contains the unchanged original vehicle and a separate selectable BESS configuration, with its own engine part, blends and WAVs.
3. Choose **Open BABM…**, or open BABM and refresh **BESS sounds**. BESS passes its selected mods folder and the parent of its last successful export when opening the companion. An export saved elsewhere is therefore discoverable without changing the saved default. BABM otherwise discovers the sibling `BESS-exports` directory and BESS's saved output preference. One level of per-export subdirectories is scanned.
4. Review the matched vehicle and select **Import BESS variant**. BABM adds the declared BESS configuration and components to the installed vehicle or grouped pack, remapping vehicle paths when necessary. Existing configuration files, original sounds, other trims and user variables remain unchanged.
5. Repeat after another edit. A stable `variant_id` identifies a sound profile. BABM verifies its installed files, replaces its owned files and removes obsolete files from that profile only. Different profiles remain separate configurations. Unexpected edits and collisions block the operation.

For a first merge, the complete original-plus-BESS package supersedes its duplicate pristine ZIP while preserving both configurations. Different BESS profiles are retained. Merely containing several configurations does not make a complete BESS export an already merged BABM pack. BESS does not perform BABM's chassis grouping.

Historical `bess-full-vehicle` exports remain supported under **Apply BESS sounds**: those replace the original trim's declared WAVs. They do not create a configuration. This legacy behavior is separate from selectable variant imports. In-game selection and audio remain user acceptance checks.

If legacy sounds were already applied, importing the first selectable variant restores that source's original Automation WAVs from its verified pristine backup, then adds the separate BESS components in the same atomic update. Restoration is allowed only while the installed WAV hashes still equal the legacy receipt. Missing pristine backups or external sound edits block import. Other sources' legacy receipts and sounds are retained; the complete pre-migration archive is backed up.

BABM preserves pristine sources and prior archive revisions. BESS can rediscover the pristine original after both standalone and grouped updates. Unmerging restores original individual source archives. Neither application silently installs a second full vehicle alongside its original.

## Selectable complete-vehicle contract

The ZIP root contains `bess-export.json` with `version: 1`, `kind: "bess-variant-vehicle"`, `source_archive_sha256`, `source_archive_name`, `vehicle_root`, `blend_path` (the original blend), `exported_at_unix_ms`, a stable `variant_id`, `configuration_path`, and `added_files: [{ "path": "...", "sha256": "..." }]`.

All original members remain present. Added files use disjoint names and include the configuration, engine parts, sound blends, sounds and optional preview image. BABM validates the complete added payload. The `babm-bess-variants.json` registry records each profile's original export hashes and the hashes of its installed, potentially remapped files. A revision can replace or remove only that profile's recorded members. The registry is written in the same staged ZIP as the configuration and sounds, and it survives a subsequent BABM merge.

Before a first merge, when the pristine ZIP is available among the inputs or preserved backups, BABM compares every original member with the complete export and rejects missing, changed or undeclared files. Without that source ZIP, the added payload is verified but the original contents cannot be independently compared with the source archive hash. Importing into an existing vehicle always retains its actual original members instead of copying those from the export.

Imports use the same per-mods lock, verified archive backups and atomic replacement as legacy sound updates. Discovery is read-only. An unavailable original/pack remains unavailable; the self-contained export can be used as an input to a new merge. Existing packs are extended by BABM without rebuilding their original JBeam or user settings.

## Legacy replacement contract

Historical complete mixed replacement exports use the following marker. Exhaust-only intermediate renders and historical unmarked add-ons are not replacement handoffs.

```json
{
  "version": 1,
  "kind": "bess-full-vehicle",
  "source_archive_sha256": "64 lowercase hexadecimal characters",
  "source_archive_name": "original.zip",
  "vehicle_root": "vehicles/original_vehicle/",
  "blend_path": "art/sound/blends/engine.sfxBlend2D.json",
  "exported_at_unix_ms": 1791057600000,
  "sounds": [
    {
      "path": "art/sound/engine/sample.wav",
      "original_sha256": "SHA-256 of the pristine source WAV bytes",
      "rendered_sha256": "SHA-256 of the exported WAV bytes"
    }
  ]
}
```

The original archive hash binds an export to a concrete preserved source, independently of its filename or current location. Sound hashes cover complete WAV bytes. The selected vehicle root and blend identify the source layout. An inherited marker is accepted by BESS only when its version, vehicle, blend, complete sound set and current rendered hashes agree with the actual source archive. Every new export retains original hashes and gets a later export timestamp.

BABM validates the marker and actual payload, matches the preserved source, derives grouped vehicle-root remapping from its backup manifest, and verifies sound ownership. Shared ambiguous sound paths, incomplete backup sets, unsupported versions, malformed paths, unexpected edits and conflicting revisions are refused rather than guessed. ZIP/path/metadata limits apply. Links and Windows reparse points cannot redirect backup or update writes.

Applying creates and verifies a staged archive, records the previous archive in history, and replaces the target atomically. The namespaced `babm-bess-updates.json` receipt travels inside the target archive so that audio and applied revision stay together. A per-mods write lock serializes application, merge and unmerge. Preserved source manifests list direct backup filenames; BESS does not trust arbitrary original absolute paths from those manifests.

## Command-line verification

```powershell
BABM.exe bess-scan --path C:\Test\mods --exports C:\Test\BESS-exports --json
BABM.exe bess-inspect C:\Test\BESS-exports\run\vehicle.zip --path C:\Test\mods --json
BABM.exe bess-apply C:\Test\BESS-exports\run\vehicle.zip --path C:\Test\mods --json
BABM.exe gui --path C:\Test\mods --exports C:\Test\BESS-exports
BESS.exe --open C:\Test\original.zip --beamng-mods C:\Test\mods --bess-exports C:\Test\BESS-exports
```

Companion folder arguments apply to the current session. Explicit user folder
choices remain separate from these launch overrides; opening a companion does
not silently replace the saved default folders.

The BESS repository's `tools/verify_babm_exchange.py` runs both compiled executables on disposable fixture directories. It checks standalone and grouped application, unchanged non-audio content, original rediscovery, repeated/stale application, BESS reopening BABM's output, and restoration. Its fixture inputs come from `tests/beamng_full_export.rs` when `BESS_BABM_TEST_ARTIFACTS` names a new directory. Runtime logs and executable hashes belong in the delivery report; the protocol alone is not proof of a successful game test.
