use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;

const DEFAULT_CONFIG_PATH: &str = "/opt/etc/reGenda/config.yml";

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    pub timezone: Option<String>,
    pub language: Option<String>,
    pub sources: HashMap<String, ServerConfig>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ServerConfig {
    #[serde(default = "default_type")]
    pub r#type: String,
    pub url: String,
    pub user: String,
    pub password: String,
}

fn default_type() -> String {
    "server".to_string()
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = std::env::var("REGENDA_CONFIG")
            .unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());

        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file at {}", path))?;

        let config: Config = serde_yaml::from_str(&contents)
            .with_context(|| "Failed to parse config YAML")?;

        Ok(config)
    }

    pub fn timezone_str(&self) -> &str {
        self.timezone.as_deref().unwrap_or("UTC")
    }

    pub fn language_str(&self) -> &str {
        self.language.as_deref().unwrap_or("en")
    }
}
