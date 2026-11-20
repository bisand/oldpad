use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone)]
pub struct Credential {
    pub id: String,         // base64url credential id
    pub public_key: String, // base64url SEC1 uncompressed P-256 point
    pub counter: u32,
    pub created: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Admin {
    pub id: String,
    pub name: String,
    pub user_handle: String,
    pub credentials: Vec<Credential>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Session {
    pub hash: String,
    pub admin_id: String,
    pub expires: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub token_hash: String,
    pub created: u64,
    pub last_seen: u64,
    pub expires: u64,
    /// Entity ids shown as tiles, in order. A device may only control these.
    #[serde(default)]
    pub tiles: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct IntegrationConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    pub config: serde_json::Map<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Data {
    #[serde(default)]
    pub admins: Vec<Admin>,
    #[serde(default)]
    pub sessions: Vec<Session>,
    #[serde(default)]
    pub devices: Vec<Device>,
    #[serde(default)]
    pub integrations: Vec<IntegrationConfig>,
}

/// One JSON file, rewritten atomically (temp file + rename) on every change.
/// shortcut: whole-file writes are fine for a household; move to an append log if it grows past a few hundred KB.
pub struct Store {
    pub data: Data,
    file: PathBuf,
}

impl Store {
    pub fn open(dir: &Path) -> std::io::Result<Store> {
        fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
        let file = dir.join("oldpad.json");
        let data = match fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| std::io::Error::other(format!("{}: {e}", file.display())))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Data::default(),
            Err(e) => return Err(e),
        };
        Ok(Store { data, file })
    }

    pub fn save(&self) {
        let tmp = self.file.with_extension("json.tmp");
        let result = (|| {
            let bytes = serde_json::to_vec_pretty(&self.data).map_err(std::io::Error::other)?;
            let mut opts = fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
            let mut f = opts.open(&tmp)?;
            std::io::Write::write_all(&mut f, &bytes)?;
            f.sync_all()?;
            fs::rename(&tmp, &self.file)
        })();
        if let Err(e) = result {
            // Losing a write is bad but not fatal: the in-memory state stays correct until restart.
            eprintln!("store: could not save {}: {e}", self.file.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_reopen() {
        let dir = std::env::temp_dir().join(format!("oldpad-store-{}", crate::util::random_code(8)));
        let mut s = Store::open(&dir).unwrap();
        s.data.devices.push(Device {
            id: "d1".into(),
            name: "Kitchen".into(),
            token_hash: "h".into(),
            created: 1,
            last_seen: 1,
            expires: 2,
            tiles: vec!["demo:lamp".into()],
        });
        s.save();
        let s2 = Store::open(&dir).unwrap();
        assert_eq!(s2.data.devices[0].tiles, vec!["demo:lamp"]);
        fs::remove_dir_all(dir).unwrap();
    }
}
