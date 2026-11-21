use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone)]
pub struct Credential {
    pub id: String,         // base64url credential id
    pub public_key: String, // base64url SEC1 uncompressed P-256 point
    pub counter: u32,
    pub created: u64,
    #[serde(default)]
    pub last_used: u64,
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
    /// When a passkey was last used for this session. Older sessions count as stale.
    #[serde(default)]
    pub verified: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub token_hash: String,
    pub created: u64,
    pub last_seen: u64,
    pub expires: u64,
    /// Entity ids shown as tiles on the home page, in order.
    #[serde(default)]
    pub tiles: Vec<String>,
    /// The home page's fixed panels.
    #[serde(default)]
    pub panels: Panels,
    /// Further entities for the menu pages (lights, climate, flows, music).
    #[serde(default)]
    pub more: Vec<String>,
    /// The admin's own pages, after the built-in ones in the side rail.
    #[serde(default)]
    pub pages: Vec<Page>,
    /// Rows of favourites on the home page's music card (0 to 3); unset means one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fav_rows: Option<u8>,
    /// Names this screen shows instead of the integration's, by entity id.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub names: std::collections::BTreeMap<String, String>,
}

/// A page of the admin's choosing. Items are entity ids, or `card:<name>` for
/// one of the home screen's cards (clock, climate, forecast, music).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Page {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub items: Vec<String>,
}

pub const PAGE_ICONS: [&str; 12] = ["home", "music", "bulb", "thermo", "bolt", "cloud", "sun", "moon", "chart", "radio", "flame", "plug"];
pub const PAGE_CARDS: [&str; 4] = ["card:clock", "card:climate", "card:forecast", "card:music"];

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
pub struct Panels {
    pub indoor: Option<String>,
    pub outdoor: Option<String>,
    pub wind: Option<String>,
    pub weather: Option<String>,
    pub speaker: Option<String>,
}

impl Panels {
    pub fn ids(&self) -> impl Iterator<Item = &String> {
        [&self.indoor, &self.outdoor, &self.wind, &self.weather, &self.speaker].into_iter().flatten()
    }
}

impl Device {
    /// Everything this screen may show and control: its tiles, panels and menu entities.
    pub fn allowed(&self) -> std::collections::HashSet<String> {
        let pages = self.pages.iter().flat_map(|p| p.items.iter()).filter(|i| !i.starts_with("card:"));
        self.tiles.iter().chain(self.more.iter()).chain(self.panels.ids()).chain(pages).cloned().collect()
    }
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
    #[serde(default)]
    pub settings: Settings,
}

/// Household-wide choices the admin makes on the settings page.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Hours (0–23) when screens switch to the light theme and back.
    pub day_start: u8,
    pub day_end: u8,
    /// "auto" (by the hours above), "day" or "night".
    pub theme: String,
    /// "auto" (the screen's own language), "nb" or "en".
    pub language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { day_start: 7, day_end: 21, theme: "auto".into(), language: "auto".into() }
    }
}

impl Settings {
    pub fn valid(&self) -> bool {
        self.day_start < 24 && self.day_end < 24 && ["auto", "day", "night"].contains(&self.theme.as_str()) && ["auto", "nb", "en"].contains(&self.language.as_str())
    }
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
        // Best effort: a mounted volume (Kubernetes, Docker) is often owned by root
        // and cannot be chmod-ed by us. The file itself is still created 0600.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) = fs::set_permissions(dir, fs::Permissions::from_mode(0o700)) {
                eprintln!("store: could not restrict {} to 0700 ({e}); continuing", dir.display());
            }
        }
        let file = dir.join("oldpad.json");
        let data = read(&file)?;
        Ok(Store { data, file })
    }

    /// On failure the in-memory data is put back to what is on disk, so a
    /// request that reports the error has not changed anything.
    pub fn save(&mut self) -> Result<(), &'static str> {
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
        let Err(e) = result else { return Ok(()) };
        eprintln!("store: could not save {}: {e}", self.file.display());
        let _ = fs::remove_file(&tmp);
        match read(&self.file) {
            Ok(data) => self.data = data,
            Err(e) => eprintln!("store: could not reload {} after a failed save: {e}", self.file.display()),
        }
        Err(SAVE_FAILED)
    }
}

pub const SAVE_FAILED: &str = "Could not save the change: the server cannot write its data file. Check the server log.";

fn read(file: &Path) -> std::io::Result<Data> {
    match fs::read(file) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| std::io::Error::other(format!("{}: {e}", file.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Data::default()),
        Err(e) => Err(e),
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
            panels: Panels { speaker: Some("sonos:RINCON_1".into()), ..Default::default() },
            more: vec![],
            pages: vec![],
            fav_rows: None,
            names: Default::default(),
        });
        s.save().unwrap();
        let s2 = Store::open(&dir).unwrap();
        assert_eq!(s2.data.devices[0].tiles, vec!["demo:lamp"]);
        assert!(s2.data.devices[0].allowed().contains("sonos:RINCON_1"));

        // A directory where the temp file goes makes the write fail, even as root.
        fs::create_dir(dir.join("oldpad.json.tmp")).unwrap();
        s.data.devices.clear();
        assert_eq!(s.save(), Err(SAVE_FAILED));
        assert_eq!(s.data.devices.len(), 1, "failed save puts memory back to what is on disk");
        fs::remove_dir_all(dir).unwrap();
    }
}
