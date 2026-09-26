use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineSpecs {
    pub idle_rpm: Option<f32>,
    pub max_rpm: Option<f32>,
    pub cylinders: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VehicleConfig {
    pub config_key: String,
    pub name: String,
    pub weight_kg: Option<f32>,
    pub power_hp: Option<f32>,
    pub torque_nm: Option<f32>,
    pub power_peak_rpm: Option<f32>,
    pub torque_peak_rpm: Option<f32>,
    pub top_speed_kmh: Option<f32>,
    pub accel_0_100: Option<f32>,
    pub drivetrain: Option<String>,
    pub transmission: Option<String>,
    pub fuel_type: Option<String>,
    pub induction: Option<String>,
    pub year_min: Option<u32>,
    pub year_max: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VehicleMod {
    pub file_path: PathBuf,
    pub file_name: String,
    pub file_size_bytes: u64,
    pub internal_name: String,
    pub display_name: String,
    pub author: String,
    pub is_automation: bool,
    pub default_config: Option<String>,
    pub configs: Vec<VehicleConfig>,
    pub engine: Option<EngineSpecs>,
    #[serde(skip)]
    pub thumbnail_png: Option<Vec<u8>>,
}

impl VehicleMod {
    pub fn main_config(&self) -> Option<&VehicleConfig> {
        if let Some(ref def) = self.default_config {
            if let Some(cfg) = self.configs.iter().find(|c| &c.config_key == def) {
                return Some(cfg);
            }
        }
        self.configs.first()
    }
}
