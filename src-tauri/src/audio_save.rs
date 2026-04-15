//! Save the audio extracted during transcription to the user's output folder.
//!
//! Two code paths:
//! - **URL** — the first yt-dlp pass (`-x`) already produced an audio file in the temp
//!   work dir. When the user picked `mp3` or `m4a`, yt-dlp converted in place via
//!   `--audio-format` (see `pipeline::prepare_media_audio`); otherwise the container
//!   is whatever bestaudio gave us (`m4a` / `opus` / `webm` / …). We just copy.
//! - **Local video** — we invoke ffmpeg ourselves. `Original` uses `-c:a copy` plus
//!   `ffprobe` to pick a matching container; `mp3` / `m4a` re-encode.
//!
//! Errors here are **non-fatal** — the caller logs them and continues transcription.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::pipeline::{run_cmd, tail_stderr};
use crate::settings::DownloadedAudioFormat;

const FFPROBE_TIMEOUT: Duration = Duration::from_secs(30);
const FFMPEG_AUDIO_TIMEOUT: Duration = Duration::from_secs(600);

/// Copy a file verbatim, creating the destination directory if needed.
pub fn copy_downloaded_audio(src: &Path, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create audio dir: {e}"))?;
    }
    fs::copy(src, dest).map_err(|e| format!("copy audio: {e}"))?;
    Ok(())
}

/// Container extension for a given audio codec returned by ffprobe. `None` means
/// "no clean stream-copy target — re-encode instead".
fn container_ext_for_codec(codec: &str) -> Option<&'static str> {
    match codec.trim().to_lowercase().as_str() {
        "aac" => Some("m4a"),
        "mp3" => Some("mp3"),
        "opus" => Some("opus"),
        "vorbis" => Some("ogg"),
        "flac" => Some("flac"),
        "pcm_s16le" | "pcm_s24le" | "pcm_s32le" | "pcm_f32le" => Some("wav"),
        _ => None,
    }
}

/// Ask ffprobe for the first audio stream's codec name.
async fn probe_audio_codec(
    ffprobe: &Path,
    input: &Path,
    cancel: &CancellationToken,
) -> Result<String, String> {
    let args: Vec<String> = vec![
        "-v".into(),
        "error".into(),
        "-select_streams".into(),
        "a:0".into(),
        "-show_entries".into(),
        "stream=codec_name".into(),
        "-of".into(),
        "default=nw=1:nk=1".into(),
        input.to_string_lossy().into_owned(),
    ];
    let out = run_cmd(ffprobe, &args, FFPROBE_TIMEOUT, cancel).await?;
    if !out.status.success() {
        return Err(format!(
            "ffprobe failed (exit {}): {}",
            out.status.code().unwrap_or(-1),
            tail_stderr(&out.stderr)
        ));
    }
    let codec = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if codec.is_empty() {
        return Err("ffprobe returned empty codec name (no audio track?)".to_string());
    }
    Ok(codec)
}

/// Extract the audio track from a local video file into `dest_dir`, building the
/// filename from `make_name(ext)`. Returns the path actually written (with the ext
/// chosen by codec / user).
pub async fn extract_audio_from_local_video(
    ffmpeg: &Path,
    ffprobe: Option<&Path>,
    input: &Path,
    dest_dir: &Path,
    format: DownloadedAudioFormat,
    make_name: impl Fn(&str) -> String,
    cancel: &CancellationToken,
) -> Result<PathBuf, String> {
    fs::create_dir_all(dest_dir).map_err(|e| format!("create audio dir: {e}"))?;

    // Ext + codec args tuple: (target_ext, extra ffmpeg args after `-vn`).
    let (ext, codec_args): (String, Vec<String>) = match format {
        DownloadedAudioFormat::Mp3 => (
            "mp3".to_string(),
            vec![
                "-c:a".into(),
                "libmp3lame".into(),
                "-q:a".into(),
                "2".into(),
            ],
        ),
        DownloadedAudioFormat::M4a => (
            "m4a".to_string(),
            vec!["-c:a".into(), "aac".into(), "-q:a".into(), "2".into()],
        ),
        DownloadedAudioFormat::Original => {
            let codec = match ffprobe {
                Some(ffp) => probe_audio_codec(ffp, input, cancel).await.ok(),
                None => None,
            };
            match codec.as_deref().and_then(container_ext_for_codec) {
                Some(e) => (e.to_string(), vec!["-c:a".into(), "copy".into()]),
                // Unknown codec or no ffprobe: fall back to m4a AAC re-encode.
                None => (
                    "m4a".to_string(),
                    vec!["-c:a".into(), "aac".into(), "-q:a".into(), "2".into()],
                ),
            }
        }
    };

    let dest = dest_dir.join(make_name(&ext));

    let mut args: Vec<String> = vec![
        "-y".into(),
        "-i".into(),
        input.to_string_lossy().into_owned(),
        "-vn".into(),
    ];
    args.extend(codec_args);
    args.push(dest.to_string_lossy().into_owned());

    let out = run_cmd(ffmpeg, &args, FFMPEG_AUDIO_TIMEOUT, cancel).await?;
    if !out.status.success() {
        return Err(format!(
            "ffmpeg audio extract failed (exit {}): {}",
            out.status.code().unwrap_or(-1),
            tail_stderr(&out.stderr)
        ));
    }
    if !dest.is_file() {
        return Err(format!("ffmpeg did not create {}", dest.display()));
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_ext_known_codecs() {
        assert_eq!(container_ext_for_codec("aac"), Some("m4a"));
        assert_eq!(container_ext_for_codec("AAC"), Some("m4a"));
        assert_eq!(container_ext_for_codec(" mp3 "), Some("mp3"));
        assert_eq!(container_ext_for_codec("opus"), Some("opus"));
        assert_eq!(container_ext_for_codec("vorbis"), Some("ogg"));
        assert_eq!(container_ext_for_codec("flac"), Some("flac"));
        assert_eq!(container_ext_for_codec("pcm_s16le"), Some("wav"));
    }

    #[test]
    fn container_ext_unknown_codec_none() {
        assert_eq!(container_ext_for_codec(""), None);
        assert_eq!(container_ext_for_codec("dts"), None);
        assert_eq!(container_ext_for_codec("ac3"), None);
    }

    #[test]
    fn copy_creates_parent_dirs() {
        let tmp = std::env::temp_dir().join("v2t-audio-save-test");
        let _ = std::fs::remove_dir_all(&tmp);
        let src = tmp.join("src.bin");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(&src, b"hello").unwrap();
        let dest = tmp.join("nested").join("out.m4a");
        copy_downloaded_audio(&src, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
