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

    /// Affiche les détails complets d'un véhicule (chemin de zip ou nom)
    Info {
        /// Chemin du fichier .zip ou nom du véhicule
        target: String,
    },

    /// Affiche les répertoires détectés (BeamNG, Automation)
    Paths,
}
