use std::sync::OnceLock;
use std::time::Instant;

static STARTED: OnceLock<Instant> = OnceLock::new();

pub fn begin() {
    STARTED.get_or_init(Instant::now);
}

pub fn record(stage: &str) {
    let Some(path) = std::env::var_os("FAST_PACK_STARTUP_TIMING") else {
        return;
    };
    let milliseconds = STARTED.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0;
    let launch = std::env::var("FAST_PACK_STARTUP_LAUNCH")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .and_then(|start| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|now| now.as_secs_f64() * 1000.0 - start)
        });
    let value = serde_json::json!({"stage": stage, "milliseconds": milliseconds, "launch_milliseconds": launch});
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{value}");
    }
}
