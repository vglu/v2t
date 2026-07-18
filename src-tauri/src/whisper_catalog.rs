//! ggml model catalog — URLs and SHA-1 digests from whisper.cpp `models/README.md` (HF `main`).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperModelListItem {
    pub id: String,
    pub file_name: String,
    pub size_mib: u32,
}

pub fn list_models_for_ui() -> Vec<WhisperModelListItem> {
    WHISPER_MODEL_CATALOG
        .iter()
        .map(|e| WhisperModelListItem {
            id: e.id.to_string(),
            file_name: e.file_name.to_string(),
            size_mib: e.size_mib,
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhisperModelCatalogEntry {
    /// Value stored in settings, e.g. `base`.
    pub id: &'static str,
    /// File name on disk, e.g. `ggml-base.bin`.
    pub file_name: &'static str,
    /// Approximate size for UI hints (MiB).
    pub size_mib: u32,
    pub url: &'static str,
    /// Lowercase hex SHA-1 from upstream model table (not SHA-256).
    pub sha1_hex: &'static str,
}

/// Pinned to Hugging Face `resolve/main`; if upstream replaces a file, update `sha1_hex` here.
pub const WHISPER_MODEL_CATALOG: &[WhisperModelCatalogEntry] = &[
    WhisperModelCatalogEntry {
        id: "tiny",
        file_name: "ggml-tiny.bin",
        size_mib: 75,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        sha1_hex: "bd577a113a864445d4c299885e0cb97d4ba92b5f",
    },
    WhisperModelCatalogEntry {
        id: "base",
        file_name: "ggml-base.bin",
        size_mib: 142,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        sha1_hex: "465707469ff3a37a2b9b8d8f89f2f99de7299dac",
    },
    WhisperModelCatalogEntry {
        id: "small",
        file_name: "ggml-small.bin",
        size_mib: 466,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        sha1_hex: "55356645c2b361a969dfd0ef2c5a50d530afd8d5",
    },
    // English tinydiarize model for experimental Person 1 / Person 2 labels (`-tdrz`).
    WhisperModelCatalogEntry {
        id: "small.en-tdrz",
        file_name: "ggml-small.en-tdrz.bin",
        size_mib: 466,
        url: "https://huggingface.co/akashmjn/tinydiarize-whisper.cpp/resolve/main/ggml-small.en-tdrz.bin",
        sha1_hex: "b6c6e7e89af1a35c08e6de56b66ca6a02a2fdfa1",
    },
    WhisperModelCatalogEntry {
        id: "medium",
        file_name: "ggml-medium.bin",
        size_mib: 1536,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin",
        sha1_hex: "fd9727b6e1217c2f614f9b698455c4ffd82463b4",
    },
    WhisperModelCatalogEntry {
        id: "large-v3",
        file_name: "ggml-large-v3.bin",
        size_mib: 2952,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
        sha1_hex: "ad82bf6a9043ceed055076d0fd39f5f186ff8062",
    },
    WhisperModelCatalogEntry {
        id: "large-v3-turbo",
        file_name: "ggml-large-v3-turbo.bin",
        size_mib: 1536,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        sha1_hex: "4af2b29d7ec73d781377bfd1758ca957a807e941",
    },
];

/// Silero VAD ggml used by whisper.cpp `--vad` for cleaner timed cues.
/// Not listed in the ASR model picker; downloaded alongside Whisper models.
pub const SILERO_VAD_MODEL: WhisperModelCatalogEntry = WhisperModelCatalogEntry {
    id: "silero-vad",
    file_name: "ggml-silero-v6.2.0.bin",
    size_mib: 1,
    url: "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin",
    sha1_hex: "470e5d9d094ddba2f0a512cecc3732a252188abd",
};

pub fn catalog_entry(model_id: &str) -> Option<&'static WhisperModelCatalogEntry> {
    let key = model_id.trim();
    if key.eq_ignore_ascii_case(SILERO_VAD_MODEL.id) {
        return Some(&SILERO_VAD_MODEL);
    }
    WHISPER_MODEL_CATALOG
        .iter()
        .find(|e| e.id.eq_ignore_ascii_case(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_resolves_base() {
        assert!(catalog_entry("base").is_some());
        assert!(catalog_entry("BASE").is_some());
        assert!(catalog_entry("nope").is_none());
    }

    #[test]
    fn catalog_resolves_silero_vad_companion() {
        let entry = catalog_entry("silero-vad").expect("silero-vad");
        assert_eq!(entry.file_name, "ggml-silero-v6.2.0.bin");
        assert_eq!(entry.sha1_hex, SILERO_VAD_MODEL.sha1_hex);
    }

    #[test]
    fn catalog_resolves_tinydiarize_small_en() {
        let entry = catalog_entry("small.en-tdrz").expect("small.en-tdrz");
        assert_eq!(entry.file_name, "ggml-small.en-tdrz.bin");
        assert_eq!(entry.sha1_hex, "b6c6e7e89af1a35c08e6de56b66ca6a02a2fdfa1");
        assert!(list_models_for_ui().iter().any(|m| m.id == "small.en-tdrz"));
    }

    #[test]
    fn catalog_resolves_large_v3() {
        let entry = catalog_entry("large-v3").expect("large-v3");
        assert_eq!(entry.file_name, "ggml-large-v3.bin");
        assert_eq!(entry.sha1_hex, "ad82bf6a9043ceed055076d0fd39f5f186ff8062");
        assert!(list_models_for_ui().iter().any(|m| m.id == "large-v3"));
    }
}
