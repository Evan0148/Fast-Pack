use super::*;

fn app() -> App {
    let (mut app, _) = App::boot();
    app.smoke = false;
    app.capture = None;
    app.smoke_output = None;
    app.target = "1.20.1".into();
    app.target_selected = true;
    app.startup_frame = false;
    app
}

fn info(name: &str) -> PackInfo {
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
        suggested_target: "26.4".into(),
        error: None,
        features: Default::default(),
    }
}

fn ready(app: &mut App, id: u64, source: PathBuf) {
    app.next_id = app.next_id.max(id);
    app.queue.push(QueueEntry {
        id,
        key: fast_pack::batch::path_key(&source),
        source: source.clone(),
        info: Some(info(&source.display().to_string())),
        phase: Phase::Ready,
        source_version: "1.8.9".into(),
        output: None,
        progress: 0,
        step: "等待转换".into(),
        submitted: false,
        error: None,
        report: None,
    });
    app.generation = id;
    app.source = Some(source);
    app.info = app.queue.last().unwrap().info.clone();
    app.refresh_queue_phase();
    app.update_output();
}

#[test]
fn separate_drops_append_and_duplicate_paths_are_ignored() {
    let mut app = app();
    app.load(PathBuf::from("first.zip"));
    app.load(PathBuf::from("second.zip"));
    app.load(PathBuf::from("first.zip"));
    assert_eq!(app.queue.len(), 2);
    assert_eq!(app.source.as_deref(), Some(Path::new("first.zip")));
}

#[test]
fn drops_during_conversion_keep_current_job_and_append() {
    let mut app = app();
    ready(&mut app, 1, "first.zip".into());
    app.batch_options = Some(ConversionOptions::default());
    app.phase = Phase::Running;
    app.queue[0].phase = Phase::Running;
    app.queue[0].progress = 60;
    app.load("second.zip".into());
    assert_eq!(app.queue.len(), 2);
    assert!(app.busy());
    assert_eq!(app.queue[0].progress, 60);
    assert_eq!(app.generation, 1);
    let _ = app.update(Message::Target("1.8.9".into()));
    assert_eq!(app.target, "1.20.1");
}

#[test]
fn stale_results_after_clear_cannot_repopulate_the_queue() {
    let mut app = app();
    ready(&mut app, 1, "first.zip".into());
    let _ = app.update(Message::NewPack);
    app.sender
        .send(WorkEvent::Probed(1, info("first.zip")))
        .unwrap();
    let _ = app.update(Message::Tick(Instant::now()));
    assert!(app.queue.is_empty());
    assert!(app.info.is_none());
    assert_eq!(app.phase, Phase::Empty);
    app.load("next.zip".into());
    assert!(app.queue[0].id > 1);
}

#[test]
fn progress_is_per_pack_and_failure_does_not_finish_another_job() {
    let mut app = app();
    ready(&mut app, 1, "one.zip".into());
    ready(&mut app, 2, "two.zip".into());
    app.batch_options = Some(ConversionOptions::default());
    app.phase = Phase::Running;
    for entry in &mut app.queue {
        entry.submitted = true;
        entry.phase = Phase::Running;
    }
    app.sender
        .send(WorkEvent::Batch(fast_pack::batch::Event::Progress(
            1,
            "更新模型".into(),
            60,
        )))
        .unwrap();
    app.sender
        .send(WorkEvent::Batch(fast_pack::batch::Event::Progress(
            2,
            "载入".into(),
            10,
        )))
        .unwrap();
    app.sender
        .send(WorkEvent::Batch(fast_pack::batch::Event::Finished(
            2,
            Box::new(Err("损坏的 ZIP".into())),
        )))
        .unwrap();
    let _ = app.update(Message::Tick(Instant::now()));
    assert_eq!(app.queue[0].progress, 60);
    assert_eq!(app.queue[1].phase, Phase::Failed);
    assert!(app.busy());
    assert_eq!(app.actual_progress, 80);
    app.sender
        .send(WorkEvent::Batch(fast_pack::batch::Event::Finished(
            1,
            Box::new(Ok(ConversionReport::default())),
        )))
        .unwrap();
    let _ = app.update(Message::Tick(Instant::now()));
    assert_eq!(app.phase, Phase::Done);
    assert!(!app.busy());
    assert!(app.result_note.as_deref().unwrap().contains("失败 1"));
}

#[test]
fn overwrite_confirmation_covers_the_whole_batch() {
    let work = tempfile::tempdir().unwrap();
    let existing = work.path().join("[1.20.1]one.zip");
    std::fs::write(&existing, b"unchanged").unwrap();
    let mut app = app();
    app.export_directory_input = work.path().display().to_string();
    ready(&mut app, 1, work.path().join("one.zip"));
    ready(&mut app, 2, work.path().join("two.zip"));
    let _ = app.update(Message::Start);
    assert!(app.confirm_overwrite);
    assert!(!app.busy());
    let _ = app.update(Message::CancelOverwrite);
    assert!(!app.confirm_overwrite);
    assert_eq!(std::fs::read(existing).unwrap(), b"unchanged");
}

#[test]
fn preview_selection_keeps_events_isolated() {
    let mut app = app();
    ready(&mut app, 1, "one.zip".into());
    ready(&mut app, 2, "two.zip".into());
    let preview = fast_pack::compat::sky::Preview {
        names: vec!["old".into()],
        selected_layer: "old".into(),
        daytime: true,
        image: image::RgbaImage::new(3, 2),
    };
    app.sender
        .send(WorkEvent::Preview(
            1,
            Ok(Some(LoadedPreview::from(preview))),
        ))
        .unwrap();
    let _ = app.update(Message::Tick(Instant::now()));
    assert!(app.sky_preview.is_none());
    let _ = app.update(Message::QueueSelect(1));
    assert_eq!(app.queue.len(), 2);
    assert_eq!(app.generation, 1);
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
fn heypixel_stays_disabled_when_an_enable_message_is_sent() {
    let mut app = app();
    assert!(!app.heypixel_compat);
    let _ = app.update(Message::HeyPixel(true));
    assert!(!app.heypixel_compat);
    assert_eq!(app.heypixel_motion.value(app.now), 0.0);
}

#[test]
fn appended_output_names_use_the_frozen_batch_version() {
    let work = tempfile::tempdir().unwrap();
    let mut app = app();
    ready(&mut app, 1, work.path().join("one.zip"));
    ready(&mut app, 2, work.path().join("two.zip"));
    app.batch_options = Some(ConversionOptions {
        tgt_format: 15,
        target_version: Some("1.20.1".into()),
        as_zip: true,
        ..Default::default()
    });
    app.target = "26.4".into();
    app.as_zip = false;
    app.plan_queue_outputs(true).unwrap();
    for entry in &app.queue {
        let filename = entry
            .output
            .as_ref()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy();
        assert!(filename.starts_with("[1.20.1]"));
        assert!(filename.ends_with(".zip"));
    }
}

#[test]
fn preview_keeps_polling_after_identification_and_applies_without_input() {
    let mut app = app();
    ready(&mut app, 1, "day.zip".into());
    let idle = iced::advanced::subscription::into_recipes(app.subscription()).len();
    app.preview_pending = Some(1);
    assert!(iced::advanced::subscription::into_recipes(app.subscription()).len() > idle);
    let loaded = LoadedPreview {
        handle: widget::image::Handle::from_rgba(3, 2, vec![180; 24]),
        daytime: true,
        layers: 3,
    };
    app.sender
        .send(WorkEvent::Preview(1, Ok(Some(loaded))))
        .unwrap();
    let _ = app.update(Message::Tick(Instant::now()));
    assert!(app.sky_preview.is_some());
    assert_eq!(app.sky_layers, 3);
    assert!(app.preview_pending.is_none());
}

#[test]
fn switching_back_reuses_the_correct_pack_preview_without_decoding_again() {
    let mut app = app();
    ready(&mut app, 1, "red.zip".into());
    ready(&mut app, 2, "blue.zip".into());
    for (id, color) in [(1, [220, 20, 20, 255]), (2, [20, 40, 220, 255])] {
        let handle = widget::image::Handle::from_rgba(3, 2, color.repeat(6));
        app.sender
            .send(WorkEvent::Preview(
                id,
                Ok(Some(LoadedPreview {
                    handle,
                    daytime: true,
                    layers: id as usize,
                })),
            ))
            .unwrap();
    }
    let _ = app.update(Message::Tick(Instant::now()));
    let blue = app.sky_preview.as_ref().unwrap().id();
    app.select_queue(1);
    let red = app.sky_preview.as_ref().unwrap().id();
    assert_ne!(red, blue);
    assert_eq!(app.sky_layers, 1);
    app.select_queue(2);
    assert_eq!(app.sky_preview.as_ref().unwrap().id(), blue);
    assert_eq!(app.sky_layers, 2);
    assert!(app.preview_pending.is_none());
    assert!(app.preview_request.is_none());
}

#[test]
fn fast_preview_requests_are_coalesced_while_a_read_is_active() {
    let mut app = app();
    ready(&mut app, 1, "one.zip".into());
    ready(&mut app, 2, "two.zip".into());
    ready(&mut app, 3, "three.zip".into());
    app.preview_pending = Some(1);
    app.request_preview(2, "two.zip".into());
    app.request_preview(3, "three.zip".into());
    assert_eq!(app.preview_pending, Some(1));
    assert_eq!(app.preview_request.as_ref().unwrap().0, 3);
    let _ = app.update(Message::NewPack);
    assert!(app.preview_request.is_none());
    assert!(app.preview_cache.is_empty());
}

#[cfg(not(feature = "gpu-renderer"))]
#[test]
fn repeated_partial_redraw_does_not_darken_panels_in_either_theme() {
    use iced::advanced::Renderer as _;
    use iced::advanced::{layout, renderer, widget::Tree};
    let size = Size::new(1040.0, 780.0);
    let bounds = iced::Rectangle::with_size(size);
    let viewport =
        iced::advanced::graphics::Viewport::with_physical_size(iced::Size::new(1040, 780), 1.0);
    for dark in [false, true] {
        let mut app = app();
        app.dark = dark;
        app.theme_motion
            .set(if dark { 1.0 } else { 0.0 }, app.now, true);
        let mut renderer = iced::Renderer::new(
            Font::with_name(crate::windows_compat::font_family()),
            iced::Pixels(16.0),
        );
        renderer.reset(bounds);
        let mut view = app.view();
        let mut tree = Tree::new(view.as_widget());
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        view.as_widget().draw(
            &tree,
            &mut renderer,
            &app.theme(),
            &renderer::Style {
                text_color: app.colors().foreground,
            },
            iced::advanced::Layout::new(&node),
            iced::mouse::Cursor::Unavailable,
            &bounds,
        );
        let mut pixels = tiny_skia::Pixmap::new(1040, 780).unwrap();
        let mut mask = tiny_skia::Mask::new(1040, 780).unwrap();
        renderer.draw(
            &mut pixels.as_mut(),
            &mut mask,
            &viewport,
            &[bounds],
            app.colors().background,
        );
        let original = pixels.data().to_vec();
        let regions = [
            iced::Rectangle {
                x: 580.0,
                y: 244.0,
                width: 380.0,
                height: 44.0,
            },
            iced::Rectangle {
                x: 70.0,
                y: 282.0,
                width: 390.0,
                height: 100.0,
            },
            iced::Rectangle {
                x: 610.0,
                y: 410.0,
                width: 180.0,
                height: 42.0,
            },
        ];
        for _ in 0..20 {
            renderer.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &viewport,
                &regions,
                app.colors().background,
            );
        }
        let changed_outside: Vec<_> = original
            .chunks_exact(4)
            .zip(pixels.data().chunks_exact(4))
            .enumerate()
            .filter_map(|(index, (before, after))| {
                let point = iced::Point::new((index % 1040) as f32, (index / 1040) as f32);
                (before != after && !regions.iter().any(|region| region.contains(point)))
                    .then(|| (point, before.to_vec(), after.to_vec()))
            })
            .take(8)
            .collect();
        assert!(
            changed_outside.is_empty(),
            "partial repaint modified pixels outside the update region; dark={dark}; samples={changed_outside:?}"
        );
    }
}
