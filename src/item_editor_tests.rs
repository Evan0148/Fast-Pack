use super::*;

#[test]
fn save_search_and_version_switch_keep_independent_rules() {
    let mut editor = Editor::new(Vec::new());
    let _ = editor.update(Event::Open, "1.20.1", None);
    let _ = editor.update(
        Event::Catalog("1.20.1".into(), custom_items::items("1.20.1")),
        "1.20.1",
        None,
    );
    let _ = editor.update(Event::Search("钻石剑".into()), "1.20.1", None);
    assert!(
        editor
            .results
            .iter()
            .any(|index| editor.entries[*index].id == "minecraft:diamond_sword")
    );
    let _ = editor.update(
        Event::Selected("minecraft:diamond_sword".into()),
        "1.20.1",
        None,
    );
    let _ = editor.update(Event::Name("自己的钻石剑".into()), "1.20.1", None);
    let _ = editor.update(Event::Held("150".into()), "1.20.1", None);
    let _ = editor.update(Event::Border(true), "1.20.1", None);
    let _ = editor.update(Event::Save, "1.20.1", None);
    let value = editor.applied("1.20.1");
    assert_eq!(value.len(), 1);
    assert_eq!(value[0].name.as_deref(), Some("自己的钻石剑"));
    assert_eq!(value[0].held_scale, 150);
    let _ = editor.sync("1.8.9", None);
    let _ = editor.update(
        Event::Catalog("1.20.1".into(), custom_items::items("1.20.1")),
        "1.8.9",
        None,
    );
    assert!(editor.entries.is_empty());
    let _ = editor.update(
        Event::Catalog("1.8.9".into(), custom_items::items("1.8.9")),
        "1.8.9",
        None,
    );
    assert!(editor.applied("1.8.9").is_empty());
    assert_eq!(editor.rules.len(), 1);
    let saved = serde_json::to_vec(&editor.rules).unwrap();
    let restored = Editor::new(serde_json::from_slice(&saved).unwrap());
    assert_eq!(restored.rules, editor.rules);
    assert!(!restored.enabled);
}

#[test]
fn invalid_edits_never_replace_saved_rule_and_restoring_removes_it() {
    let mut editor = Editor::new(Vec::new());
    let _ = editor.update(Event::Open, "1.20.1", None);
    let _ = editor.update(
        Event::Catalog("1.20.1".into(), custom_items::items("1.20.1")),
        "1.20.1",
        None,
    );
    let _ = editor.update(Event::Save, "1.20.1", None);
    assert_eq!(editor.rules.len(), 1);
    let _ = editor.update(Event::Inventory("0".into()), "1.20.1", None);
    let _ = editor.update(Event::Save, "1.20.1", None);
    assert_eq!(editor.rules[0].inventory_scale, 100);
    assert!(editor.note.contains("未保存"));
    let _ = editor.update(Event::Reset, "1.20.1", None);
    assert!(editor.rules.is_empty());
}

#[test]
fn switching_input_invalidates_the_previous_pack_preview() {
    let mut editor = Editor::new(Vec::new());
    let _ = editor.update(Event::Open, "1.20.1", Some("first.zip".into()));
    let _ = editor.update(
        Event::Catalog("1.20.1".into(), custom_items::items("1.20.1")),
        "1.20.1",
        Some("first.zip".into()),
    );
    let previous = editor.revision;
    let _ = editor.sync("1.20.1", Some("second.zip".into()));
    assert!(editor.revision > previous);
    let _ = editor.update(
        Event::Preview(previous, Ok(image::RgbaImage::new(16, 16))),
        "1.20.1",
        Some("second.zip".into()),
    );
    assert!(editor.preview.is_none());
    assert!(editor.preview_source.is_none());
}
