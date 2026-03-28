//! Append-only session log under `app_data_dir/logs/` with rotation (keep newest files).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use chrono::Local;
use tauri::AppHandle;
use tauri::Manager;

const KEEP_NEWEST_LOG_FILES: usize = 5;

pub struct SessionLog {
    path: PathBuf,
    file: Mutex<std::fs::File>,
}

impl SessionLog {
    pub fn try_init(app: &AppHandle) -> Option<Self> {
        let dir = app.path().app_data_dir().ok()?.join("logs");
        fs::create_dir_all(&dir).ok()?;
        let _ = prune_to_keep_newest(&dir, KEEP_NEWEST_LOG_FILES.saturating_sub(1));

        let name = format!("v2t-{}.log", Local::now().format("%Y-%m-%d-%H%M%S"));
        let path = dir.join(name);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        Some(Self {
            path,
            file: Mutex::new(file),
        })
    }

    pub fn log_path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, job_id: Option<&str>, phase: &str, message: &str) {
        let now = Local::now().format("%H:%M:%S");
        let jid = job_id.unwrap_or("-");
        let line = format!("[{now}] [{jid}] {phase}: {message}\n");
        if let Ok(mut g) = self.file.lock() {
            let _ = g.write_all(line.as_bytes());
            let _ = g.flush();
        }
    }
}

pub fn try_append(app: &AppHandle, job_id: Option<&str>, phase: &str, message: &str) {
    if let Some(log) = app.try_state::<SessionLog>() {
        log.append(job_id, phase, message);
    }
}

fn prune_to_keep_newest(dir: &Path, keep: usize) -> Result<(), std::io::Error> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("log"))
        })
        .collect();
    if files.len() <= keep {
        return Ok(());
    }
    files.sort_by_key(|p| {
        fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    });
    files.reverse();
    for p in files.into_iter().skip(keep) {
        let _ = fs::remove_file(p);
    }
    Ok(())
}
