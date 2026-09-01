//! AI provider settings. The API key is stored outside the project directory
//! with user-only permissions, is never logged, and is never returned to the
//! UI (PRD §7.1, §13, §16.3).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const PROVIDER_OPENAI: &str = "openai";
pub const PROVIDER_ANTHROPIC: &str = "anthropic";
pub const PROVIDER_OFFLINE: &str = "offline";

pub const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";
pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-5";

/// The AI backends a user can configure. The stored setting stays a string
/// (the on-disk format and wire API are unchanged); parse it and match
/// exhaustively, so adding a provider walks you to every place that must
/// learn about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    OpenAi,
    Anthropic,
    Offline,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            PROVIDER_OPENAI => Some(Self::OpenAi),
            PROVIDER_ANTHROPIC => Some(Self::Anthropic),
            PROVIDER_OFFLINE => Some(Self::Offline),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => PROVIDER_OPENAI,
            Self::Anthropic => PROVIDER_ANTHROPIC,
            Self::Offline => PROVIDER_OFFLINE,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AiSettings {
    pub provider: String,
    #[serde(default)]
    pub model: String,
    /// Never serialized into API responses — see `public()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings {
            provider: PROVIDER_OPENAI.into(),
            model: String::new(),
            api_key: None,
        }
    }
}

/// The only settings shape that ever leaves the backend.
#[derive(Serialize, Clone, Debug)]
pub struct PublicSettings {
    pub provider: String,
    pub model: String,
    pub connected: bool,
}

impl AiSettings {
    pub fn effective_model(&self) -> String {
        if !self.model.trim().is_empty() {
            return self.model.trim().to_string();
        }
        match Provider::parse(&self.provider) {
            Some(Provider::Anthropic) => DEFAULT_ANTHROPIC_MODEL.into(),
            // Unknown stored values keep the historical default.
            _ => DEFAULT_OPENAI_MODEL.into(),
        }
    }

    pub fn connected(&self) -> bool {
        self.provider == PROVIDER_OFFLINE
            || self
                .api_key
                .as_deref()
                .map(|k| !k.trim().is_empty())
                .unwrap_or(false)
    }

    pub fn public(&self) -> PublicSettings {
        PublicSettings {
            provider: self.provider.clone(),
            model: self.effective_model(),
            connected: self.connected(),
        }
    }
}

fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

pub fn load(data_dir: &Path) -> AiSettings {
    let path = settings_path(data_dir);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => AiSettings::default(),
    }
}

pub fn save(data_dir: &Path, settings: &AiSettings) -> Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let path = settings_path(data_dir);
    let temp = crate::util::unique_temp_path(&path);
    let json = serde_json::to_vec_pretty(settings)?;

    let result = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        // The temporary contains the API key too, so it must never briefly be
        // created with process-default permissions.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temp)
            .with_context(|| format!("creating {}", temp.display()))?;
        file.write_all(&json)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, &path).with_context(|| format!("replacing {}", path.display()))?;
        #[cfg(unix)]
        std::fs::File::open(data_dir)?.sync_all()?;
        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_parse_round_trips_through_as_str() {
        for p in [Provider::OpenAi, Provider::Anthropic, Provider::Offline] {
            assert_eq!(Provider::parse(p.as_str()), Some(p));
        }
    }

    /// Parsing stays exact-match: mixed case was never accepted by the API.
    #[test]
    fn provider_parse_rejects_unknown_names() {
        assert_eq!(Provider::parse("gemini"), None);
        assert_eq!(Provider::parse(""), None);
        assert_eq!(Provider::parse("OpenAI"), None);
    }

    #[test]
    fn settings_save_round_trips() {
        let dir = std::env::temp_dir().join(format!("cf-settings-{}", crate::util::short_id()));
        let settings = AiSettings {
            provider: PROVIDER_ANTHROPIC.into(),
            model: "test-model".into(),
            api_key: Some("secret".into()),
        };
        save(&dir, &settings).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.provider, settings.provider);
        assert_eq!(loaded.model, settings.model);
        assert_eq!(loaded.api_key, settings.api_key);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(settings_path(&dir))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(dir).ok();
    }
}
