use super::*;
use iced::advanced::text::editor::{Cursor, Position};

#[test]
fn result_warning_details_toggle_without_changing_the_saved_draft() {
    let (mut app, _) = App::boot();
    let before = serde_json::to_value(app.overlay.draft()).unwrap();
    assert!(!app.overlay.warnings_expanded);
    let _ = app.overlay_update(Event::ToggleWarnings);
    assert!(app.overlay.warnings_expanded);
    let _ = app.overlay_update(Event::ToggleWarnings);
    assert!(!app.overlay.warnings_expanded);
    assert_eq!(serde_json::to_value(app.overlay.draft()).unwrap(), before);
}

#[cfg(not(feature = "gpu-renderer"))]
#[test]
fn description_editor_draws_initial_text_before_any_input() {
    use iced::advanced::{
        Renderer as _, layout, renderer,
        widget::{Id, Operation, Tree, operation},
    };
    #[derive(Default)]
    struct EditorBounds(Option<iced::Rectangle>);
    impl Operation for EditorBounds {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn focusable(
            &mut self,
            id: Option<&Id>,
            bounds: iced::Rectangle,
            _: &mut dyn operation::Focusable,
        ) {
            if id == Some(&Id::new("overlay-description")) {
                self.0 = Some(bounds);
            }
        }
    }
    let (mut app, _) = App::boot();
    app.overlay_mode = true;
    app.target = "1.20.1".into();
    let mut renderer = iced::Renderer::new(
        iced::Font::with_name(crate::windows_compat::font_family()),
        iced::Pixels(16.0),
    );
    let bounds = iced::Rectangle::with_size(iced::Size::new(1040.0, 980.0));
    let mut view = app.view();
    let mut tree = Tree::new(view.as_widget());
    let node = view.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(iced::Size::ZERO, bounds.size()),
    );
    let mut editor_bounds = EditorBounds::default();
    view.as_widget_mut().operate(
        &mut tree,
        iced::advanced::Layout::new(&node),
        &renderer,
        &mut editor_bounds,
    );
    renderer.reset(bounds);
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
    let mut pixels = tiny_skia::Pixmap::new(1040, 980).unwrap();
    let mut mask = tiny_skia::Mask::new(1040, 980).unwrap();
    renderer.draw(
        &mut pixels.as_mut(),
        &mut mask,
        &iced::advanced::graphics::Viewport::with_physical_size(iced::Size::new(1040, 980), 1.0),
        &[bounds],
        app.colors().background,
    );
    let field = editor_bounds.0.unwrap().shrink(12);
    let mut bright = 0;
    for y in field.y as u32..(field.y + field.height) as u32 {
        for x in field.x as u32..(field.x + field.width) as u32 {
            let pixel = pixels.pixel(x, y).unwrap();
            if pixel.red() > 150 && pixel.green() > 150 && pixel.blue() > 150 {
                bright += 1;
            }
        }
    }
    if let Some(path) = std::env::var_os("FAST_PACK_RENDER_CHECK") {
        image::RgbaImage::from_raw(1040, 980, pixels.data().to_vec())
            .unwrap()
            .save(path)
            .unwrap();
    }
    assert!(
        bright > 40,
        "Initial description text is invisible ({bright} bright pixels)"
    );
}

#[test]
fn selected_description_colors_and_unicode_positions_round_trip_with_the_draft() {
    let mut composer = Composer::new();
    composer.description = Description::plain("中文abc\n第二行");
    composer.content = text_editor::Content::with_text(&composer.description.text);
    composer.content.move_to(Cursor {
        position: Position { line: 0, column: 6 },
        selection: Some(Position { line: 0, column: 3 }),
    });
    assert_eq!(composer.selection(), Some(1..2));
    composer.set_color([12, 34, 56]);
    assert_eq!(composer.description.colors[0], None);
    assert_eq!(composer.description.colors[1], Some([12, 34, 56]));
    composer.edit(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
    composer.edit(text_editor::Action::Edit(text_editor::Edit::Insert('新')));
    assert_eq!(composer.description.text, "中文abc\n第二行新");
    assert_eq!(
        composer.description.colors.last(),
        Some(&Some([12, 34, 56]))
    );
    let saved = serde_json::to_vec(&composer.draft()).unwrap();
    let restored = Composer::from_draft(serde_json::from_slice(&saved).unwrap());
    assert_eq!(restored.description.text, composer.description.text);
    assert_eq!(restored.description.colors, composer.description.colors);
}

#[test]
fn one_click_formatting_applies_to_whole_text_or_selection_and_keeps_fields_independent() {
    let mut composer = Composer::new();
    composer.set_name_color([255, 85, 85]);
    composer.toggle_style(TextField::Name, Format::Bold);
    assert!(
        composer
            .name_colors
            .iter()
            .all(|color| *color == Some([255, 85, 85]))
    );
    assert!(composer.name_styles.iter().all(|style| style.bold));
    assert!(composer.description.colors.iter().all(Option::is_none));
    composer.set_color([85, 255, 255]);
    composer.toggle_style(TextField::Description, Format::Italic);
    assert!(
        composer
            .description
            .colors
            .iter()
            .all(|color| *color == Some([85, 255, 255]))
    );
    assert!(composer.description.styles.iter().all(|style| style.italic));
    composer.content.move_to(Cursor {
        position: Position { line: 0, column: 6 },
        selection: Some(Position { line: 0, column: 3 }),
    });
    composer.toggle_style(TextField::Description, Format::Underlined);
    assert!(composer.description.styles[1].underlined);
    assert!(!composer.description.styles[0].underlined);
    composer.clear_format(TextField::Description);
    assert_eq!(composer.description.colors[1], None);
    assert_eq!(composer.description.styles[1], TextStyle::default());
    assert!(composer.description.styles[0].italic);
    let restored = Composer::from_draft(
        serde_json::from_slice(&serde_json::to_vec(&composer.draft()).unwrap()).unwrap(),
    );
    assert_eq!(restored.name_colors, composer.name_colors);
    assert_eq!(restored.name_styles, composer.name_styles);
    assert_eq!(restored.description.styles, composer.description.styles);
}

#[test]
fn pasted_native_codes_become_editable_styles_and_name_edits_preserve_other_letters() {
    let mut composer = Composer::new();
    composer.content.perform(text_editor::Action::SelectAll);
    composer.edit(text_editor::Action::Edit(text_editor::Edit::Paste(
        std::sync::Arc::new("§c红§l粗\r\n§a绿§r原".into()),
    )));
    assert_eq!(composer.description.text, "红粗\n绿原");
    assert_eq!(composer.content.text(), composer.description.text);
    assert_eq!(composer.description.colors[0], Some([255, 85, 85]));
    assert!(composer.description.styles[1].bold);
    assert!(!composer.description.styles[3].bold);
    composer.set_name_color([85, 255, 85]);
    composer.toggle_style(TextField::Name, Format::Bold);
    composer
        .name_content
        .perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
    composer.edit_name(text_editor::Action::Edit(text_editor::Edit::Insert('新')));
    assert_eq!(composer.name, "我的覆盖包新");
    assert!(composer.name_styles.iter().all(|style| style.bold));
    assert!(
        composer
            .name_colors
            .iter()
            .all(|color| *color == Some([85, 255, 85]))
    );
    composer.edit_name(text_editor::Action::Edit(text_editor::Edit::Enter));
    assert!(!composer.name.contains('\n'));
}

#[test]
fn older_drafts_with_native_codes_restore_fonts_and_colors_without_new_fields() {
    let draft = serde_json::from_value(serde_json::json!({
        "name":"§6§l旧名称", "description":{"text":"§b§o旧简介\r\n第二行","colors":[]}
    }))
    .unwrap();
    let composer = Composer::from_draft(draft);
    assert_eq!(composer.name, "旧名称");
    assert_eq!(composer.name_ink, [255, 170, 0]);
    assert!(composer.name_typing_style.bold);
    assert_eq!(composer.description.text, "旧简介\n第二行");
    assert_eq!(composer.ink, [85, 255, 255]);
    assert!(composer.typing_style.italic);
}

#[test]
fn switching_modes_preserves_the_conversion_queue_and_uses_native_item_previews() {
    let (mut app, _) = App::boot();
    app.load("source.zip".into());
    let source = app.source.clone();
    let generation = app.generation;
    let _ = app.update(Message::Mode(true));
    assert!(app.overlay_mode);
    assert!(app.customization_source().is_none());
    app.load("not-an-overlay-input.zip".into());
    assert_eq!(app.queue.len(), 1);
    assert_eq!(app.source, source);
    assert_eq!(app.generation, generation);
    let _ = app.update(Message::Mode(false));
    assert_eq!(app.customization_source(), source);
    assert_eq!(app.queue.len(), 1);
}

#[test]
fn standalone_overlay_generation_needs_no_source_pack_and_does_not_overwrite() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, _) = App::boot();
    let _ = app.update(Message::Mode(true));
    app.target = "1.21.10".into();
    app.export_directory_input = directory.path().display().to_string();
    app.overlay.set_name_color([255, 85, 85]);
    app.overlay.toggle_style(TextField::Name, Format::Bold);
    app.overlay
        .toggle_style(TextField::Description, Format::Italic);
    let planned = app.overlay_destination().unwrap();
    assert!(
        planned
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("§c§l")
    );
    app.start();
    assert!(app.overlay.running);
    assert!(app.queue.is_empty());
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while app.overlay.running && Instant::now() < deadline {
        let _ = app.update(Message::Tick(Instant::now()));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!app.overlay.running);
    assert!(app.overlay.error.is_none(), "{:?}", app.overlay.error);
    assert_eq!(app.overlay.result.as_ref().unwrap().output, planned);
    assert!(planned.is_file());
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&planned).unwrap()).unwrap();
    let metadata: serde_json::Value =
        serde_json::from_reader(archive.by_name("pack.mcmeta").unwrap()).unwrap();
    assert_eq!(metadata["pack"]["description"]["extra"][0]["italic"], true);
    assert_ne!(app.overlay_destination().unwrap(), planned);
    assert!(app.source.is_none());
    assert_eq!(app.phase, super::super::Phase::Empty);
}
