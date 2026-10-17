use super::*;

#[test]
fn rainbow_switches_are_independent_and_fixed_colors_survive() {
    let mut app = app();
    let _ = app.update(Message::Items(super::super::item_editor::Event::Open));
    let _ = app.update(Message::Items(super::super::item_editor::Event::Catalog(
        "1.20.1".into(),
        fast_pack::compat::custom_items::items("1.20.1"),
    )));
    for target in [
        ColorTarget::Glint,
        ColorTarget::Crosshair,
        ColorTarget::ItemOutline,
        ColorTarget::FishingLine,
        ColorTarget::BlockOutline,
    ] {
        app.open_color_picker(target);
        app.set_picker_color([11, 22, 33]);
        let _ = app.update(Message::RainbowColor(true));
        assert!(app.rainbow_for(target));
        assert_eq!(app.color_for(target), [11, 22, 33]);
        let _ = app.update(Message::RainbowColor(false));
        assert!(!app.rainbow_for(target));
        assert_eq!(app.color_for(target), [11, 22, 33]);
        let _ = app.update(Message::RainbowColor(true));
        app.close_glint_picker();
    }
    assert!(
        app.rainbow.glint
            && app.rainbow.crosshair
            && app.rainbow.fishing_line
            && app.rainbow.block_outline
    );
    assert!(app.item_editor.rules[0].outline.as_ref().unwrap().rainbow);
    let preferences = crate::preferences::Preferences {
        rainbow: app.rainbow,
        custom_item_rules: app.item_editor.rules.clone(),
        ..Default::default()
    };
    let restored: crate::preferences::Preferences =
        serde_json::from_slice(&serde_json::to_vec(&preferences).unwrap()).unwrap();
    assert_eq!(restored.rainbow, app.rainbow);
    assert!(
        restored.custom_item_rules[0]
            .outline
            .as_ref()
            .unwrap()
            .rainbow
    );
    let old: crate::preferences::Preferences = serde_json::from_str("{}").unwrap();
    assert_eq!(old.rainbow, fast_pack::compat::rainbow::Settings::default());
}

#[test]
fn all_dynamic_palette_indicators_animate_without_changing_saved_colors() {
    let mut app = app();
    let _ = app.update(Message::Items(super::super::item_editor::Event::Open));
    let _ = app.update(Message::Items(super::super::item_editor::Event::Catalog(
        "1.20.1".into(),
        fast_pack::compat::custom_items::items("1.20.1"),
    )));
    for target in [
        ColorTarget::Glint,
        ColorTarget::Crosshair,
        ColorTarget::ItemOutline,
        ColorTarget::FishingLine,
        ColorTarget::BlockOutline,
    ] {
        app.open_color_picker(target);
        app.set_picker_color([11, 22, 33]);
        app.set_rainbow(true);
        app.close_glint_picker();
        assert_eq!(app.palette_caption(target).as_deref(), Some("动态彩色"));
        let start = app.rainbow_preview_started;
        app.now = start + std::time::Duration::from_secs(1);
        let first = app.palette_color(target);
        app.now = start + std::time::Duration::from_secs(3);
        assert_ne!(first, app.palette_color(target), "{target:?} stayed static");
        assert_eq!(app.palette_color(target), app.rainbow_preview_color());
        // Opening another palette must not reset the shared rainbow phase.
        let before = app.palette_color(target);
        app.open_color_picker(target);
        assert_eq!(app.palette_color(target), before);
        app.now = start + std::time::Duration::from_secs(7);
        assert_eq!(app.palette_color(target), first);
        assert_eq!(app.color_for(target), [11, 22, 33]);
        app.set_rainbow(false);
        app.close_glint_picker();
        assert_ne!(app.palette_caption(target).as_deref(), Some("动态彩色"));
        assert_eq!(app.palette_color(target), [11, 22, 33]);
    }
}

#[test]
fn dynamic_swatches_keep_refreshing_after_picker_closes_and_stop_when_hidden() {
    use std::time::Duration;
    let subscriptions =
        |app: &App| iced::advanced::subscription::into_recipes(app.subscription()).len();
    let mut app = app();
    app.startup_frame = false;
    app.open_color_picker(ColorTarget::FishingLine);
    app.set_rainbow(true);
    app.close_glint_picker();
    app.now += Duration::from_secs(1);
    let idle = subscriptions(&app);
    assert!(!app.rainbow_preview_active());
    app.advanced = true;
    app.advanced_motion.set(1.0, app.now, true);
    assert!(app.rainbow_preview_active());
    assert!(subscriptions(&app) > idle);
    assert!(
        !app.animation_active(),
        "swatches only need the shared 25 Hz tick"
    );
    app.learn_more = true;
    assert_eq!(subscriptions(&app), idle);
    app.learn_more = false;
    app.reduced = true;
    let fixed = app.palette_color(ColorTarget::FishingLine);
    app.now += Duration::from_secs(1);
    assert_eq!(fixed, app.palette_color(ColorTarget::FishingLine));
    assert_eq!(subscriptions(&app), idle);
    app.reduced = false;
    app.fishing_line = None;
    assert_eq!(subscriptions(&app), idle);
    app.custom_glint = false;
    app.rainbow.glint = true;
    assert_eq!(
        app.palette_caption(ColorTarget::Glint).as_deref(),
        Some("原版紫色")
    );
    assert_eq!(
        app.palette_color(ColorTarget::Glint),
        fast_pack::compat::glint::VANILLA_COLOR
    );
    assert_eq!(subscriptions(&app), idle);
}

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
