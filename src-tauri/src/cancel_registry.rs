use std::collections::HashMap;
use std::sync::Mutex;

use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub struct JobCancelRegistry {
    inner: Mutex<HashMap<String, CancellationToken>>,
}

impl JobCancelRegistry {
    pub fn register_job(&self, job_id: &str) -> CancellationToken {
        let t = CancellationToken::new();
        self.inner
            .lock()
            .unwrap()
            .insert(job_id.to_string(), t.clone());
        t
    }

    pub fn finish_job(&self, job_id: &str) {
        self.inner.lock().unwrap().remove(job_id);
    }

    /// Idempotent: returns true if a token existed for this job.
    pub fn cancel_job(&self, job_id: &str) -> bool {
        let g = self.inner.lock().unwrap();
        if let Some(t) = g.get(job_id) {
            t.cancel();
            true
        } else {
            false
        }
    }

    /// Cancel every active job (e.g. app exit).
    pub fn cancel_all(&self) {
        let g = self.inner.lock().unwrap();
        for t in g.values() {
            t.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_marks_token() {
        let reg = JobCancelRegistry::default();
        let t = reg.register_job("j1");
        assert!(reg.cancel_job("j1"));
        assert!(t.is_cancelled());
    }

    #[test]
    fn cancel_all_marks_all() {
        let reg = JobCancelRegistry::default();
        let a = reg.register_job("a");
        let b = reg.register_job("b");
        reg.cancel_all();
        assert!(a.is_cancelled());
        assert!(b.is_cancelled());
    }
}
