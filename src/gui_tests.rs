use super::*;

fn app() -> App {
    let (mut app, _) = App::boot();
    app.smoke = false;
    app.capture = None;
    app.smoke_output = None;
    app.phase = Phase::Empty;
    app.info = None;
    app.source = None;
    app
}

fn pack(name: &str) -> PackInfo {
    PackInfo {
        ok: true,
        path: name.into(),
        name: name.into(),
        src_format: Some(1),
        version_label: Some("1.8.9".into()),
        file_count: 22,
        texture_count: 10,
        has_mcmeta: true,
        has_assets: true,
        description: String::new(),
        layout: Some("legacy".into()),
        suggested_target: "26.2".into(),
        error: None,
        features: Default::default(),
    }
}

#[test]
fn ignores_stale_identification_result() {
    let mut app = app();
    app.generation = 2;
    app.source = Some(PathBuf::from("current.zip"));
    app.phase = Phase::Identifying;
    app.sender
        .send(WorkEvent::Probed(1, pack("stale.zip")))
        .unwrap();
    let _ = app.update(Message::Tick(Instant::now()));
    assert_eq!(app.phase, Phase::Identifying);
    assert!(app.info.is_none());
    app.sender
        .send(WorkEvent::Probed(2, pack("current.zip")))
        .unwrap();
    let _ = app.update(Message::Tick(Instant::now()));
    assert_eq!(app.phase, Phase::Ready);
    assert_eq!(app.info.as_ref().unwrap().name, "current.zip");
    assert!(app.output.ends_with("current_converted_26_2.zip"));
}

#[test]
fn progress_never_exceeds_confirmed_work_and_settles() {
    let mut app = app();
    app.phase = Phase::Running;
    app.sender
        .send(WorkEvent::Progress("更新模型引用".into(), 60))
        .unwrap();
    let now = Instant::now();
    let _ = app.update(Message::Tick(now));
    assert_eq!(app.actual_progress, 60);
    assert!(app.progress.value(now) <= 60.0);
    let _ = app.update(Message::Frame(now + Duration::from_millis(250)));
    assert_eq!(app.progress.value(app.now), 60.0);
    assert!(!app.animation_active());
}

#[test]
fn overwrite_requires_explicit_confirmation_and_cancellation_is_safe() {
    let work = tempfile::tempdir().unwrap();
    let path = work.path().join("existing.zip");
    std::fs::write(&path, b"unchanged").unwrap();
    let mut app = app();
    app.output = path.display().to_string();
    let _ = app.update(Message::Start);
    assert!(app.confirm_overwrite);
    assert!(!app.busy());
    let _ = app.update(Message::CancelOverwrite);
    assert!(!app.confirm_overwrite);
    assert_eq!(std::fs::read(path).unwrap(), b"unchanged");
}

#[test]
fn options_are_frozen_during_conversion_and_report_does_not_leak() {
    let mut app = app();
    app.phase = Phase::Running;
    let _ = app.update(Message::Target("1.8.9".into()));
    let _ = app.update(Message::Output("other.zip".into()));
    let _ = app.update(Message::PreserveLang(true));
    assert_eq!(app.target, "26.2");
    assert!(app.output.is_empty());
    assert!(!app.preserve_lang);
    app.phase = Phase::Done;
    app.report = Some("old report".into());
    let _ = app.update(Message::NewPack);
    assert!(app.report.is_none());
    assert_eq!(app.phase, Phase::Empty);
}

#[test]
fn reduced_motion_stops_all_interpolations() {
    let mut app = app();
    let _ = app.update(Message::Advanced);
    let _ = app.update(Message::Theme);
    let _ = app.update(Message::Hover(1, true));
    let _ = app.update(Message::ReducedMotion(true));
    assert!(!app.animation_active());
    assert_eq!(app.advanced_motion.value(app.now), 1.0);
    assert_eq!(app.hover[&1].value(app.now), 1.0);
}

#[test]
fn failure_can_recover_and_keeps_log_bounded() {
    let mut app = app();
    app.info = Some(pack("valid.zip"));
    app.fail("测试错误".into());
    assert_eq!(app.phase, Phase::Failed);
    let _ = app.update(Message::Target("1.21.11".into()));
    assert_eq!(app.phase, Phase::Ready);
    assert!(app.error.is_none());
    for index in 0..2000 {
        app.log(index.to_string());
    }
    assert!(app.logs.len() <= 500);
    assert_eq!(app.logs.last().unwrap(), "1999");
}

#[test]
fn ignores_preview_from_previously_uploaded_pack() {
    let mut app = app();
    app.generation = 3;
    let preview = fast_pack::compat::sky::Preview {
        names: vec!["old".into()],
        image: image::RgbaImage::new(3, 2),
    };
    app.sender
        .send(WorkEvent::Preview(2, Ok(Some(preview))))
        .unwrap();
    let _ = app.update(Message::Tick(Instant::now()));
    assert!(app.sky_preview.is_none());
    assert_eq!(app.sky_layers, 0);
}

#[test]
fn new_pack_clears_preview_and_invalidates_pending_events() {
    let mut app = app();
    app.phase = Phase::Done;
    app.generation = 7;
    app.sky_layers = 2;
    app.sky_preview = Some(widget::image::Handle::from_rgba(3, 2, vec![255; 24]));
    let _ = app.update(Message::NewPack);
    assert!(app.sky_preview.is_none());
    assert_eq!(app.sky_layers, 0);
    assert_eq!(app.generation, 8);
}
