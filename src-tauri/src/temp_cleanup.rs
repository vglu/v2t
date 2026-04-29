//! Sweep orphaned `v2t-work-*` directories from the system temp dir.
//!
//! `pipeline::prepare_media_audio` creates `temp_dir().join("v2t-work-{nanos}")`
//! per call; cleanup happens only on certain success/error paths. Hard kills,
//! panics, and `delete_audio_after = false` jobs leave the directory behind.
//! This module scans on app startup and removes anything older than `max_age`.

use std::path::Path;
use std::time::{Duration, SystemTime};

const WORK_DIR_PREFIX: &str = "v2t-work-";

#[derive(Debug, Default, Clone)]
pub struct CleanupReport {
    pub removed: u32,
    pub bytes_freed: u64,
    pub errors: u32,
}

/// Scan the system temp dir and remove `v2t-work-*` directories older than `max_age`.
pub fn run_cleanup(max_age: Duration) -> CleanupReport {
    run_cleanup_at(max_age, SystemTime::now(), &std::env::temp_dir())
}

/// Testable core: cleanup against an explicit `now` and `root`.
pub(crate) fn run_cleanup_at(max_age: Duration, now: SystemTime, root: &Path) -> CleanupReport {
    let mut report = CleanupReport::default();
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return report,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(WORK_DIR_PREFIX) {
            continue;
        }
        let modified = match entry.metadata().and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let age = match now.duration_since(modified) {
            Ok(d) => d,
            Err(_) => continue, // mtime in the future; skip
        };
        if age < max_age {
            continue;
        }
        let size = dir_size(&path);
        match std::fs::remove_dir_all(&path) {
            Ok(()) => {
                report.removed = report.removed.saturating_add(1);
                report.bytes_freed = report.bytes_freed.saturating_add(size);
            }
            Err(_) => report.errors = report.errors.saturating_add(1),
        }
    }
    report
}

fn dir_size(p: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(rd) = std::fs::read_dir(p) else {
        return 0;
    };
    for e in rd.flatten() {
        let path = e.path();
        if path.is_dir() {
            total = total.saturating_add(dir_size(&path));
        } else if let Ok(md) = e.metadata() {
            total = total.saturating_add(md.len());
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fresh_dir() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!("v2t-cleanup-test-{nanos}"));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn removes_old_work_dirs_only() {
        let root = fresh_dir();
        let work_a = root.join("v2t-work-aaa");
        let work_b = root.join("v2t-work-bbb");
        let other = root.join("not-ours-ccc");
        let work_file = root.join("v2t-work-not-a-dir.txt");
        fs::create_dir_all(&work_a).unwrap();
        fs::create_dir_all(&work_b).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::write(work_a.join("payload.bin"), vec![0u8; 4096]).unwrap();
        fs::write(work_b.join("payload.bin"), vec![0u8; 8192]).unwrap();
        fs::write(other.join("payload.bin"), vec![0u8; 1024]).unwrap();
        fs::write(&work_file, b"unrelated").unwrap();

        // Force "now" 48h in the future → all dirs appear ancient.
        let future = SystemTime::now() + Duration::from_secs(48 * 3600);
        let report = run_cleanup_at(Duration::from_secs(24 * 3600), future, &root);

        assert_eq!(report.removed, 2);
        assert!(report.bytes_freed >= 4096 + 8192);
        assert_eq!(report.errors, 0);
        assert!(!work_a.exists());
        assert!(!work_b.exists());
        assert!(other.exists(), "non-prefixed dirs should be untouched");
        assert!(work_file.exists(), "regular files should be untouched");

        // Cleanup
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn keeps_fresh_work_dirs() {
        let root = fresh_dir();
        let work = root.join("v2t-work-fresh");
        fs::create_dir_all(&work).unwrap();
        fs::write(work.join("data"), vec![0u8; 100]).unwrap();

        // "Now" is right now → freshly-created dir is younger than max_age.
        let report = run_cleanup_at(Duration::from_secs(24 * 3600), SystemTime::now(), &root);

        assert_eq!(report.removed, 0);
        assert!(work.exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_root_returns_empty_report() {
        let nonexistent = std::env::temp_dir().join("v2t-cleanup-does-not-exist-xyz");
        let report = run_cleanup_at(
            Duration::from_secs(60),
            SystemTime::now(),
            &nonexistent,
        );
        assert_eq!(report.removed, 0);
        assert_eq!(report.errors, 0);
    }
}
