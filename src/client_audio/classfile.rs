use anyhow::{Result, bail, ensure};

const HOLDER: &str = "com/viaversion/viaversion/api/minecraft/Holder";
const SOUND: &str = "com/viaversion/viaversion/api/minecraft/SoundEvent";
const PROTOCOL: &str =
    "com.viaversion.viabackwards.protocol.v1_20_5to1_20_3.Protocol1_20_5To1_20_3";
pub const RULES: &[(i16, &str)] = &[
    (800, "item.mace.smash_air"),
    (801, "item.mace.smash_ground"),
    (802, "item.mace.smash_ground_heavy"),
    (1527, "entity.wind_charge.throw"),
    (1526, "entity.wind_charge.wind_burst"),
    (198, "entity.breeze.wind_burst"),
];

struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or_else(|| anyhow::anyhow!("类文件长度溢出"))?;
        let value = self
            .data
            .get(self.offset..end)
            .ok_or_else(|| anyhow::anyhow!("类文件被截断"))?;
        self.offset = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.bytes(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.bytes(4)?.try_into()?))
    }
}

fn u16(out: &mut Vec<u8>, value: u16) {
    out.extend(value.to_be_bytes());
}
fn u32(out: &mut Vec<u8>, value: u32) {
    out.extend(value.to_be_bytes());
}

#[derive(Default)]
struct Pool(Vec<Vec<u8>>);

impl Pool {
    fn read(reader: &mut Reader<'_>) -> Result<Self> {
        let count = reader.u16()?;
        let mut pool = Self(vec![vec![]]);
        while pool.0.len() < count as usize {
            let start = reader.offset;
            let tag = reader.u8()?;
            let length = match tag {
                1 => reader.u16()? as usize,
                3 | 4 | 9 | 10 | 11 | 12 | 17 | 18 => 4,
                5 | 6 => 8,
                7 | 8 | 16 | 19 | 20 => 2,
                15 => 3,
                _ => bail!("未知常量池类型 {tag}"),
            };
            reader.bytes(length)?;
            pool.0.push(reader.data[start..reader.offset].to_vec());
            if matches!(tag, 5 | 6) {
                pool.0.push(vec![]);
            }
        }
        ensure!(pool.0.len() == count as usize, "无效常量池");
        Ok(pool)
    }
    fn add(&mut self, value: Vec<u8>) -> u16 {
        if let Some(index) = self.0.iter().position(|entry| *entry == value) {
            return index as u16;
        }
        self.0.push(value);
        (self.0.len() - 1) as u16
    }
    fn text(&self, index: u16) -> Result<&str> {
        let value = self
            .0
            .get(index as usize)
            .ok_or_else(|| anyhow::anyhow!("常量索引越界"))?;
        ensure!(value.first() == Some(&1), "常量不是 UTF8");
        Ok(std::str::from_utf8(&value[3..])?)
    }
    fn utf(&mut self, value: &str) -> u16 {
        let mut entry = vec![1];
        u16(&mut entry, value.len() as u16);
        entry.extend(value.as_bytes());
        self.add(entry)
    }
    fn single(&mut self, tag: u8, value: u16) -> u16 {
        let mut entry = vec![tag];
        u16(&mut entry, value);
        self.add(entry)
    }
    fn class(&mut self, value: &str) -> u16 {
        let name = self.utf(value);
        self.single(7, name)
    }
    fn string(&mut self, value: &str) -> u16 {
        let name = self.utf(value);
        self.single(8, name)
    }
    fn reference(&mut self, tag: u8, class: &str, name: &str, desc: &str) -> u16 {
        let class = self.class(class);
        let name = self.utf(name);
        let desc = self.utf(desc);
        let mut pair = vec![12];
        u16(&mut pair, name);
        u16(&mut pair, desc);
        let pair = self.add(pair);
        let mut entry = vec![tag];
        u16(&mut entry, class);
        u16(&mut entry, pair);
        self.add(entry)
    }
    fn write(&self, output: &mut Vec<u8>) -> Result<()> {
        ensure!(self.0.len() < u16::MAX as usize, "常量池过大");
        u16(output, self.0.len() as u16);
        for entry in self.0.iter().skip(1) {
            output.extend(entry);
        }
        Ok(())
    }
}

struct Attribute {
    name: u16,
    data: Vec<u8>,
}
struct Member {
    access: u16,
    name: u16,
    desc: u16,
    attributes: Vec<Attribute>,
}

fn attributes(reader: &mut Reader<'_>) -> Result<Vec<Attribute>> {
    let count = reader.u16()?;
    (0..count)
        .map(|_| {
            let name = reader.u16()?;
            let length = reader.u32()? as usize;
            Ok(Attribute {
                name,
                data: reader.bytes(length)?.to_vec(),
            })
        })
        .collect()
}

fn members(reader: &mut Reader<'_>) -> Result<Vec<Member>> {
    let count = reader.u16()?;
    (0..count)
        .map(|_| {
            Ok(Member {
                access: reader.u16()?,
                name: reader.u16()?,
                desc: reader.u16()?,
                attributes: attributes(reader)?,
            })
        })
        .collect()
}

fn write_attributes(out: &mut Vec<u8>, values: &[Attribute]) {
    u16(out, values.len() as u16);
    for value in values {
        u16(out, value.name);
        u32(out, value.data.len() as u32);
        out.extend(&value.data);
    }
}

fn write_members(out: &mut Vec<u8>, values: &[Member]) {
    u16(out, values.len() as u16);
    for value in values {
        u16(out, value.access);
        u16(out, value.name);
        u16(out, value.desc);
        write_attributes(out, &value.attributes);
    }
}

struct Code {
    bytes: Vec<u8>,
    frames: Vec<(usize, Vec<u16>)>,
    branches: Vec<(usize, usize)>,
    labels: Vec<usize>,
}

impl Code {
    fn new() -> Self {
        Self {
            bytes: vec![],
            frames: vec![],
            branches: vec![],
            labels: vec![],
        }
    }
    fn op(&mut self, bytes: &[u8]) {
        self.bytes.extend(bytes);
    }
    fn reference(&mut self, opcode: u8, index: u16) {
        self.bytes.push(opcode);
        u16(&mut self.bytes, index);
    }
    fn interface(&mut self, index: u16, count: u8) {
        self.reference(0xb9, index);
        self.op(&[count, 0]);
    }
    fn label(&mut self) -> usize {
        self.labels.push(usize::MAX);
        self.labels.len() - 1
    }
    fn mark(&mut self, label: usize, locals: &[u16]) {
        self.labels[label] = self.bytes.len();
        if let Some(frame) = self
            .frames
            .last_mut()
            .filter(|frame| frame.0 == self.bytes.len())
        {
            frame.1 = locals.to_vec();
        } else {
            self.frames.push((self.bytes.len(), locals.to_vec()));
        }
    }
    fn branch(&mut self, opcode: u8, label: usize) {
        self.branches.push((self.bytes.len(), label));
        self.op(&[opcode, 0, 0]);
    }
    fn finish(mut self, pool: &mut Pool, max_stack: u16, locals: u16) -> Result<Vec<u8>> {
        for (offset, label) in self.branches {
            let target = self.labels[label];
            ensure!(target != usize::MAX, "跳转目标未定义");
            let delta = i16::try_from(target as isize - offset as isize)?;
            self.bytes[offset + 1..offset + 3].copy_from_slice(&delta.to_be_bytes());
        }
        self.frames.sort_by_key(|frame| frame.0);
        self.frames.dedup_by_key(|frame| frame.0);
        let mut frames = vec![];
        u16(&mut frames, self.frames.len() as u16);
        let mut previous = -1isize;
        for (offset, locals) in self.frames {
            frames.push(255);
            u16(&mut frames, u16::try_from(offset as isize - previous - 1)?);
            u16(&mut frames, locals.len() as u16);
            for local in locals {
                if local == 0 {
                    frames.push(1);
                } else {
                    frames.push(7);
                    u16(&mut frames, local);
                }
            }
            u16(&mut frames, 0);
            previous = offset as isize;
        }
        let mut output = vec![];
        u16(&mut output, max_stack);
        u16(&mut output, locals);
        u32(&mut output, self.bytes.len() as u32);
        output.extend(self.bytes);
        u16(&mut output, 0);
        write_attributes(
            &mut output,
            &[Attribute {
                name: pool.utf("StackMapTable"),
                data: frames,
            }],
        );
        Ok(output)
    }
}

fn original_sound(pool: &mut Pool) -> Result<Vec<u8>> {
    let object = pool.class("java/lang/Object");
    let holder = pool.class(HOLDER);
    let string = pool.class("java/lang/String");
    let event = pool.class(SOUND);
    let has_id = pool.reference(11, HOLDER, "hasId", "()Z");
    let id = pool.reference(11, HOLDER, "id", "()I");
    let get_class = pool.reference(10, "java/lang/Object", "getClass", "()Ljava/lang/Class;");
    let get_name = pool.reference(10, "java/lang/Class", "getName", "()Ljava/lang/String;");
    let equals = pool.reference(10, "java/lang/String", "equals", "(Ljava/lang/Object;)Z");
    let constructor = pool.reference(
        10,
        SOUND,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/Float;)V",
    );
    let holder_of = pool.reference(11, HOLDER, "of", &format!("(Ljava/lang/Object;)L{HOLDER};"));
    let value = pool.reference(11, HOLDER, "value", "()Ljava/lang/Object;");
    let identifier = pool.reference(10, SOUND, "identifier", "()Ljava/lang/String;");
    let rename = pool.reference(
        10,
        SOUND,
        "withIdentifier",
        &format!("(Ljava/lang/String;)L{SOUND};"),
    );
    let mut code = Code::new();
    let direct = code.label();
    let original = code.label();
    code.op(&[0x2b]);
    code.interface(has_id, 1);
    code.branch(0x99, direct);
    code.op(&[0x2a]);
    code.reference(0xb6, get_class);
    code.reference(0xb6, get_name);
    code.reference(0x13, pool.string(PROTOCOL));
    code.reference(0xb6, equals);
    code.branch(0x99, original);
    code.op(&[0x2b]);
    code.interface(id, 1);
    code.op(&[0x3d]);
    for (id, name) in RULES {
        let next = code.label();
        code.op(&[0x1c, 0x11]);
        code.bytes.extend(id.to_be_bytes());
        code.branch(0xa0, next);
        code.reference(0xbb, event);
        code.op(&[0x59]);
        code.reference(0x13, pool.string(&format!("fastpack:{name}")));
        code.op(&[0x01]);
        code.reference(0xb7, constructor);
        code.reference(0xb8, holder_of);
        code.op(&[0xb0]);
        code.mark(next, &[object, holder, 0]);
    }
    code.branch(0xa7, original);
    code.mark(direct, &[object, holder]);
    code.op(&[0x2b]);
    code.interface(value, 1);
    code.reference(0xc0, event);
    code.reference(0xb6, identifier);
    code.op(&[0x4d]);
    for (_, name) in RULES {
        for input in [name.to_string(), format!("minecraft:{name}")] {
            let next = code.label();
            code.op(&[0x2c]);
            code.reference(0x13, pool.string(&input));
            code.reference(0xb6, equals);
            code.branch(0x99, next);
            code.op(&[0x2b]);
            code.interface(value, 1);
            code.reference(0xc0, event);
            code.reference(0x13, pool.string(&format!("fastpack:{name}")));
            code.reference(0xb6, rename);
            code.reference(0xb8, holder_of);
            code.op(&[0xb0]);
            code.mark(next, &[object, holder, string]);
        }
    }
    code.mark(original, &[object, holder]);
    code.op(&[0x2b, 0xb0]);
    code.finish(pool, 4, 3)
}

fn shifted_frames(data: &[u8], shift: u16) -> Result<Vec<u8>> {
    let mut reader = Reader { data, offset: 0 };
    let count = reader.u16()?;
    let mut out = vec![];
    u16(&mut out, count);
    if count == 0 {
        return Ok(out);
    }
    let kind = reader.u8()?;
    match kind {
        0..=63 => {
            let delta = kind as u16 + shift;
            if delta < 64 {
                out.push(delta as u8);
            } else {
                out.push(251);
                u16(&mut out, delta);
            }
        }
        64..=127 => {
            let delta = kind as u16 - 64 + shift;
            if delta < 64 {
                out.push(delta as u8 + 64);
            } else {
                out.push(247);
                u16(&mut out, delta);
            }
        }
        247..=255 => {
            out.push(kind);
            u16(
                &mut out,
                reader
                    .u16()?
                    .checked_add(shift)
                    .ok_or_else(|| anyhow::anyhow!("栈帧偏移溢出"))?,
            );
        }
        _ => bail!("无效 StackMapTable"),
    }
    out.extend(&data[reader.offset..]);
    Ok(out)
}

fn hook(data: &[u8], pool: &Pool, field: u16, helper: u16) -> Result<Vec<u8>> {
    let mut reader = Reader { data, offset: 0 };
    let stack = reader.u16()?;
    let locals = reader.u16()?;
    let count = reader.u32()? as usize;
    let bytes = reader.bytes(count)?;
    ensure!(
        bytes.len() > 13
            && bytes[0] == 0x2b
            && bytes[1] == 0xb2
            && bytes[4] == 0xb9
            && bytes[9] == 0xc0
            && bytes[12] == 0x4d,
        "声音处理器不是已核对的 Holder 读取结构"
    );
    ensure!(reader.u16()? == 0, "声音处理器存在未支持的异常表");
    let nested = attributes(&mut reader)?;
    ensure!(reader.offset == data.len(), "Code 属性尾部异常");
    let mut inject = vec![0x2a, 0xb4];
    u16(&mut inject, field);
    inject.push(0x2c);
    inject.push(0xb8);
    u16(&mut inject, helper);
    inject.push(0x4d);
    let mut out = vec![];
    u16(&mut out, stack.max(2));
    u16(&mut out, locals);
    u32(&mut out, (bytes.len() + inject.len()) as u32);
    out.extend(&bytes[..13]);
    out.extend(&inject);
    out.extend(&bytes[13..]);
    u16(&mut out, 0);
    let mut frames = vec![];
    for attribute in nested {
        match pool.text(attribute.name)? {
            "StackMapTable" => frames.push(Attribute {
                name: attribute.name,
                data: shifted_frames(&attribute.data, inject.len() as u16)?,
            }),
            "LineNumberTable" | "LocalVariableTable" | "LocalVariableTypeTable" => {}
            name => bail!("未支持的 Code 属性 {name}"),
        }
    }
    ensure!(frames.len() == 1, "缺少唯一栈帧表");
    write_attributes(&mut out, &frames);
    Ok(out)
}

pub fn patch(input: &[u8], backwards: bool) -> Result<Vec<u8>> {
    let mut reader = Reader {
        data: input,
        offset: 0,
    };
    ensure!(reader.u32()? == 0xcafebabe, "不是 Java 类文件");
    reader.bytes(4)?;
    let header = &input[..8];
    let mut pool = Pool::read(&mut reader)?;
    let class_start = reader.offset;
    reader.bytes(6)?;
    let interfaces = reader.u16()?;
    reader.bytes(interfaces as usize * 2)?;
    let class_header = &input[class_start..reader.offset];
    let fields = members(&mut reader)?;
    let mut methods = members(&mut reader)?;
    let class_attributes = attributes(&mut reader)?;
    ensure!(reader.offset == input.len(), "类文件尾部异常");
    let class = if backwards {
        "com/viaversion/viabackwards/api/rewriters/SoundRewriter"
    } else {
        "com/viaversion/viaversion/rewriter/SoundRewriter"
    };
    let protocol = if backwards {
        "Lcom/viaversion/viabackwards/api/BackwardsProtocol;"
    } else {
        "Lcom/viaversion/viaversion/api/protocol/Protocol;"
    };
    ensure!(
        !methods
            .iter()
            .any(|m| pool.text(m.name).ok() == Some("fastpack$originalSound")),
        "声音处理器已有适配器"
    );
    ensure!(
        fields
            .iter()
            .any(|f| pool.text(f.name).ok() == Some("protocol")
                && pool.text(f.desc).ok() == Some(protocol)),
        "协议字段不匹配"
    );
    let field = pool.reference(9, class, "protocol", protocol);
    let desc = format!("(Ljava/lang/Object;L{HOLDER};)L{HOLDER};");
    let helper = pool.reference(10, class, "fastpack$originalSound", &desc);
    let mut changed = 0;
    for method in &mut methods {
        let name = pool.text(method.name)?;
        if (backwards && name.starts_with("lambda$getSoundHandler1_19_3$"))
            || (!backwards && name.starts_with("lambda$soundHolderHandler$"))
        {
            for attribute in &mut method.attributes {
                if pool.text(attribute.name)? == "Code" {
                    attribute.data = hook(&attribute.data, &pool, field, helper)?;
                    changed += 1;
                }
            }
        }
    }
    ensure!(changed == 1, "未找到唯一的声音处理方法");
    let data = original_sound(&mut pool)?;
    methods.push(Member {
        access: 0x000a,
        name: pool.utf("fastpack$originalSound"),
        desc: pool.utf(&desc),
        attributes: vec![Attribute {
            name: pool.utf("Code"),
            data,
        }],
    });
    let mut output = header.to_vec();
    pool.write(&mut output)?;
    output.extend(class_header);
    write_members(&mut output, &fields);
    write_members(&mut output, &methods);
    write_attributes(&mut output, &class_attributes);
    Ok(output)
}

const JSON: &str = "com/viaversion/viaversion/libs/gson/JsonElement";
const OBJECT: &str = "com/viaversion/viaversion/libs/gson/JsonObject";
const ARRAY: &str = "com/viaversion/viaversion/libs/gson/JsonArray";
const PRIMITIVE: &str = "com/viaversion/viaversion/libs/gson/JsonPrimitive";
const ITEM_REWRITER: &str = "com/viaversion/viabackwards/api/rewriters/BackwardsItemRewriter";
const TRANSLATABLE: &str = "com/viaversion/viabackwards/api/rewriters/text/TranslatableRewriter";
const CONNECTION: &str = "com/viaversion/viaversion/api/connection/UserConnection";

fn localize_string(pool: &mut Pool) -> Result<Vec<u8>> {
    let string = pool.class("java/lang/String");
    let replace = pool.reference(
        10,
        "java/lang/String",
        "replaceAll",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
    );
    let mut code = Code::new();
    let lore = code.label();
    let done = code.label();
    code.op(&[0x1b]);
    code.branch(0x99, lore);
    for (english, chinese) in [("1\\.21 Mace", "重锤"), ("1\\.21 Wind Charge", "风弹")] {
        code.op(&[0x2a]);
        code.reference(
            0x13,
            pool.string(&format!("(?i)^((?:§[0-9a-fk-or])*){english}$")),
        );
        code.reference(0x13, pool.string(&format!("$1{chinese}")));
        code.reference(0xb6, replace);
        code.op(&[0x4b]);
    }
    code.branch(0xa7, done);
    code.mark(lore, &[string, 0]);
    for (english, chinese) in [
        ("Wind Burst", "风爆"),
        ("Density", "致密"),
        ("Breach", "破甲"),
    ] {
        code.op(&[0x2a]);
        code.reference(0x13,pool.string(&format!("(?i)^((?:§[0-9a-fk-or])*)(?:1\\.21 )?{english}(?= *(?:[IVXLCDM]+|[0-9]+)?(?:§[0-9a-fk-or])*$)")));
        code.reference(0x13, pool.string(&format!("$1{chinese}")));
        code.reference(0xb6, replace);
        code.op(&[0x4b]);
    }
    code.mark(done, &[string, 0]);
    code.op(&[0x2a, 0xb0]);
    code.finish(pool, 3, 2)
}

fn localize_json(pool: &mut Pool) -> Result<Vec<u8>> {
    let json = pool.class(JSON);
    let array = pool.class(ARRAY);
    let object = pool.class(OBJECT);
    let primitive = pool.class(PRIMITIVE);
    let iterator = pool.class("java/util/Iterator");
    let entry = pool.class("java/util/Map$Entry");
    let recursive = pool.reference(
        10,
        ITEM_REWRITER,
        "fastpack$localizeComponent",
        &format!("(L{JSON};Z)L{JSON};"),
    );
    let primitive_test = pool.reference(10, JSON, "isJsonPrimitive", "()Z");
    let as_primitive = pool.reference(10, JSON, "getAsJsonPrimitive", &format!("()L{PRIMITIVE};"));
    let is_string = pool.reference(10, PRIMITIVE, "isString", "()Z");
    let get_string = pool.reference(10, JSON, "getAsString", "()Ljava/lang/String;");
    let translate = pool.reference(
        10,
        ITEM_REWRITER,
        "fastpack$localizeString",
        "(Ljava/lang/String;Z)Ljava/lang/String;",
    );
    let init = pool.reference(10, PRIMITIVE, "<init>", "(Ljava/lang/String;)V");
    let array_test = pool.reference(10, JSON, "isJsonArray", "()Z");
    let array_size = pool.reference(10, ARRAY, "size", "()I");
    let array_get = pool.reference(10, ARRAY, "get", &format!("(I)L{JSON};"));
    let array_set = pool.reference(10, ARRAY, "set", &format!("(IL{JSON};)L{JSON};"));
    let object_test = pool.reference(10, JSON, "isJsonObject", "()Z");
    let entries = pool.reference(10, OBJECT, "entrySet", "()Ljava/util/Set;");
    let iterate = pool.reference(11, "java/util/Set", "iterator", "()Ljava/util/Iterator;");
    let has_next = pool.reference(11, "java/util/Iterator", "hasNext", "()Z");
    let next = pool.reference(11, "java/util/Iterator", "next", "()Ljava/lang/Object;");
    let key = pool.reference(11, "java/util/Map$Entry", "getKey", "()Ljava/lang/Object;");
    let value = pool.reference(
        11,
        "java/util/Map$Entry",
        "getValue",
        "()Ljava/lang/Object;",
    );
    let set = pool.reference(
        11,
        "java/util/Map$Entry",
        "setValue",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
    );
    let equals = pool.reference(10, "java/lang/String", "equals", "(Ljava/lang/Object;)Z");
    let mut code = Code::new();
    let original = code.label();
    let check_array = code.label();
    let check_object = code.label();
    code.op(&[0x2a]);
    code.branch(0xc6, original);
    code.op(&[0x2a]);
    code.reference(0xb6, primitive_test);
    code.branch(0x99, check_array);
    code.op(&[0x2a]);
    code.reference(0xb6, as_primitive);
    code.reference(0xb6, is_string);
    code.branch(0x99, original);
    code.reference(0xbb, primitive);
    code.op(&[0x59, 0x2a]);
    code.reference(0xb6, get_string);
    code.op(&[0x1b]);
    code.reference(0xb8, translate);
    code.reference(0xb7, init);
    code.op(&[0xb0]);
    code.mark(check_array, &[json, 0]);
    code.op(&[0x2a]);
    code.reference(0xb6, array_test);
    code.branch(0x99, check_object);
    code.op(&[0x2a]);
    code.reference(0xc0, array);
    code.op(&[0x4d, 0x03, 0x3e]);
    let array_loop = code.label();
    code.mark(array_loop, &[json, 0, array, 0]);
    code.op(&[0x1d, 0x2c]);
    code.reference(0xb6, array_size);
    code.branch(0xa2, original);
    code.op(&[0x2c, 0x1d, 0x2c, 0x1d]);
    code.reference(0xb6, array_get);
    code.op(&[0x1b]);
    code.reference(0xb8, recursive);
    code.reference(0xb6, array_set);
    code.op(&[0x57, 0x84, 0x03, 0x01]);
    code.branch(0xa7, array_loop);
    code.mark(check_object, &[json, 0]);
    code.op(&[0x2a]);
    code.reference(0xb6, object_test);
    code.branch(0x99, original);
    code.op(&[0x2a]);
    code.reference(0xc0, object);
    code.reference(0xb6, entries);
    code.interface(iterate, 1);
    code.op(&[0x4d]);
    let object_loop = code.label();
    let handle = code.label();
    code.mark(object_loop, &[json, 0, iterator]);
    code.op(&[0x2c]);
    code.interface(has_next, 1);
    code.branch(0x99, original);
    code.op(&[0x2c]);
    code.interface(next, 1);
    code.reference(0xc0, entry);
    code.op(&[0x4e]);
    for name in ["text", "extra", "with"] {
        code.reference(0x13, pool.string(name));
        code.op(&[0x2d]);
        code.interface(key, 1);
        code.reference(0xb6, equals);
        code.branch(0x9a, handle);
    }
    code.branch(0xa7, object_loop);
    code.mark(handle, &[json, 0, iterator, entry]);
    code.op(&[0x2d, 0x2d]);
    code.interface(value, 1);
    code.reference(0xc0, json);
    code.op(&[0x1b]);
    code.reference(0xb8, recursive);
    code.interface(set, 2);
    code.op(&[0x57]);
    code.branch(0xa7, object_loop);
    code.mark(original, &[json, 0]);
    code.op(&[0x2a, 0xb0]);
    code.finish(pool, 4, 4)
}

pub fn patch_localization(input: &[u8]) -> Result<Vec<u8>> {
    let mut reader = Reader {
        data: input,
        offset: 0,
    };
    ensure!(reader.u32()? == 0xcafebabe, "不是 Java 类文件");
    reader.bytes(4)?;
    let header = &input[..8];
    let mut pool = Pool::read(&mut reader)?;
    let start = reader.offset;
    reader.bytes(6)?;
    let interfaces = reader.u16()?;
    reader.bytes(interfaces as usize * 2)?;
    let class_header = &input[start..reader.offset];
    let fields = members(&mut reader)?;
    let mut methods = members(&mut reader)?;
    let class_attributes = attributes(&mut reader)?;
    ensure!(reader.offset == input.len(), "类文件尾部异常");
    ensure!(
        !methods
            .iter()
            .any(|method| pool.text(method.name).ok() == Some("fastpack$localizeComponent")),
        "汉化适配器已存在"
    );
    let process = pool.reference(
        11,
        TRANSLATABLE,
        "processText",
        &format!("(L{CONNECTION};L{JSON};)V"),
    );
    let wrapper_desc = format!("(L{TRANSLATABLE};L{CONNECTION};L{JSON};)L{JSON};");
    let localize = pool.reference(
        10,
        ITEM_REWRITER,
        "fastpack$localizeComponent",
        &format!("(L{JSON};Z)L{JSON};"),
    );
    let mut changed = 0;
    for method in &mut methods {
        if pool.text(method.name)? != "handleItemToClient" {
            continue;
        }
        for attribute in &mut method.attributes {
            if pool.text(attribute.name)? != "Code" {
                continue;
            }
            let mut body = Reader {
                data: &attribute.data,
                offset: 0,
            };
            let _ = body.u16()?;
            let _ = body.u16()?;
            let size = body.u32()? as usize;
            let code = body.bytes(size)?;
            let needle = [0xb9, (process >> 8) as u8, process as u8, 3, 0];
            let hits: Vec<_> = code
                .windows(5)
                .enumerate()
                .filter_map(|(index, bytes)| (bytes == needle).then_some(index))
                .collect();
            ensure!(hits.len() == 2, "物品文本处理结构与已核对版本不同");
            let expected = [(113, 6u8), (225, 11u8)];
            for (index, (end, local)) in hits.into_iter().zip(expected) {
                ensure!(index + 5 == end, "物品名称或附魔处理位置不匹配");
                let name = if local == 6 {
                    "fastpack$processName"
                } else {
                    "fastpack$processLore"
                };
                let wrapper = pool.reference(10, ITEM_REWRITER, name, &wrapper_desc);
                attribute.data[8 + index..8 + index + 5].copy_from_slice(&[
                    0xb8,
                    (wrapper >> 8) as u8,
                    wrapper as u8,
                    0x3a,
                    local,
                ]);
                changed += 1;
            }
        }
    }
    ensure!(changed == 2, "未找到唯一的名称和附魔文本入口");
    let string_data = localize_string(&mut pool)?;
    let json_data = localize_json(&mut pool)?;
    methods.push(Member {
        access: 0x000a,
        name: pool.utf("fastpack$localizeString"),
        desc: pool.utf("(Ljava/lang/String;Z)Ljava/lang/String;"),
        attributes: vec![Attribute {
            name: pool.utf("Code"),
            data: string_data,
        }],
    });
    methods.push(Member {
        access: 0x000a,
        name: pool.utf("fastpack$localizeComponent"),
        desc: pool.utf(&format!("(L{JSON};Z)L{JSON};")),
        attributes: vec![Attribute {
            name: pool.utf("Code"),
            data: json_data,
        }],
    });
    for (name, mode) in [("fastpack$processName", 1), ("fastpack$processLore", 0)] {
        let mut code = Code::new();
        code.op(&[0x2a, 0x2b, 0x2c]);
        code.interface(process, 3);
        code.op(&[0x2c, 0x03 + mode]);
        code.reference(0xb8, localize);
        code.op(&[0xb0]);
        let data = code.finish(&mut pool, 3, 3)?;
        methods.push(Member {
            access: 0x000a,
            name: pool.utf(name),
            desc: pool.utf(&wrapper_desc),
            attributes: vec![Attribute {
                name: pool.utf("Code"),
                data,
            }],
        });
    }
    let mut output = header.to_vec();
    pool.write(&mut output)?;
    output.extend(class_header);
    write_members(&mut output, &fields);
    write_members(&mut output, &methods);
    write_attributes(&mut output, &class_attributes);
    Ok(output)
}
