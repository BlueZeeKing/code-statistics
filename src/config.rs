use std::{collections::HashSet, path::Path};

use dirs::config_dir;
use serde::Deserialize;
use smol::fs::read_to_string;

#[derive(Deserialize)]
pub struct Config {
    pub ignored_filetype: HashSet<String>,
    pub timeout: f64,
    pub heartbeat_frequency: f64,
}

fn default_config() -> Config {
    Config {
        ignored_filetype: HashSet::new(),
        timeout: 60.0,
        heartbeat_frequency: 60.0,
    }
}

pub async fn read_config() -> Config {
    let Ok(config_file) = read_to_string(
        config_dir()
            .expect("Failed to get config dir")
            .join(Path::new("code-statistics/config.toml")),
    )
    .await
    else {
        return default_config();
    };

    toml::from_str(&config_file)
        .ok()
        .unwrap_or_else(default_config)
}
