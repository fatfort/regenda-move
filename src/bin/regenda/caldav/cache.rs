use super::types::{CalendarInfo, Event};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const CACHE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CacheFile {
    pub version: u32,
    pub fetched_at: DateTime<Utc>,
    pub calendars: Vec<CalendarInfo>,
    pub events: Vec<Event>,
}

/// Resolve the cache path. Precedence:
///   1. explicit `path` (from config)
///   2. `REGENDA_CACHE` env var
///   3. `$HOME/.config/reGenda/cache.json`
pub fn resolve_path(config_path: Option<&str>) -> PathBuf {
    if let Some(p) = config_path {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var("REGENDA_CACHE") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/root".to_string());
    PathBuf::from(home).join(".config/reGenda/cache.json")
}

pub fn load(path: &Path) -> Option<CacheFile> {
    let data = std::fs::read_to_string(path).ok()?;
    let parsed: CacheFile = match serde_json::from_str(&data) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Cache at {:?} failed to parse: {}", path, e);
            return None;
        }
    };
    if parsed.version != CACHE_VERSION {
        log::warn!(
            "Cache at {:?} has unexpected version {} (expected {}); ignoring",
            path,
            parsed.version,
            CACHE_VERSION
        );
        return None;
    }
    log::info!(
        "Cache loaded: {} events, {} calendars, fetched_at={}",
        parsed.events.len(),
        parsed.calendars.len(),
        parsed.fetched_at
    );
    Some(parsed)
}

pub fn save(path: &Path, calendars: &[CalendarInfo], events: &[Event]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let cache = CacheFile {
        version: CACHE_VERSION,
        fetched_at: Utc::now(),
        calendars: calendars.to_vec(),
        events: events.to_vec(),
    };
    let data = serde_json::to_string(&cache).context("serialize cache")?;
    std::fs::write(path, data).with_context(|| format!("write cache to {:?}", path))?;
    log::info!(
        "Cache saved: {} events, {} calendars -> {:?}",
        events.len(),
        calendars.len(),
        path
    );
    Ok(())
}
