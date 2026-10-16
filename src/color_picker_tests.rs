use super::*;

fn app() -> App {
    let (mut app, _) = App::boot();
    app.target = "1.20.1".into();
    app
}

#[test]
fn each_palette_updates_its_own_setting_without_touching_glint() {
    let mut app = app();
    let original = app.glint_color;
    app.open_color_picker(ColorTarget::FishingLine);
    let _ = app.update(Message::GlintPreset([12, 34, 56]));
    assert_eq!(app.fishing_line, Some([12, 34, 56]));
    assert_eq!(app.glint_color, original);
    assert!(!app.custom_glint);
    app.close_glint_picker();
    app.open_color_picker(ColorTarget::BlockOutline);
    let _ = app.update(Message::GlintHex("#ABCDEF".into()));
    assert_eq!(app.block_outline, Some([171, 205, 239]));
    assert_eq!(app.fishing_line, Some([12, 34, 56]));
    assert_eq!(app.glint_color, original);
    app.close_glint_picker();
    let _ = app.update(Message::FishingLine(false));
    assert_eq!(app.fishing_line_color, [12, 34, 56]);
    assert!(app.fishing_line.is_none());
}

#[test]
fn preset_color_preserves_shape_and_alpha_while_canvas_color_only_changes_the_pen() {
    let mut app = app();
    let original = app.selected_crosshair().unwrap();
    app.open_color_picker(ColorTarget::Crosshair);
    let _ = app.update(Message::GlintPreset([37, 93, 168]));
    let tinted = app.selected_crosshair().unwrap();
    assert_eq!(tinted.image.dimensions(), original.image.dimensions());
    for (before, after) in original.image.pixels().zip(tinted.image.pixels()) {
        assert_eq!(before[3], after[3]);
        if before[3] > 0 {
            assert_eq!(after.0[..3], [37, 93, 168]);
        }
    }
    assert!(!app.custom_glint);
    app.crosshair_drawing = true;
    app.crosshair_canvas.paint(3, 3, [255, 255, 255, 255]);
    app.open_color_picker(ColorTarget::Crosshair);
    let _ = app.update(Message::GlintPreset([1, 2, 3]));
    assert_eq!(app.crosshair_canvas.color(3, 3), [255, 255, 255, 255]);
    assert_eq!(app.crosshair_pen_color, [1, 2, 3]);
}

#[test]
fn outline_palette_saves_the_selected_item_and_preferences_round_trip_theme_and_colors() {
    use super::super::item_editor::Event;
    let mut app = app();
    let _ = app.update(Message::Items(Event::Open));
    let _ = app.update(Message::Items(Event::Catalog(
        "1.20.1".into(),
        fast_pack::compat::custom_items::items("1.20.1"),
    )));
    app.open_color_picker(ColorTarget::ItemOutline);
    let _ = app.update(Message::GlintPreset([23, 45, 67]));
    app.close_glint_picker();
    assert_eq!(
        app.item_editor.rules[0].outline.as_ref().unwrap().color,
        [23, 45, 67]
    );
    let preferences = crate::preferences::Preferences {
        dark: false,
        fishing_line: Some([12, 34, 56]),
        fishing_line_color: [12, 34, 56],
        block_outline_color: [65, 43, 21],
        crosshair_tint: Some([23, 45, 67]),
        ..Default::default()
    };
    let bytes = serde_json::to_vec(&preferences).unwrap();
    let restored: crate::preferences::Preferences = serde_json::from_slice(&bytes).unwrap();
    assert!(!restored.dark);
    assert_eq!(restored.fishing_line, Some([12, 34, 56]));
    assert_eq!(restored.block_outline_color, [65, 43, 21]);
    assert_eq!(restored.crosshair_tint, Some([23, 45, 67]));
}
