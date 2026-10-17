use super::*;

#[test]
fn mixed_sprite_resolutions_merge_without_losing_uncovered_atlas_pixels() {
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    let textures = root.join("assets/minecraft/textures");
    let path = textures.join("gui/icons.png");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut atlas = RgbaImage::new(256, 256);
    atlas.put_pixel(240, 240, Rgba([123, 45, 67, 180]));
    crate::packfs::save_image(&atlas, &path).unwrap();
    let recipes: Vec<Sprite> =
        serde_json::from_str(include_str!("../../assets/gui-sprites.json")).unwrap();
    let first = recipes
        .iter()
        .find(|recipe| recipe.target == "gui/sprites/hud/crosshair.png")
        .unwrap();
    let second = recipes
        .iter()
        .find(|recipe| recipe.target == "gui/sprites/hud/heart/full.png")
        .unwrap();
    for (recipe, scale, color) in [
        (first, 2, Rgba([220, 20, 30, 255])),
        (second, 1, Rgba([10, 200, 30, 255])),
    ] {
        let path = textures.join(&recipe.target);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        crate::packfs::save_image(
            &RgbaImage::from_pixel(recipe.width * scale, recipe.height * scale, color),
            &path,
        )
        .unwrap();
    }
    gui(root, 69, 1, &mut AssetReport::default()).unwrap();
    let output = open(&path).unwrap();
    assert_eq!(output.width(), 512);
    assert_eq!(*output.get_pixel(480, 480), Rgba([123, 45, 67, 180]));
    assert_eq!(
        *output.get_pixel(first.x * 2, first.y * 2),
        Rgba([220, 20, 30, 255])
    );
    assert_eq!(
        *output.get_pixel(second.x * 2, second.y * 2),
        Rgba([10, 200, 30, 255])
    );
}
