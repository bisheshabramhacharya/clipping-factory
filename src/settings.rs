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
pub const PROVIDER_LOCAL: &str = "local";

pub const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";
pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-5";
/// Ollama's OpenAI-compatible endpoint; llama.cpp and LM Studio differ only by port.
pub const DEFAULT_LOCAL_BASE_URL: &str = "http://localhost:11434/v1";

/// The AI backends a user can configure. The stored setting stays a string
/// (the on-disk format and wire API are unchanged); parse it and match
/// exhaustively, so adding a provider walks you to every place that must
/// learn about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    OpenAi,
    Anthropic,
    Offline,
    Local,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            PROVIDER_OPENAI => Some(Self::OpenAi),
            PROVIDER_ANTHROPIC => Some(Self::Anthropic),
            PROVIDER_OFFLINE => Some(Self::Offline),
            PROVIDER_LOCAL => Some(Self::Local),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => PROVIDER_OPENAI,
            Self::Anthropic => PROVIDER_ANTHROPIC,
            Self::Offline => PROVIDER_OFFLINE,
            Self::Local => PROVIDER_LOCAL,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AiSettings {
    pub provider: String,
    #[serde(default)]
    pub model: String,
    /// OpenAI-compatible base URL for the `local` provider (Ollama, llama.cpp,
    /// LM Studio). Ignored by the cloud providers.
    #[serde(default)]
    pub base_url: String,
    /// Never serialized into API responses — see `public()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings {
            provider: PROVIDER_OPENAI.into(),
            model: String::new(),
            base_url: String::new(),
            api_key: None,
        }
    }
}

/// The only settings shape that ever leaves the backend.
#[derive(Serialize, Clone, Debug)]
pub struct PublicSettings {
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub connected: bool,
}

impl AiSettings {
    pub fn effective_model(&self) -> String {
        if !self.model.trim().is_empty() {
            return self.model.trim().to_string();
        }
        match Provider::parse(&self.provider) {
            Some(Provider::Anthropic) => DEFAULT_ANTHROPIC_MODEL.into(),
            // A local endpoint serves whatever model the user loaded; there is
            // no sensible default name.
            Some(Provider::Local) => String::new(),
            // Unknown stored values keep the historical default.
            _ => DEFAULT_OPENAI_MODEL.into(),
        }
    }

    pub fn effective_base_url(&self) -> String {
        let url = self.base_url.trim().trim_end_matches('/');
        if url.is_empty() {
            DEFAULT_LOCAL_BASE_URL.into()
        } else {
            url.to_string()
        }
    }

    /// The stored API key, trimmed, or None when blank.
    pub fn effective_key(&self) -> Option<&str> {
        self.api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
    }

    pub fn connected(&self) -> bool {
        match Provider::parse(&self.provider) {
            Some(Provider::Offline) => true,
            // The base URL has a default; a local endpoint only counts as
            // configured once the user names a model to run.
            Some(Provider::Local) => !self.model.trim().is_empty(),
            _ => self.effective_key().is_some(),
        }
    }

    pub fn public(&self) -> PublicSettings {
        PublicSettings {
            provider: self.provider.clone(),
            model: self.effective_model(),
            base_url: if Provider::parse(&self.provider) == Some(Provider::Local) {
                self.effective_base_url()
            } else {
                self.base_url.clone()
            },
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
        for p in [
            Provider::OpenAi,
            Provider::Anthropic,
            Provider::Offline,
            Provider::Local,
        ] {
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
            base_url: String::new(),
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

    #[test]
    fn local_provider_uses_default_base_url_and_needs_a_model() {
        let mut settings = AiSettings {
            provider: PROVIDER_LOCAL.into(),
            ..AiSettings::default()
        };
        assert_eq!(settings.effective_base_url(), DEFAULT_LOCAL_BASE_URL);
        assert!(!settings.connected());
        settings.model = " qwen2.5:7b ".into();
        assert!(settings.connected());
        assert_eq!(settings.effective_model(), "qwen2.5:7b");
        settings.base_url = "http://localhost:1234/v1/".into();
        assert_eq!(settings.effective_base_url(), "http://localhost:1234/v1");
    }
}
