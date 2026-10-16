use crate::{ConversionOptions, ConversionReport, convert_pack, suggest_output};
use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};

pub struct Job {
    pub id: u64,
    pub source: PathBuf,
    pub output: PathBuf,
    pub options: ConversionOptions,
}

pub enum Event {
    Started(u64),
    Progress(u64, String, u8),
    Finished(u64, Box<std::result::Result<ConversionReport, String>>),
}

pub struct Pool {
    sender: Option<mpsc::Sender<Job>>,
    workers: Vec<std::thread::JoinHandle<()>>,
}

pub fn parallelism() -> usize {
    std::thread::available_parallelism()
        .map_or(1, usize::from)
        .clamp(1, 4)
}

impl Pool {
    pub fn new(workers: usize, callback: impl Fn(Event) + Send + Sync + 'static) -> Self {
        let (sender, receiver) = mpsc::channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));
        let callback = Arc::new(callback);
        let active = Arc::new(AtomicUsize::new(0));
        let workers = (0..workers.max(1))
            .map(|_| {
                let receiver = receiver.clone();
                let callback = callback.clone();
                let active = active.clone();
                std::thread::spawn(move || {
                    loop {
                        let job = receiver.lock().unwrap().recv();
                        let Ok(job) = job else { break };
                        active.fetch_add(1, Ordering::Relaxed);
                        let budget = crate::parallel::batch_budget(active.clone());
                        callback(Event::Started(job.id));
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            convert_pack(&job.source, &job.output, &job.options, |step, percent| {
                                callback(Event::Progress(job.id, step.into(), percent));
                            })
                        }))
                        .map_err(|_| "转换线程意外停止".to_string())
                        .and_then(|result| result.map_err(|error| format!("{error:#}")));
                        drop(budget);
                        active.fetch_sub(1, Ordering::Relaxed);
                        callback(Event::Finished(job.id, Box::new(result)));
                    }
                })
            })
            .collect();
        Self {
            sender: Some(sender),
            workers,
        }
    }

    pub fn submit(&self, job: Job) -> Result<()> {
        ensure!(
            self.sender
                .as_ref()
                .is_some_and(|sender| sender.send(job).is_ok()),
            "批量转换队列已关闭"
        );
        Ok(())
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.sender.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

pub fn path_key(path: &Path) -> String {
    let path = path.canonicalize().unwrap_or_else(|_| {
        path.parent()
            .and_then(|parent| parent.canonicalize().ok())
            .and_then(|parent| path.file_name().map(|name| parent.join(name)))
            .unwrap_or_else(|| path.to_owned())
    });
    let key = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        key.to_lowercase()
    } else {
        key
    }
}

pub fn output_path(
    source: &Path,
    target: &str,
    as_zip: bool,
    directory: Option<&Path>,
    reserved: &[PathBuf],
    inputs: &[PathBuf],
    avoid_existing: bool,
) -> PathBuf {
    let suggested = suggest_output(source, target, as_zip);
    let initial = directory.map_or_else(
        || suggested.clone(),
        |directory| directory.join(suggested.file_name().unwrap_or_default()),
    );
    let stem = if as_zip {
        initial.file_stem()
    } else {
        initial.file_name()
    }
    .unwrap_or_default()
    .to_string_lossy();
    let mut candidate = initial.clone();
    let mut index = 2;
    loop {
        let key = path_key(&candidate);
        let conflicts = reserved
            .iter()
            .chain(inputs)
            .any(|path| path_key(path) == key);
        if !conflicts && !(avoid_existing && candidate.exists()) {
            return candidate;
        }
        candidate = initial.with_file_name(format!(
            "{stem} ({index}){}",
            if as_zip { ".zip" } else { "" }
        ));
        index += 1;
    }
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
