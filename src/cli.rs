use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "babm",
    author = "Bunchy",
    version = "0.1.0",
    about = "Bunchy's Automation BeamNG Management"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Launch the graphical interface (default behavior)
    Gui {
        /// BeamNG mods folder selected by BESS or by the caller
        #[arg(short, long)]
        path: Option<PathBuf>,
        /// Open the BESS sounds tab using this export folder
        #[arg(long)]
        exports: Option<PathBuf>,
    },

    /// Scan exported vehicles and display a summary table
    Scan {
        /// Specific folder to scan (defaults to auto-detected BeamNG mods folder)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Filter only Automation vehicles
        #[arg(short, long)]
        automation_only: bool,
    },

    /// Display detected chassis groups and their variants
    Groups {
        /// Specific folder to scan
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Merge variants of a chassis into a single unified BeamNG mod
    Merge {
        /// Name of the chassis (e.g. "B5" or "Volk Icarus")
        chassis: String,

        /// Mods folder
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Unmerge a chassis and restore original variant files
    Unmerge {
        /// Name of the chassis to unmerge
        chassis: String,

        /// Mods folder
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Isolate a variant into a standalone archive for BESS (Bunchy Engine Sound Synthesizer)
    Isolate {
        /// Variant name or zip filename
        target: String,

        /// Output directory
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// Display full details of a vehicle (zip path or name)
    Info {
        /// Path to .zip file or vehicle name
        target: String,
    },

    /// Display detected directories (BeamNG, Automation)
    Paths,

    /// Find BESS sound exports and preview their target without changing mods
    BessScan {
        /// BeamNG mods folder (detected automatically when omitted)
        #[arg(short, long)]
        path: Option<PathBuf>,
        /// BESS export folder (saved BESS setting and sibling BESS-exports by default)
        #[arg(long)]
        exports: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },

    /// Inspect a full-vehicle BESS ZIP without applying it
    BessInspect {
        export: PathBuf,
        #[arg(short, long)]
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },

    /// Apply the sounds of one BESS export to its original or grouped vehicle
    BessApply {
        export: PathBuf,
        #[arg(short, long)]
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}
