# BABM — Bunchy's Automation BeamNG Management

[![Release](https://img.shields.io/github/v/release/Bunchyearth23/babm)](https://github.com/Bunchyearth23/babm/releases)
[![Build & Release](https://github.com/Bunchyearth23/babm/actions/workflows/release.yml/badge.svg)](https://github.com/Bunchyearth23/babm/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)

**BABM** (Bunchy's Automation BeamNG Management) is a high-performance management and mod unification utility designed to bridge **Automation: The Car Company Tycoon Game** and **BeamNG.drive**.

---

## 📌 Problem & Solution

When you design multiple trims, variants, and engine configurations of a car model in Automation, exporting them creates an individual mod ZIP for each trim. In BeamNG's vehicle selector, this results in dozens of separate vehicle entries cluttering the menu instead of a single car model with multiple configurations.

**BABM solves this cleanly:**
- It automatically groups related trims under their parent **chassis model**.
- It merges separate variant ZIP files into a single unified BeamNG vehicle mod.
- All trims become accessible as **configurations (`.pc`)** under one single car banner in BeamNG.
- It tags JBeam part names in the vehicle parts configurator with their variant name (e.g. `[V6 Turbo]`) and `[BESS]` status, making it effortless to identify which parts belong to which trim.
- **100% Non-Destructive**: Original variant archives are safely preserved in `.babm_backup/` and can be unmerged / restored at any time in one click.

---

## ✨ Features

- **Chassis Model Grouping**: Identifies chassis families using Automation's SQLite database (`Sandbox_*.db`) or smart heuristic name matching.
- **Part Selector Disambiguation**: Adds `[{variant}]` and `[BESS]` tags to part names inside JBeam definitions, so parts from different trims are clearly separated in BeamNG's parts selector.
- **Non-Destructive Merge & Unmerge**: Safely moves and stores original variant archives. Unmerging completely restores all original files and removes the unified mod.
- **BESS Compatibility**: Fully compatible with [BESS (Bunchy's Engine Sound Synthesis)](https://github.com/Bunchyearth23/bess). Includes a one-click variant isolation tool to prepare any trim for standalone BESS sound synthesis.
- **Accurate Engine & JBeam Parsing**: Correctly computes real engine redline RPM using `revLimiterRPM` instead of fallback overrev limits, and extracts cylinders, idle RPM, power, torque, and weight.
- **Multithreaded GUI**: Asynchronous background operations ensure the UI remains smooth and responsive during merging, unmerging, and file scanning.
- **Dual Mode (GUI & CLI)**: Run as an interactive desktop GUI powered by `egui`/`eframe`, or automate via a fast command-line interface.

---

## 🚀 Getting Started

### Download
Download the latest pre-compiled binary for Windows from the [Releases](https://github.com/Bunchyearth23/babm/releases) page (`BABM.exe`).

### Graphical User Interface (GUI)
Simply launch `BABM.exe`. BABM will automatically scan your BeamNG user and mods folders, as well as Automation's export directories.

- **Chassis & Variants Tab**: View detected chassis families, inspect trims, and click **Merge variants** or **Unmerge / Restore**.
- **All Vehicles Tab**: Browse all installed vehicles with detailed engine specs, power curves, weight, and configuration lists.
- **Paths & Diagnostics Tab**: Verify detected directories or configure custom mod search paths.

---

## 💻 Command Line Interface (CLI)

BABM includes a comprehensive CLI for headless usage and scripts:

```bash
# Launch the Graphical User Interface
babm gui

# Display detected BeamNG and Automation directories
babm paths

# Scan mods directory and list all vehicles
babm scan

# Scan only Automation vehicle exports
babm scan --automation-only

# List detected chassis groups and their variants
babm groups

# Merge variants of a chassis into a single mod
babm merge "ChassisName"

# Unmerge a previously merged chassis and restore original files
babm unmerge "ChassisName"

# Isolate a variant for BESS sound synthesis
babm isolate "VariantName" -o ./bess_export

# Inspect detailed specs and JBeam data of a vehicle ZIP
babm info "vehicles/my_car.zip"
```

---

## 🛠️ Building from Source

### Prerequisites
- [Rust](https://www.rust-lang.org/) (2024 edition / latest stable)
- `cargo` package manager

### Build & Run
```bash
# Clone repository
git clone https://github.com/Bunchyearth23/babm.git
cd babm

# Run GUI in debug mode
cargo run

# Build optimized release executable
cargo build --release
```

The compiled binary will be located at `target/release/babm.exe`.

---

## 📄 License

This project is licensed under the MIT License or Apache-2.0.
