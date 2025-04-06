use std::{collections::HashSet, path::Path, time::Duration};

use dirs::config_dir;
use serde::{Deserialize, Deserializer};
use smol::fs::read_to_string;

fn deserialize_seconds<'de, D: Deserializer<'de>>(de: D) -> Result<Duration, D::Error> {
    f64::deserialize(de).map(Duration::from_secs_f64)
}

fn default_timeout() -> Duration {
    Duration::from_secs(20)
}

fn default_heartbeat() -> Duration {
    Duration::from_secs(20)
}

fn default_debounce() -> Duration {
    Duration::from_secs(5)
}

#[derive(Deserialize, Debug)]
pub struct Config {
    #[serde(default)]
    pub ignored_languages: HashSet<String>,
    #[serde(deserialize_with = "deserialize_seconds", default = "default_timeout")]
    pub timeout: Duration,
    #[serde(
        deserialize_with = "deserialize_seconds",
        default = "default_heartbeat"
    )]
    pub heartbeat_frequency: Duration,
    #[serde(deserialize_with = "deserialize_seconds", default = "default_debounce")]
    pub debounce_amount: Duration,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            ignored_languages: HashSet::new(),
            timeout: default_timeout(),
            heartbeat_frequency: default_heartbeat(),
            debounce_amount: default_debounce(),
        }
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
        return Config::default();
    };

    toml::from_str(&config_file)
        .ok()
        .unwrap_or_else(Config::default)
}
