use super::*;
use std::io::Read;
use std::sync::{Condvar, atomic::AtomicUsize};
use std::time::Duration;

fn fixture(directory: &Path, name: &str) -> PathBuf {
    let root = directory.join(name);
    std::fs::create_dir_all(root.join("assets/minecraft/textures/items")).unwrap();
    std::fs::write(
        root.join("pack.mcmeta"),
        br#"{"pack":{"pack_format":1,"description":"batch"}}"#,
    )
    .unwrap();
    std::fs::write(root.join("marker.txt"), name).unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([150, 70, 20, 255]))
        .save(root.join("assets/minecraft/textures/items/diamond_sword.png"))
        .unwrap();
    root
}

fn job(id: u64, source: PathBuf, output: PathBuf) -> Job {
    Job {
        id,
        source,
        output,
        options: ConversionOptions {
            target_version: Some("1.20.1".into()),
            tgt_format: 15,
            auto_client_setup: false,
            heypixel_compat: false,
            ..Default::default()
        },
    }
}

#[test]
fn appends_while_two_jobs_run_and_keeps_pack_data_isolated() {
    let work = tempfile::tempdir().unwrap();
    let inputs: Vec<_> = (0..3)
        .map(|id| fixture(work.path(), &format!("pack-{id}")))
        .collect();
    let (sender, receiver) = mpsc::channel();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let pool = Pool::new(2, {
        let gate = gate.clone();
        let running = running.clone();
        let peak = peak.clone();
        move |event| {
            if matches!(&event, Event::Started(_)) {
                let current = running.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(current, Ordering::SeqCst);
            }
            let started = matches!(&event, Event::Started(id) if *id < 2);
            let finished = matches!(&event, Event::Finished(..));
            sender.send(event).unwrap();
            if started {
                let _ = gate
                    .1
                    .wait_timeout_while(
                        gate.0.lock().unwrap(),
                        Duration::from_secs(5),
                        |released| !*released,
                    )
                    .unwrap();
            }
            if finished {
                running.fetch_sub(1, Ordering::SeqCst);
            }
        }
    });
    for id in 0..2 {
        pool.submit(job(
            id as u64,
            inputs[id].clone(),
            work.path().join(format!("out-{id}.zip")),
        ))
        .unwrap();
    }
    let mut started = 0;
    while started < 2 {
        if matches!(
            receiver.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::Started(_)
        ) {
            started += 1;
        }
    }
    pool.submit(job(2, inputs[2].clone(), work.path().join("out-2.zip")))
        .unwrap();
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    let mut finished = 0;
    while finished < 3 {
        if let Event::Finished(_, result) = receiver.recv_timeout(Duration::from_secs(20)).unwrap()
        {
            assert!(result.is_ok(), "{result:?}");
            finished += 1;
        }
    }
    drop(pool);
    assert!(peak.load(Ordering::SeqCst) >= 2);
    for id in 0..3 {
        let mut archive = zip::ZipArchive::new(
            std::fs::File::open(work.path().join(format!("out-{id}.zip"))).unwrap(),
        )
        .unwrap();
        let mut marker = String::new();
        archive
            .by_name("marker.txt")
            .unwrap()
            .read_to_string(&mut marker)
            .unwrap();
        assert_eq!(marker, format!("pack-{id}"));
        assert!(
            archive
                .by_name("assets/minecraft/textures/item/diamond_sword.png")
                .is_ok()
        );
    }
}

#[test]
fn failed_pack_does_not_stop_other_queued_packs() {
    let work = tempfile::tempdir().unwrap();
    let input = fixture(work.path(), "good");
    let (sender, receiver) = mpsc::channel();
    let pool = Pool::new(2, move |event| {
        let _ = sender.send(event);
    });
    pool.submit(job(
        1,
        work.path().join("missing.zip"),
        work.path().join("bad.zip"),
    ))
    .unwrap();
    pool.submit(job(2, input, work.path().join("good.zip")))
        .unwrap();
    let mut results = std::collections::BTreeMap::new();
    while results.len() < 2 {
        if let Event::Finished(id, result) = receiver.recv_timeout(Duration::from_secs(20)).unwrap()
        {
            results.insert(id, result.is_ok());
        }
    }
    assert_eq!(results[&1], false);
    assert_eq!(results[&2], true);
    assert!(!work.path().join("bad.zip").exists());
}

#[test]
fn same_names_get_distinct_outputs_without_overwriting_inputs() {
    let work = tempfile::tempdir().unwrap();
    let inputs = vec![
        work.path().join("first/pack.zip"),
        work.path().join("second/pack.zip"),
        work.path().join("[1.20.1]pack.zip"),
    ];
    let first = output_path(
        &inputs[0],
        "1.20.1",
        true,
        Some(work.path()),
        &[],
        &inputs,
        false,
    );
    let second = output_path(
        &inputs[1],
        "1.20.1",
        true,
        Some(work.path()),
        &[first.clone()],
        &inputs,
        false,
    );
    assert_ne!(first, second);
    assert_ne!(first, inputs[2]);
    std::fs::write(&second, b"existing output").unwrap();
    let appended = output_path(
        &inputs[1],
        "1.20.1",
        true,
        Some(work.path()),
        &[first],
        &inputs,
        true,
    );
    assert_ne!(appended, second);
    assert_eq!(std::fs::read(second).unwrap(), b"existing output");
}

#[test]
fn deleting_a_queued_job_skips_its_conversion_and_preserves_input() {
    let work = tempfile::tempdir().unwrap();
    let first = fixture(work.path(), "first");
    let second = fixture(work.path(), "removed");
    let third = fixture(work.path(), "third");
    let (sender, receiver) = mpsc::channel();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let pool = Pool::new(1, {
        let gate = gate.clone();
        move |event| {
            let blocked = matches!(&event, Event::Started(1));
            sender.send(event).unwrap();
            if blocked {
                let _ = gate
                    .1
                    .wait_timeout_while(
                        gate.0.lock().unwrap(),
                        Duration::from_secs(5),
                        |released| !*released,
                    )
                    .unwrap();
            }
        }
    });
    pool.submit(job(1, first, work.path().join("one.zip")))
        .unwrap();
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(5)).unwrap(),
        Event::Started(1)
    ));
    assert!(!pool.cancel_pending(1));
    pool.submit(job(2, second.clone(), work.path().join("removed.zip")))
        .unwrap();
    pool.submit(job(3, third, work.path().join("three.zip")))
        .unwrap();
    assert!(pool.is_pending(2));
    assert!(pool.cancel_pending(2));
    assert!(!pool.is_pending(2));
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    let mut completed = Vec::new();
    while completed.len() < 2 {
        match receiver.recv_timeout(Duration::from_secs(20)).unwrap() {
            Event::Started(2) | Event::Progress(2, ..) | Event::Finished(2, _) => {
                panic!("deleted task was executed")
            }
            Event::Finished(id, result) => {
                assert!(result.is_ok());
                completed.push(id);
            }
            _ => {}
        }
    }
    drop(pool);
    assert_eq!(completed, [1, 3]);
    assert!(!work.path().join("removed.zip").exists());
    assert_eq!(
        std::fs::read(second.join("marker.txt")).unwrap(),
        b"removed"
    );
}

#[test]
fn heypixel_cannot_be_enabled_by_conversion_options() {
    let work = tempfile::tempdir().unwrap();
    let input = fixture(work.path(), "disabled");
    let metadata = input.join("fastpack/via-bridge/manifest.json");
    std::fs::create_dir_all(metadata.parent().unwrap()).unwrap();
    std::fs::write(&metadata, br#"{"enabled":true,"items":[]}"#).unwrap();
    let output = work.path().join("disabled.zip");
    let mut stages = Vec::new();
    let options = ConversionOptions {
        target_version: Some("1.20.1".into()),
        tgt_format: 15,
        heypixel_compat: true,
        auto_client_setup: true,
        ..Default::default()
    };
    convert_pack(&input, &output, &options, |step, _| {
        stages.push(step.to_string())
    })
    .unwrap();
    assert!(!stages.iter().any(|stage| stage.contains("布吉岛")));
    let mut zip = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
    let value: serde_json::Value =
        serde_json::from_reader(zip.by_name("fastpack/via-bridge/manifest.json").unwrap()).unwrap();
    assert_eq!(value["enabled"], false);
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(metadata).unwrap()).unwrap();
    assert_eq!(original["enabled"], true);
    assert!(
        crate::client_audio::install(work.path())
            .unwrap_err()
            .to_string()
            .contains("临时停用")
    );
}
