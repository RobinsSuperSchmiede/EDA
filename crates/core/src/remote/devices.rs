//! Per-device tokens: every paired device gets its own token, which can be revoked on its own.
//! Only a SHA-256 hash of each token is stored (`.openshell/data/remote-devices.json`).

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

pub const DEVICES_FILE: &str = "remote-devices.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub name: String,
    /// Hex SHA-256 of the token.
    pub token_sha256: String,
    pub paired_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<DateTime<Utc>>,
}

/// What the admin API shows (never the hash).
#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub name: String,
    pub paired_at: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
}

pub struct Devices {
    path: PathBuf,
    list: RwLock<Vec<Device>>,
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

fn valid_device_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

impl Devices {
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join(DEVICES_FILE);
        let list = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<Device>>(&t).ok())
            .unwrap_or_default();
        Self { path, list: RwLock::new(list) }
    }

    fn save(&self, list: &[Device]) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.path, serde_json::to_vec_pretty(list)?)
            .with_context(|| format!("cannot write {}", self.path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    /// Pair a device: returns its new token (shown once). Pairing an existing name replaces
    /// its token.
    pub fn pair(&self, name: &str) -> Result<String> {
        if !valid_device_name(name) {
            bail!("`{name}` is not a valid device name (letters, digits, `-`, `_`, `.`)");
        }
        let token = format!("osd_{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        let mut list = self.list.write().unwrap();
        list.retain(|d| d.name != name);
        list.push(Device {
            name: name.to_string(),
            token_sha256: sha256_hex(token.as_bytes()),
            paired_at: Utc::now(),
            last_seen: None,
        });
        self.save(&list)?;
        Ok(token)
    }

    /// Revoke a device. Returns `false` if it was not paired.
    pub fn revoke(&self, name: &str) -> Result<bool> {
        let mut list = self.list.write().unwrap();
        let before = list.len();
        list.retain(|d| d.name != name);
        if list.len() == before {
            return Ok(false);
        }
        self.save(&list)?;
        Ok(true)
    }

    pub fn list(&self) -> Vec<DeviceInfo> {
        self.list
            .read()
            .unwrap()
            .iter()
            .map(|d| DeviceInfo { name: d.name.clone(), paired_at: d.paired_at, last_seen: d.last_seen })
            .collect()
    }

    /// The device a token belongs to (updates `last_seen` in memory).
    pub fn verify(&self, token: &str) -> Option<String> {
        if token.is_empty() {
            return None;
        }
        let hash = sha256_hex(token.as_bytes());
        let mut list = self.list.write().unwrap();
        let d = list.iter_mut().find(|d| super::same_token(&d.token_sha256, &hash))?;
        d.last_seen = Some(Utc::now());
        Some(d.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_verify_revoke() {
        let dir = tempfile::tempdir().unwrap();
        let devices = Devices::load(dir.path());
        let laptop = devices.pair("laptop").unwrap();
        let phone = devices.pair("phone").unwrap();
        assert!(laptop.starts_with("osd_") && laptop.len() > 60);
        assert_eq!(devices.verify(&laptop).as_deref(), Some("laptop"));
        assert_eq!(devices.verify(&phone).as_deref(), Some("phone"));
        assert_eq!(devices.verify("osd_wrong"), None);
        assert_eq!(devices.verify(""), None);

        // stored as hashes, survives a reload
        let stored = std::fs::read_to_string(dir.path().join(DEVICES_FILE)).unwrap();
        assert!(!stored.contains(&laptop));
        let reloaded = Devices::load(dir.path());
        assert_eq!(reloaded.verify(&laptop).as_deref(), Some("laptop"));

        // re-pairing replaces the token, revoking removes the device
        let laptop2 = reloaded.pair("laptop").unwrap();
        assert_eq!(reloaded.verify(&laptop), None);
        assert_eq!(reloaded.verify(&laptop2).as_deref(), Some("laptop"));
        assert!(reloaded.revoke("phone").unwrap());
        assert!(!reloaded.revoke("phone").unwrap());
        assert_eq!(reloaded.verify(&phone), None);
        assert_eq!(reloaded.list().len(), 1);
        assert!(reloaded.pair("bad name!").is_err());
    }
}
