//! Runtime configuration resolved from environment variables with sensible
//! discovery-based defaults. Every external dependency can be overridden:
//!
//! - `CF_PORT`            — HTTP port (default 4571)
//! - `CF_DATA_DIR`        — project state root (default ~/.clipping-factory)
//! - `CF_OUTPUT_DIR`      — finished clip root (default ~/Downloads/Clipping Factory)
//! - `CF_FFMPEG` / `CF_FFPROBE`
//! - `CF_WHISPER_BIN`     — whisper.cpp `whisper-cli`
//! - `CF_WHISPER_MODEL`   — ggml model path
//! - `CF_FONTS_DIR`       — directory containing caption fonts
//! - `CF_FACE_MODEL`      — rustface seeta model path
//! - `CF_THREADS`         — transcription threads (default = physical cores)
//! - `CF_NO_OPEN=1`       — don't try to open the browser on start

use crate::util::which;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    pub port: u16,
    pub open_browser: bool,
    pub data_dir: PathBuf,
    pub output_root: PathBuf,
    pub ffmpeg: String,
    pub ffprobe: String,
    pub whisper_bin: Option<PathBuf>,
    pub whisper_model: Option<PathBuf>,
    pub fonts_dir: Option<PathBuf>,
    /// Bundled "try it now" episode for the first-run path.
    pub sample_episode: Option<PathBuf>,
    pub caption_font: String,
    /// Default caption style when a project doesn't specify one: "impact" | "clean".
    pub caption_style: String,
    pub face_model: Option<PathBuf>,
    pub threads: usize,
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

fn first_existing(cands: Vec<PathBuf>) -> Option<PathBuf> {
    cands.into_iter().find(|p| p.is_file())
}

fn find_whisper_model(data_dir: &Path, cwd: &Path) -> Option<PathBuf> {
    // English-only weights stay preferred: they are measurably stronger on
    // English than the multilingual equivalent at the same size. Multilingual
    // names come after so an install with only e.g. ggml-base.bin still works.
    first_existing(vec![
        data_dir.join("models/ggml-small.en.bin"),
        data_dir.join("models/ggml-base.en.bin"),
        cwd.join("models/ggml-base.en.bin"),
        cwd.join("../models/ggml-base.en.bin"),
    ])
    .or_else(|| find_multilingual_model(&model_search_dirs(data_dir, cwd)))
}

/// Multilingual ggml model names, best first (same quality ordering as the
/// `.en` list: bigger models win). whisper.cpp English-only weights end in
/// `.en.bin`; everything else covers the ~99 supported languages.
pub const MULTILINGUAL_MODELS: &[&str] = &[
    "ggml-large-v3-turbo.bin",
    "ggml-large-v3.bin",
    "ggml-medium.bin",
    "ggml-small.bin",
    "ggml-base.bin",
];

/// Directories whisper models are discovered in, in priority order.
pub fn model_search_dirs(data_dir: &Path, cwd: &Path) -> Vec<PathBuf> {
    vec![
        data_dir.join("models"),
        cwd.join("models"),
        cwd.join("../models"),
    ]
}

/// First multilingual ggml model in the given search dirs — the fallback the
/// transcribe stage switches to when a project needs more than English.
pub fn find_multilingual_model(dirs: &[PathBuf]) -> Option<PathBuf> {
    for dir in dirs {
        for name in MULTILINGUAL_MODELS {
            let path = dir.join(name);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// whisper.cpp ships English-only weights as `ggml-*.en.bin`.
pub fn model_is_multilingual(model: &Path) -> bool {
    model
        .file_name()
        .map(|name| !name.to_string_lossy().ends_with(".en.bin"))
        .unwrap_or(true)
}

impl Config {
    pub fn resolve() -> Config {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let data_dir = env_path("CF_DATA_DIR").unwrap_or_else(|| home.join(".clipping-factory"));
        let output_root = env_path("CF_OUTPUT_DIR")
            .unwrap_or_else(|| home.join("Downloads").join("Clipping Factory"));

        // Homebrew's regular FFmpeg omits libass; prefer its full build when installed.
        let homebrew_full = PathBuf::from("/opt/homebrew/opt/ffmpeg-full/bin");
        let ffmpeg = std::env::var("CF_FFMPEG").unwrap_or_else(|_| {
            let binary = homebrew_full.join("ffmpeg");
            if binary.is_file() {
                binary.to_string_lossy().into_owned()
            } else {
                "ffmpeg".into()
            }
        });
        let ffprobe = std::env::var("CF_FFPROBE").unwrap_or_else(|_| {
            let binary = homebrew_full.join("ffprobe");
            if binary.is_file() {
                binary.to_string_lossy().into_owned()
            } else {
                "ffprobe".into()
            }
        });

        // whisper.cpp binary: env → PATH → common local build locations.
        let whisper_bin = env_path("CF_WHISPER_BIN")
            .filter(|p| p.is_file())
            .or_else(|| which("whisper-cli"))
            .or_else(|| which("whisper-cpp"))
            .or_else(|| {
                first_existing(vec![
                    cwd.join("whisper.cpp/build/bin/whisper-cli"),
                    cwd.join("../whisper.cpp/build/bin/whisper-cli"),
                    PathBuf::from("/opt/homebrew/bin/whisper-cli"),
                    PathBuf::from("/usr/local/bin/whisper-cli"),
                ])
            });

        // Model: env → data dir → common local locations.
        let whisper_model = env_path("CF_WHISPER_MODEL")
            .filter(|p| p.is_file())
            .or_else(|| find_whisper_model(&data_dir, &cwd));

        // Caption fonts: bundled assets dir preferred.
        let fonts_dir = env_path("CF_FONTS_DIR").filter(|p| p.is_dir()).or_else(|| {
            let d = cwd.join("assets/fonts");
            if d.is_dir() {
                Some(d)
            } else {
                None
            }
        });
        // Bundled sample episode: env → repo assets → data dir.
        let sample_episode = env_path("CF_SAMPLE_EPISODE")
            .filter(|p| p.is_file())
            .or_else(|| {
                first_existing(vec![
                    cwd.join("assets/sample-episode.mp4"),
                    data_dir.join("sample-episode.mp4"),
                ])
            });

        let caption_font = if fonts_dir
            .as_ref()
            .and_then(|d| std::fs::read_dir(d).ok())
            .map(|entries| {
                entries.flatten().any(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .to_lowercase()
                        .starts_with("inter")
                })
            })
            .unwrap_or(false)
        {
            "Inter".to_string()
        } else {
            "DejaVu Sans".to_string()
        };

        let face_model = env_path("CF_FACE_MODEL")
            .filter(|p| p.is_file())
            .or_else(|| {
                first_existing(vec![
                    cwd.join("assets/models/seeta_fd_frontal_v1.0.bin"),
                    data_dir.join("models/seeta_fd_frontal_v1.0.bin"),
                ])
            });

        let threads = std::env::var("CF_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(4)
            });

        Config {
            port: std::env::var("CF_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(4571),
            open_browser: std::env::var("CF_NO_OPEN")
                .map(|v| v != "1")
                .unwrap_or(true),
            data_dir,
            output_root,
            ffmpeg,
            ffprobe,
            whisper_bin,
            whisper_model,
            fonts_dir,
            sample_episode,
            caption_font,
            caption_style: std::env::var("CF_CAPTION_STYLE").unwrap_or_else(|_| "impact".into()),
            face_model,
            threads,
        }
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.data_dir.join("projects")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_model_is_preferred_when_base_and_small_are_available() {
        let root = std::env::temp_dir().join(format!("cf-config-test-{}", uuid::Uuid::new_v4()));
        let models = root.join("models");
        std::fs::create_dir_all(&models).unwrap();
        let small = models.join("ggml-small.en.bin");
        let base = models.join("ggml-base.en.bin");
        std::fs::write(&base, b"base").unwrap();
        std::fs::write(&small, b"small").unwrap();

        assert_eq!(find_whisper_model(&root, &root), Some(small));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn en_only_names_detect_english_only_models() {
        assert!(!model_is_multilingual(Path::new("models/ggml-base.en.bin")));
        assert!(model_is_multilingual(Path::new("models/ggml-base.bin")));
        assert!(model_is_multilingual(Path::new("models/ggml-large-v3.bin")));
    }

    #[test]
    fn multilingual_models_fill_in_when_no_en_model_exists() {
        let root = std::env::temp_dir().join(format!("cf-config-test-{}", uuid::Uuid::new_v4()));
        let models = root.join("models");
        std::fs::create_dir_all(&models).unwrap();
        let base_multi = models.join("ggml-base.bin");
        std::fs::write(&base_multi, b"multi").unwrap();

        assert_eq!(find_whisper_model(&root, &root), Some(base_multi.clone()));
        assert_eq!(
            find_multilingual_model(&model_search_dirs(&root, &root)),
            Some(base_multi)
        );

        // An English-only model still wins discovery when both are present;
        // the transcribe stage falls back to the multilingual one on demand.
        let en = models.join("ggml-base.en.bin");
        std::fs::write(&en, b"en").unwrap();
        assert_eq!(find_whisper_model(&root, &root), Some(en));

        std::fs::remove_dir_all(root).unwrap();
    }
}
