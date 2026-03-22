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
    WhisperModelCatalogEntry {
        id: "medium",
        file_name: "ggml-medium.bin",
        size_mib: 1536,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin",
        sha1_hex: "fd9727b6e1217c2f614f9b698455c4ffd82463b4",
    },
    WhisperModelCatalogEntry {
        id: "large-v3-turbo",
        file_name: "ggml-large-v3-turbo.bin",
        size_mib: 1536,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        sha1_hex: "4af2b29d7ec73d781377bfd1758ca957a807e941",
    },
];

pub fn catalog_entry(model_id: &str) -> Option<&'static WhisperModelCatalogEntry> {
    let key = model_id.trim();
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
}
