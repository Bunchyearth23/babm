use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "babm", author = "Bunchy", version = "0.1.0", about = "Bunchy's Automation BeamNG Management")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Lance l'interface graphique egui (comportement par défaut)
    Gui,

    /// Scanne les véhicules exportés et affiche un résumé
    Scan {
        /// Dossier spécifique à scanner (sinon détection automatique)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Filtrer uniquement les véhicules Automation
        #[arg(short, long)]
        automation_only: bool,
    },

    /// Affiche les groupes de châssis et leurs variantes détectées
    Groups {
        /// Dossier spécifique à scanner
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Fusionne les variantes d'un châssis sous un mod BeamNG unique
    Merge {
        /// Nom du châssis (ex: "B5" ou "Volk Icarus")
        chassis: String,

        /// Dossier des mods
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Défusionne un châssis et restaure les fichiers de variantes originaux
    Unmerge {
        /// Nom du châssis à défusionner
        chassis: String,

        /// Dossier des mods
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Isole une variante pour l'utiliser dans BESS (Bunchy Engine Sound Synthesizer)
    Isolate {
        /// Nom ou fichier de la variante
        target: String,

        /// Dossier de sortie
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// Affiche les détails complets d'un véhicule (chemin de zip ou nom)
    Info {
        /// Chemin du fichier .zip ou nom du véhicule
        target: String,
    },

    /// Affiche les répertoires détectés (BeamNG, Automation)
    Paths,
}
