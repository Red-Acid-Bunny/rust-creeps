use serde::{Deserialize, Serialize};
use std::path::Path;

/// Game configuration loaded from JSON file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    /// Map tiles: each string is one row.
    /// '#' = wall, '~' = swamp, '.' = plain
    /// 'S' = spawn, 'E' = source, 'c' = creep
    pub map: Vec<String>,

    /// Simulation settings
    #[serde(default = "default_tick_delay_ms")]
    pub tick_delay_ms: u64,

    #[serde(default = "default_total_ticks")]
    pub total_ticks: u64,

    /// Lua script path (relative to project root)
    #[serde(default = "default_script_path")]
    pub script_path: String,

    /// Game parameters
    #[serde(default = "default_view_range")]
    pub view_range: i32,

    #[serde(default = "default_harvest_rate")]
    pub harvest_rate: u32,

    #[serde(default = "default_source_regen_rate")]
    pub source_regen_rate: u32,

    #[serde(default = "default_max_source_amount")]
    pub max_source_amount: u32,

    /// Initial spawn energy
    #[serde(default = "default_spawn_energy")]
    pub spawn_initial_energy: u32,

    /// Source initial resource amount
    #[serde(default = "default_source_amount")]
    pub source_initial_amount: u32,
}

fn default_tick_delay_ms() -> u64 {
    30
}
fn default_total_ticks() -> u64 {
    4500
}
fn default_script_path() -> String {
    "scripts/harvester.lua".to_string()
}
fn default_view_range() -> i32 {
    50
}
fn default_harvest_rate() -> u32 {
    10
}
fn default_source_regen_rate() -> u32 {
    1
}
fn default_max_source_amount() -> u32 {
    1000
}
fn default_spawn_energy() -> u32 {
    300
}
fn default_source_amount() -> u32 {
    1000
}

impl GameConfig {
    /// Load configuration from a JSON file.
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read config file {}: {}", path.display(), e))?;
        Self::from_str(&contents)
    }

    /// Parse configuration from a JSON string.
    pub fn from_str(s: &str) -> anyhow::Result<Self> {
        let config: GameConfig =
            serde_json::from_str(s).map_err(|e| anyhow::anyhow!("Failed to parse config JSON: {}", e))?;
        Ok(config)
    }

    /// Returns a default GameConfig with hardcoded values (used by from_map wrapper).
    pub fn with_defaults(map: Vec<String>) -> Self {
        GameConfig {
            map,
            tick_delay_ms: default_tick_delay_ms(),
            total_ticks: default_total_ticks(),
            script_path: default_script_path(),
            view_range: default_view_range(),
            harvest_rate: default_harvest_rate(),
            source_regen_rate: default_source_regen_rate(),
            max_source_amount: default_max_source_amount(),
            spawn_initial_energy: default_spawn_energy(),
            source_initial_amount: default_source_amount(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_str() {
        let map1 = String::from("###");
        let map2 = String::from("#S#");
        let map3 = String::from("###");
        let json = format!(r#"{{
            "map": ["{}", "{}", "{}"],
            "tick_delay_ms": 100,
            "total_ticks": 100,
            "view_range": 20
        }}"#, map1, map2, map3);
        let config = GameConfig::from_str(&json).unwrap();
        assert_eq!(config.map.len(), 3);
        assert_eq!(config.tick_delay_ms, 100);
        assert_eq!(config.total_ticks, 100);
        assert_eq!(config.view_range, 20);
        // Defaults should apply
        assert_eq!(config.harvest_rate, 10);
        assert_eq!(config.spawn_initial_energy, 300);
        assert_eq!(config.source_initial_amount, 1000);
    }

    #[test]
    fn test_config_defaults() {
        let json = r#"{"map": ["."]}"#;
        let config = GameConfig::from_str(json).unwrap();
        assert_eq!(config.tick_delay_ms, 30);
        assert_eq!(config.total_ticks, 4500);
        assert_eq!(config.script_path, "scripts/harvester.lua");
        assert_eq!(config.view_range, 50);
        assert_eq!(config.harvest_rate, 10);
        assert_eq!(config.source_regen_rate, 1);
        assert_eq!(config.max_source_amount, 1000);
        assert_eq!(config.spawn_initial_energy, 300);
        assert_eq!(config.source_initial_amount, 1000);
    }

    #[test]
    fn test_config_with_defaults() {
        let map = vec![r"###".to_string(), r"#S#".to_string(), r"###".to_string()];
        let config = GameConfig::with_defaults(map);
        assert_eq!(config.map.len(), 3);
        assert_eq!(config.tick_delay_ms, 30);
        assert_eq!(config.view_range, 50);
    }
}
