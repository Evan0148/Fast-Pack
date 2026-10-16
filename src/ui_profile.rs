use std::path::PathBuf;
use std::time::Instant;

pub(crate) struct Profile {
    path: PathBuf,
    previous: Option<Instant>,
    frames: Vec<f64>,
    count: usize,
}

impl Profile {
    pub(crate) fn load() -> Option<Self> {
        Some(Self {
            path: std::env::var_os("FAST_PACK_UI_PROFILE").map(PathBuf::from)?,
            previous: None,
            frames: Vec::new(),
            count: 0,
        })
    }

    pub(crate) fn frame(&mut self, now: Instant) -> (f32, Option<u8>, bool) {
        if let Some(previous) = self.previous.replace(now) {
            if self.count > 15 {
                self.frames
                    .push(now.saturating_duration_since(previous).as_secs_f64() * 1000.0);
            }
        }
        self.count += 1;
        let action = match self.count {
            40 | 140 => Some(0),
            70 | 110 => Some(1),
            _ => None,
        };
        let done = self.count >= 180;
        if done {
            self.frames.sort_by(f64::total_cmp);
            let percentile = |part: usize| {
                self.frames
                    .get((self.frames.len() * part / 100).min(self.frames.len().saturating_sub(1)))
                    .copied()
                    .unwrap_or_default()
            };
            let report = serde_json::json!({
                "frames":self.frames.len(), "median_ms":percentile(50), "p95_ms":percentile(95), "maximum_ms":percentile(100),
                "over_33ms":self.frames.iter().filter(|value| **value > 33.34).count(),
                "backend":std::env::var("ICED_BACKEND").unwrap_or_default(), "gpu_build":cfg!(feature="gpu-renderer")
            });
            if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
                let _ = std::fs::write(&self.path, bytes);
            }
        }
        (
            if self.count / 30 % 2 == 0 {
                18.0
            } else {
                -18.0
            },
            action,
            done,
        )
    }
}
