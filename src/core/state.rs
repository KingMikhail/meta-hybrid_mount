use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use crate::defs;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct RuntimeState {
    pub timestamp: u64,
    pub pid: u32,
    pub storage_mode: String,
    pub mount_point: PathBuf,
    pub overlay_modules: Vec<String>,
    pub magic_modules: Vec<String>,
    pub nuke_active: bool,
    #[serde(default)]
    pub active_mounts: Vec<String>,
}

impl RuntimeState {
    pub fn new(
        storage_mode: String,
        mount_point: PathBuf,
        overlay_modules: Vec<String>,
        magic_modules: Vec<String>,
        nuke_active: bool,
    ) -> Self {
        let start = SystemTime::now();
        let timestamp = start.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let pid = std::process::id();
        let active_mounts = Self::detect_active_mounts(&mount_point);

        Self {
            timestamp,
            pid,
            storage_mode,
            mount_point,
            overlay_modules,
            magic_modules,
            nuke_active,
            active_mounts,
        }
    }

    fn detect_active_mounts(base_path: &Path) -> Vec<String> {
        let mut actives = Vec::new();
        let base_str = base_path.to_string_lossy();

        if let Ok(content) = fs::read_to_string("/proc/mounts") {
            for line in content.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    let target = parts[1];
                    let fstype = parts[2];
                    let options = parts[3];

                    if fstype == "overlay" && options.contains(&*base_str) {
                        let name = Path::new(target)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                        if !actives.contains(&name) {
                            actives.push(name);
                        }
                    }
                }
            }
        }
        actives
    }

    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(defs::STATE_FILE, json)?;
        Ok(())
    }

    pub fn load() -> Result<Self> {
        if !std::path::Path::new(defs::STATE_FILE).exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(defs::STATE_FILE)?;
        let state = serde_json::from_str(&content)?;
        Ok(state)
    }
}
