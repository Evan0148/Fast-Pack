use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Write;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use tempfile::{TempDir, tempdir};
use zip::{ZipWriter, write::SimpleFileOptions};
pub struct Fixture {
    _work: TempDir,
    path: PathBuf,
}
impl Deref for Fixture {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}
impl AsRef<OsStr> for Fixture {
    fn as_ref(&self) -> &OsStr {
        self.path.as_os_str()
    }
}
pub fn fixture(name: &str) -> Fixture {
    let work = tempdir().unwrap();
    let path = work.path().join(name);
    if name == "reference_upgrade.zip" {
        let mut archive = ZipWriter::new(File::create(&path).unwrap());
        for (name, data) in GOLDEN {
            archive
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            archive.write_all(data.as_bytes()).unwrap();
        }
        archive.finish().unwrap();
    } else {
        let entries = match name {
            "legacy" => LEGACY,
            "modern" => MODERN,
            _ => panic!("unknown fixture"),
        };
        for (name, data) in entries {
            let file = path.join(name);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, data.as_bytes()).unwrap();
        }
    }
    Fixture { _work: work, path }
}
const LEGACY: &[(&str, &str)] = &[
    (
        r###"assets/minecraft/lang/en_US.lang"###,
        r###"# Comment
greeting=你好
value = a=b
ignored
duplicate=first
duplicate=last
"###,
    ),
    (
        r###"assets/minecraft/lang/invalid.json"###,
        r###"not JSON"###,
    ),
    (
        r###"assets/minecraft/models/example.json"###,
        r###"{"textures": {"a": "minecraft:blocks/stonebrick", "b": "blocks/stone", "item": "items/sword_diamond"}}"###,
    ),
    (
        r###"assets/minecraft/textures/blocks/custom.png"###,
        r###"PNG fixture: custom"###,
    ),
    (
        r###"assets/minecraft/textures/blocks/custom.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/blocks/grass_path.png"###,
        r###"PNG fixture: grass_path"###,
    ),
    (
        r###"assets/minecraft/textures/blocks/grass_path.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/blocks/nested/stonebrick.png"###,
        r###"nested preserved"###,
    ),
    (
        r###"assets/minecraft/textures/blocks/planks_oak.png"###,
        r###"PNG fixture: planks_oak"###,
    ),
    (
        r###"assets/minecraft/textures/blocks/planks_oak.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/blocks/stone.png"###,
        r###"PNG fixture: stone"###,
    ),
    (
        r###"assets/minecraft/textures/blocks/stone.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/blocks/stonebrick.png"###,
        r###"PNG fixture: stonebrick"###,
    ),
    (
        r###"assets/minecraft/textures/blocks/stonebrick.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/blocks/tallgrass.png"###,
        r###"PNG fixture: tallgrass"###,
    ),
    (
        r###"assets/minecraft/textures/blocks/tallgrass.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/blocks/wool_colored_silver.png"###,
        r###"PNG fixture: wool_colored_silver"###,
    ),
    (
        r###"assets/minecraft/textures/blocks/wool_colored_silver.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/entity/chest.png"###,
        r###"entity"###,
    ),
    (
        r###"assets/minecraft/textures/items/sword_diamond.png"###,
        r###"item"###,
    ),
    (
        r###"pack.mcmeta"###,
        r###"{"pack": {"pack_format": 1, "description": {"text": "材质测试"}}, "extra": "discarded"}"###,
    ),
    (r###"README.TXT"###, r###"case-preserved on Windows"###),
];
const MODERN: &[(&str, &str)] = &[
    (
        r###"assets/minecraft/lang/en_us.json"###,
        r###"{"greeting":"你好", "value":3, "nested":{"k":"v"}}"###,
    ),
    (
        r###"assets/minecraft/lang/invalid.json"###,
        r###"not JSON"###,
    ),
    (
        r###"assets/minecraft/models/example.json"###,
        r###"{"textures": {"a": "minecraft:block/stone_bricks", "b": "block/stone", "item": "item/diamond_sword"}}"###,
    ),
    (
        r###"assets/minecraft/textures/block/custom.png"###,
        r###"PNG fixture: custom"###,
    ),
    (
        r###"assets/minecraft/textures/block/custom.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/block/dirt_path.png"###,
        r###"PNG fixture: dirt_path"###,
    ),
    (
        r###"assets/minecraft/textures/block/dirt_path.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/block/nested/stone_bricks.png"###,
        r###"nested preserved"###,
    ),
    (
        r###"assets/minecraft/textures/block/oak_planks.png"###,
        r###"PNG fixture: oak_planks"###,
    ),
    (
        r###"assets/minecraft/textures/block/oak_planks.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/block/short_grass.png"###,
        r###"PNG fixture: short_grass"###,
    ),
    (
        r###"assets/minecraft/textures/block/short_grass.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/block/stone.png"###,
        r###"PNG fixture: stone"###,
    ),
    (
        r###"assets/minecraft/textures/block/stone.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/block/stone_bricks.png"###,
        r###"PNG fixture: stone_bricks"###,
    ),
    (
        r###"assets/minecraft/textures/block/stone_bricks.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/block/sulfur.png"###,
        r###"PNG fixture: sulfur"###,
    ),
    (
        r###"assets/minecraft/textures/block/sulfur.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/entity/chest.png"###,
        r###"entity"###,
    ),
    (
        r###"assets/minecraft/textures/item/diamond_sword.png"###,
        r###"item"###,
    ),
    (
        r###"pack.mcmeta"###,
        r###"{"pack": {"pack_format": 88, "description": {"text": "材质测试"}}, "extra": "discarded"}"###,
    ),
    (r###"README.TXT"###, r###"case-preserved on Windows"###),
];
const GOLDEN: &[(&str, &str)] = &[
    (
        r###"assets/minecraft/lang/en_us.json"###,
        r###"{
  "greeting": "你好",
  "value": "a=b",
  "duplicate": "last"
}"###,
    ),
    (
        r###"assets/minecraft/lang/invalid.json"###,
        r###"not JSON"###,
    ),
    (
        r###"assets/minecraft/models/example.json"###,
        r###"{"textures": {"a": "minecraft:block/stone_bricks", "b": "block/stone", "item": "item/sword_diamond"}}"###,
    ),
    (
        r###"assets/minecraft/textures/block/custom.png"###,
        r###"PNG fixture: custom"###,
    ),
    (
        r###"assets/minecraft/textures/block/custom.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/block/dirt_path.png"###,
        r###"PNG fixture: grass_path"###,
    ),
    (
        r###"assets/minecraft/textures/block/grass_path.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/block/light_gray_wool.png"###,
        r###"PNG fixture: wool_colored_silver"###,
    ),
    (
        r###"assets/minecraft/textures/block/light_gray_wool.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/block/nested/stonebrick.png"###,
        r###"nested preserved"###,
    ),
    (
        r###"assets/minecraft/textures/block/oak_planks.png"###,
        r###"PNG fixture: planks_oak"###,
    ),
    (
        r###"assets/minecraft/textures/block/oak_planks.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/block/stone.png"###,
        r###"PNG fixture: stone"###,
    ),
    (
        r###"assets/minecraft/textures/block/stone.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/block/stone_bricks.png"###,
        r###"PNG fixture: stonebrick"###,
    ),
    (
        r###"assets/minecraft/textures/block/stone_bricks.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/block/tallgrass.png"###,
        r###"PNG fixture: tallgrass"###,
    ),
    (
        r###"assets/minecraft/textures/block/tallgrass.png.mcmeta"###,
        r###"{"animation":{"frametime":2}}"###,
    ),
    (
        r###"assets/minecraft/textures/entity/chest.png"###,
        r###"entity"###,
    ),
    (
        r###"assets/minecraft/textures/item/sword_diamond.png"###,
        r###"item"###,
    ),
    (
        r###"pack.mcmeta"###,
        r###"{
  "pack": {
    "description": "{\"text\": \"材质测试\"}",
    "min_format": [
      88,
      0
    ],
    "max_format": [
      88,
      0
    ],
    "pack_format": 88
  }
}"###,
    ),
    (r###"README.TXT"###, r###"case-preserved on Windows"###),
];
