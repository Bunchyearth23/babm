# BESS / BABM sound exchange, version 1

## User workflow

1. In BABM, choose **Edit sound in BESS** for an individual original. Grouped vehicles use their preserved original archives. BESS also lists those originals in its detected BeamNG library, without activating or moving them.
2. In BESS, adjust the engine, save the project if you want to continue editing it, and choose **Export vehicle ZIP**. The result remains a complete vehicle ZIP, with the original structure and replacement engine WAVs.
3. Choose **Open BABM…**, or open BABM and refresh **BESS sounds**. BESS passes its selected mods folder and the parent of its last successful export when opening the companion. An export saved elsewhere is therefore discoverable without changing the saved default. BABM otherwise discovers the sibling `BESS-exports` directory and BESS's saved output preference. One level of per-export subdirectories is scanned.
4. Review the matched vehicle and select **Apply BESS sounds**. Discovery is read-only; applying is explicit. For an already grouped pack, only the original trim's associated WAVs and BABM's update receipt change. Other trims, user variables, JBeam parts, configurations and legacy add-on sounds remain intact.
5. Repeat the export/apply cycle after another edit. Applied, older, conflicting or unavailable exports have distinct states. Reopening an already processed complete ZIP in BESS retains the verified original source identity.

The original trim receives the new sound. Older selectable BESS add-on configurations remain separate and retain their own sound routing. In BeamNG, choose the original trim/engine routing to hear a full-vehicle sound update. Actual in-game audio and ergonomics remain user acceptance checks.

BABM preserves pristine sources and prior archive revisions. BESS can rediscover the pristine original after both standalone and grouped updates. Unmerging restores original individual source archives. Neither application silently installs a second full vehicle alongside its original.

## Embedded contract

Only BESS's complete mixed vehicle export writes `bess-export.json` at the ZIP root. Exhaust-only intermediate renders and historical selectable add-ons do not become full-vehicle handoffs.

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
