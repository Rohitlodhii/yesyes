use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub hotkey_modifiers: u32,
    pub hotkey_vk: u32,
    /// None = Infinite (stay until explicitly closed)
    /// Some(0) = Disabled (no thumbnail shown)
    /// Some(n) = Dismiss after n seconds
    pub thumbnail_lifespan_secs: Option<u32>,
    pub save_directory: PathBuf,
}

impl Default for AppConfig {
    fn default() -> Self {
        let save_dir = std::env::var("USERPROFILE")
            .map(|p| PathBuf::from(p).join("Pictures").join("Snapdock"))
            .unwrap_or_else(|_| PathBuf::from("Snapdock"));

        Self {
            // MOD_CONTROL (2) | MOD_SHIFT (4) = 6
            hotkey_modifiers: 0x0002 | 0x0004,
            // 'S' = 0x53
            hotkey_vk: 0x53,
            // Default to Infinite (stays until closed) as requested
            thumbnail_lifespan_secs: None,
            save_directory: save_dir,
        }
    }
}

pub fn get_config_dir() -> PathBuf {
    if let Ok(app_data) = std::env::var("APPDATA") {
        PathBuf::from(app_data).join("Snapdock")
    } else if let Ok(user_profile) = std::env::var("USERPROFILE") {
        PathBuf::from(user_profile).join(".snapdock")
    } else {
        PathBuf::from(".snapdock")
    }
}

pub fn get_config_path() -> PathBuf {
    get_config_dir().join("config.json")
}

impl AppConfig {
    pub fn load() -> Self {
        let path = get_config_path();
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(cfg) = Self::from_json(&content) {
                return cfg;
            }
        }

        let default_cfg = Self::default();
        let _ = default_cfg.save();
        default_cfg
    }

    pub fn save(&self) -> io::Result<()> {
        let dir = get_config_dir();
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }
        let content = self.to_json();
        fs::write(get_config_path(), content)
    }

    pub fn to_json(&self) -> String {
        let lifespan_str = match self.thumbnail_lifespan_secs {
            Some(secs) => secs.to_string(),
            None => "null".to_string(),
        };

        let dir_str = self
            .save_directory
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");

        format!(
            "{{\n  \"hotkey_modifiers\": {},\n  \"hotkey_vk\": {},\n  \"thumbnail_lifespan_secs\": {},\n  \"save_directory\": \"{}\"\n}}\n",
            self.hotkey_modifiers, self.hotkey_vk, lifespan_str, dir_str
        )
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        let mut cfg = Self::default();

        for line in json.lines() {
            let line = line.trim();
            if line.starts_with('"') {
                if let Some((key_part, val_part)) = line.split_once(':') {
                    let key = key_part.trim().trim_matches('"');
                    let val = val_part.trim().trim_end_matches(',').trim();

                    match key {
                        "hotkey_modifiers" => {
                            if let Ok(v) = val.parse::<u32>() {
                                cfg.hotkey_modifiers = v;
                            }
                        }
                        "hotkey_vk" => {
                            if let Ok(v) = val.parse::<u32>() {
                                cfg.hotkey_vk = v;
                            }
                        }
                        "thumbnail_lifespan_secs" => {
                            if val == "null" {
                                cfg.thumbnail_lifespan_secs = None;
                            } else if let Ok(v) = val.parse::<u32>() {
                                cfg.thumbnail_lifespan_secs = Some(v);
                            }
                        }
                        "save_directory" => {
                            let s = val.trim_matches('"');
                            let unescaped = s.replace("\\\\", "\\");
                            if !unescaped.is_empty() {
                                cfg.save_directory = PathBuf::from(unescaped);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_json_roundtrip() {
        let mut cfg = AppConfig::default();
        cfg.hotkey_modifiers = 7;
        cfg.hotkey_vk = 65;
        cfg.thumbnail_lifespan_secs = Some(15);
        cfg.save_directory = PathBuf::from(r"C:\Custom\Screenshots");

        let json = cfg.to_json();
        let parsed = AppConfig::from_json(&json).expect("parse failed");
        assert_eq!(cfg, parsed);

        // Test null lifespan
        cfg.thumbnail_lifespan_secs = None;
        let json2 = cfg.to_json();
        let parsed2 = AppConfig::from_json(&json2).expect("parse failed");
        assert_eq!(cfg, parsed2);
    }
}
