# BESS / BABM integration — 2026-10-03

## Delivered workflow

The complete Automation vehicle ZIP remains BESS's primary export. It embeds a
versioned source identity and the original/rendered WAV hashes. BABM discovers
that export, matches its preserved original and explicitly applies the sound to
the individual vehicle or the existing grouped pack. Regrouped variables, other
trims, JBeam parts and configuration files are preserved.

Both applications expose a companion action and carry the selected folders
between them. BESS lists pristine originals from BABM backup manifests, including
after a standalone sound update. The last successful one-off export folder is
used when opening BABM without changing the saved default.

Original archives and prior target revisions are retained. Revision receipts
prevent rollback to old sound exports and detect conflicting target edits. ZIP
updates are staged, verified, serialized with merge/unmerge and atomically
replaced. Matching uses archive and WAV hashes, rather than display names.

Older selectable BESS add-ons keep their separate engine/sound routing. These
updates affect the original trim's routing; select that trim/engine in BeamNG to
hear the new full-vehicle sound. The [shared contract](../BESS-BABM-EXCHANGE.md)
documents the workflow and command-line interface.

## Validation

Both projects pass strict release Clippy across all targets and formatting checks.
BESS passes **69 targeted release tests**: 32 import/path/library tests, one
marker-integrity test, two complete-export tests, one archive-preservation
regression and 33 application tests. BABM passes its **18 release tests**, including
12 exchange/merge/restore tests. The Python acceptance and packaging scripts
compile successfully.

The compiled acceptance script exercises standalone and grouped vehicles:

- Discovery and inspection leave target bytes unchanged.
- Applying replaces exactly the declared WAVs and preserves non-audio members.
- Repeated application is safe; a prior revision cannot roll back a newer one.
- BESS rediscovers the pristine source in BABM backups.
- BESS reopens a processed standalone ZIP, exports another sound revision and
  retains its original source identity; BABM accepts that revision.
- Unmerge restores the original source archive with its exact SHA-256.

A disposable copy of the installed B5 pack and its complete backup set provides
the real-vehicle check: **60 WAVs replaced, 651 other entries unchanged**, including
regrouped variables and legacy add-on sounds. BESS rediscovers all three original
vehicles. The eight installed pack/backup/manifest files retain their hashes.

Local evidence is stored under `output/babm-interop-20261003/` in the BESS checkout.
The initial compiled roundtrip is in `roundtrip/verification.json`, with the B5
result in `real-copy/verification.json`. Final-executable repetitions and delivery
hashes are recorded separately below. Private vehicle archives and generated
sound files are excluded from Git.

The final binaries repeat both compiled scenarios successfully in
`roundtrip-final/verification.json` and the real B5 copy in
`real-copy-final/verification.json`. Native BESS capture visibly shows the custom
mods folder and its three preserved originals. Both Desktop executables open
responsive native windows with explicit test folders; `desktop-startup.json`
records their identities. This is startup evidence, not an automated mouse-click
acceptance of the complete GUI workflow.

An initial Rust compiler access violation was resolved by cleaning only the
local BESS release package cache and rebuilding. No compiler flags or acceptance
thresholds were weakened; subsequent strict checks and release tests passed.

## Release executable delivery

| Application | Desktop executable | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| BESS 0.11.1 | `D:\Desktop\BESS.exe` | 9,770,496 | `ff7e4f5d7cf3a14fb1c5e614837b396089eb013df03a7962a9d26d1bf402cc36` |
| BABM 0.1.0 | `D:\Desktop\BABM.exe` | 7,326,208 | `556a59e47d04f8c76a7d58831cfe3c97c036bc8049dae6404f10ce6eb1e6a47d` |

The Desktop copies match each repository's `target/release` executable and the
archived final pair in `output/babm-interop-20261003/release-final/`. The executables
sit together so companion discovery finds the corresponding application.
`desktop-delivery.json` records the byte-for-byte copy verification.

Publication is directly to each repository's `main`, as requested. The change is
based on BESS `1225689d4476b732b3576ef5fe082cd9773cd2a0` and BABM
`3f4eca2d02fd9eedf4a7abd0ab46b8b3a8637091`; final publication receipts record local
and remote commit identities after pushing. This delivery does not create a new
tagged/public release or package the user's private vehicles.

## Scope of acceptance

These checks establish package preservation, source identity, restoration and
application startup. They do not establish subjective audio quality, actual
BeamNG playback or user acceptance of the interface. No installed mod is updated
by this validation; real-vehicle checks use a disposable copy of the B5 pack and
its backup set. Existing listening and game acceptance gates remain open.
