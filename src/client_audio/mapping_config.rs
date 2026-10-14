use anyhow::{Result, bail, ensure};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub enum Tag {
    String(String),
    Compound(BTreeMap<String, Tag>),
    Raw(u8, Vec<u8>),
}

struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl Reader<'_> {
    fn bytes(&mut self, count: usize) -> Result<&[u8]> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or_else(|| anyhow::anyhow!("NBT 长度溢出"))?;
        let value = self
            .data
            .get(self.offset..end)
            .ok_or_else(|| anyhow::anyhow!("NBT 被截断"))?;
        self.offset = end;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn text(&mut self) -> Result<String> {
        let count = u16::from_be_bytes(self.bytes(2)?.try_into()?) as usize;
        Ok(std::str::from_utf8(self.bytes(count)?)?.to_string())
    }
    fn count(&mut self) -> Result<usize> {
        let value = i32::from_be_bytes(self.bytes(4)?.try_into()?);
        ensure!((0..=16777216).contains(&value), "NBT 数量无效");
        Ok(value as usize)
    }
    fn tag(&mut self, kind: u8, depth: usize) -> Result<Tag> {
        ensure!(depth < 64, "NBT 嵌套过深");
        let start = self.offset;
        match kind {
            8 => return Ok(Tag::String(self.text()?)),
            10 => {
                let mut values = BTreeMap::new();
                loop {
                    let kind = self.byte()?;
                    if kind == 0 {
                        break;
                    }
                    let name = self.text()?;
                    ensure!(!values.contains_key(&name), "NBT 字段重复");
                    values.insert(name, self.tag(kind, depth + 1)?);
                }
                return Ok(Tag::Compound(values));
            }
            1 => {
                self.bytes(1)?;
            }
            2 => {
                self.bytes(2)?;
            }
            3 | 5 => {
                self.bytes(4)?;
            }
            4 | 6 => {
                self.bytes(8)?;
            }
            7 | 11 | 12 => {
                let count = self.count()?;
                let size = match kind {
                    7 => 1,
                    11 => 4,
                    _ => 8,
                };
                self.bytes(
                    count
                        .checked_mul(size)
                        .ok_or_else(|| anyhow::anyhow!("NBT 数组过大"))?,
                )?;
            }
            9 => {
                let element = self.byte()?;
                let count = self.count()?;
                for _ in 0..count {
                    self.tag(element, depth + 1)?;
                }
            }
            _ => bail!("不支持的 NBT 标签 {kind}"),
        }
        Ok(Tag::Raw(kind, self.data[start..self.offset].to_vec()))
    }
}

fn text(output: &mut Vec<u8>, value: &str) -> Result<()> {
    output.extend(u16::try_from(value.len())?.to_be_bytes());
    output.extend(value.as_bytes());
    Ok(())
}

fn write_tag(output: &mut Vec<u8>, name: &str, value: &Tag) -> Result<()> {
    output.push(match value {
        Tag::String(_) => 8,
        Tag::Compound(_) => 10,
        Tag::Raw(kind, _) => *kind,
    });
    text(output, name)?;
    match value {
        Tag::String(value) => text(output, value)?,
        Tag::Compound(values) => {
            for (name, value) in values {
                write_tag(output, name, value)?;
            }
            output.push(0);
        }
        Tag::Raw(_, bytes) => output.extend(bytes),
    }
    Ok(())
}

pub fn read(bytes: &[u8]) -> Result<BTreeMap<String, Tag>> {
    ensure!(bytes.len() <= 16777216, "映射文件过大");
    let mut reader = Reader {
        data: bytes,
        offset: 0,
    };
    ensure!(reader.byte()? == 10, "映射配置不是未压缩的 NBT 复合标签");
    let _ = reader.text()?;
    let Tag::Compound(values) = reader.tag(10, 0)? else {
        unreachable!()
    };
    ensure!(reader.offset == bytes.len(), "映射配置包含多余数据");
    Ok(values)
}

pub fn write(values: &BTreeMap<String, Tag>) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    write_tag(&mut bytes, "", &Tag::Compound(values.clone()))?;
    Ok(bytes)
}

pub fn official_names(original: &BTreeMap<String, Tag>) -> BTreeMap<String, Tag> {
    let mut changes = BTreeMap::new();
    for group in ["itemnames", "enchantmentnames"] {
        let Some(Tag::Compound(values)) = original.get(group) else {
            continue;
        };
        let mut names = BTreeMap::new();
        for (id, value) in values {
            let Tag::String(name) = value else {
                continue;
            };
            let clean = name.strip_prefix("1.21 ").unwrap_or(name);
            let chinese = match (group, clean) {
                ("itemnames", "Mace") => "重锤",
                ("itemnames", "Wind Charge") => "风弹",
                ("enchantmentnames", "Wind Burst") => "风爆",
                ("enchantmentnames", "Density") => "致密",
                ("enchantmentnames", "Breach") => "破甲",
                _ => continue,
            };
            names.insert(id.clone(), Tag::String(chinese.into()));
        }
        if !names.is_empty() {
            changes.insert(group.into(), Tag::Compound(names));
        }
    }
    changes
}

pub fn merge(
    existing: &mut BTreeMap<String, Tag>,
    changes: BTreeMap<String, Tag>,
) -> Result<usize> {
    let mut count = 0;
    for (group, change) in changes {
        let Tag::Compound(changes) = change else {
            unreachable!()
        };
        let current = existing
            .entry(group)
            .or_insert_with(|| Tag::Compound(BTreeMap::new()));
        let Tag::Compound(values) = current else {
            bail!("已有名称配置字段不是复合标签");
        };
        for (id, name) in changes {
            values.insert(id, name);
            count += 1;
        }
    }
    Ok(count)
}
