include!("../src/client_audio/classfile.rs");

fn main() -> Result<()> {
    let directory = std::env::args_os()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("需要输出目录"))?;
    let mut pool = Pool(vec![vec![]]);
    let class = pool.class("FastPackAudioProbe");
    let object = pool.class("java/lang/Object");
    let class_type = pool.class("java/lang/Class");
    let holder = pool.class(HOLDER);
    let event = pool.class(SOUND);
    let protocol = pool.class(&PROTOCOL.replace('.', "/"));
    let unsafe_class = pool.class("sun/misc/Unsafe");
    let get_field = pool.reference(
        10,
        "java/lang/Class",
        "getDeclaredField",
        "(Ljava/lang/String;)Ljava/lang/reflect/Field;",
    );
    let field_access = pool.reference(10, "java/lang/reflect/Field", "setAccessible", "(Z)V");
    let field_get = pool.reference(
        10,
        "java/lang/reflect/Field",
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
    );
    let allocate = pool.reference(
        10,
        "sun/misc/Unsafe",
        "allocateInstance",
        "(Ljava/lang/Class;)Ljava/lang/Object;",
    );
    let lookup = pool.reference(
        10,
        "java/lang/Class",
        "getDeclaredMethod",
        "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;",
    );
    let accessible = pool.reference(10, "java/lang/reflect/Method", "setAccessible", "(Z)V");
    let invoke = pool.reference(
        10,
        "java/lang/reflect/Method",
        "invoke",
        "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
    );
    let output = pool.reference(9, "java/lang/System", "out", "Ljava/io/PrintStream;");
    let print = pool.reference(
        10,
        "java/io/PrintStream",
        "println",
        "(Ljava/lang/Object;)V",
    );
    let of_id = pool.reference(11, HOLDER, "of", &format!("(I)L{HOLDER};"));
    let of_value = pool.reference(11, HOLDER, "of", &format!("(Ljava/lang/Object;)L{HOLDER};"));
    let event_init = pool.reference(
        10,
        SOUND,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/Float;)V",
    );
    let mut code = Code::new();
    code.reference(0x13, unsafe_class);
    code.reference(0x13, pool.string("theUnsafe"));
    code.reference(0xb6, get_field);
    code.op(&[0x59, 0x04]);
    code.reference(0xb6, field_access);
    code.op(&[0x01]);
    code.reference(0xb6, field_get);
    code.reference(0xc0, unsafe_class);
    code.reference(0x13, protocol);
    code.reference(0xb6, allocate);
    code.op(&[0x4d]);
    for class in [
        "com/viaversion/viaversion/rewriter/SoundRewriter",
        "com/viaversion/viabackwards/api/rewriters/SoundRewriter",
    ] {
        code.reference(0x13, pool.class(class));
        code.reference(0x13, pool.string("fastpack$originalSound"));
        code.op(&[0x05]);
        code.reference(0xbd, class_type);
        code.op(&[0x59, 0x03]);
        code.reference(0x13, object);
        code.op(&[0x53, 0x59, 0x04]);
        code.reference(0x13, holder);
        code.op(&[0x53]);
        code.reference(0xb6, lookup);
        code.op(&[0x59, 0x04]);
        code.reference(0xb6, accessible);
        code.op(&[0x4c]);
        for (id, name) in RULES.iter().copied().chain([(3, "ordinary-event")]) {
            code.reference(0xb2, output);
            code.reference(
                0x13,
                pool.string(&format!("{class} numeric {id} -> {name}")),
            );
            code.reference(0xb6, print);
            code.reference(0xb2, output);
            code.op(&[0x2b, 0x01, 0x05]);
            code.reference(0xbd, object);
            code.op(&[0x59, 0x03, 0x2c, 0x53, 0x59, 0x04, 0x11]);
            code.bytes.extend(id.to_be_bytes());
            code.reference(0xb8, of_id);
            code.op(&[0x53]);
            code.reference(0xb6, invoke);
            code.reference(0xb6, print);
        }
        for (_, name) in RULES.iter().chain([(0, "item.firecharge.use")].iter()) {
            for prefix in ["minecraft:", ""] {
                code.reference(0xb2, output);
                code.reference(0x13, pool.string(&format!("{class} direct {prefix}{name}")));
                code.reference(0xb6, print);
                code.reference(0xb2, output);
                code.op(&[0x2b, 0x01, 0x05]);
                code.reference(0xbd, object);
                code.op(&[0x59, 0x03, 0x2c, 0x53, 0x59, 0x04]);
                code.reference(0xbb, event);
                code.op(&[0x59]);
                code.reference(0x13, pool.string(&format!("{prefix}{name}")));
                code.op(&[0x01]);
                code.reference(0xb7, event_init);
                code.reference(0xb8, of_value);
                code.op(&[0x53]);
                code.reference(0xb6, invoke);
                code.reference(0xb6, print);
            }
        }
        code.reference(0xb2, output);
        code.reference(
            0x13,
            pool.string(&format!("{class} unrelated protocol numeric 800")),
        );
        code.reference(0xb6, print);
        code.reference(0xb2, output);
        code.op(&[0x2b, 0x01, 0x05]);
        code.reference(0xbd, object);
        code.op(&[0x59, 0x03]);
        code.reference(0xbb, object);
        code.op(&[0x59]);
        let init = pool.reference(10, "java/lang/Object", "<init>", "()V");
        code.reference(0xb7, init);
        code.op(&[0x53, 0x59, 0x04, 0x11, 0x03, 0x20]);
        code.reference(0xb8, of_id);
        code.op(&[0x53]);
        code.reference(0xb6, invoke);
        code.reference(0xb6, print);
    }
    let protocol_name = PROTOCOL.replace('.', "/");
    let mapping_class = "com/viaversion/viaversion/api/data/MappingDataBase";
    let full = "com/viaversion/viaversion/api/data/FullMappingsBase";
    let intmap = "com/viaversion/viaversion/api/data/IntArrayMappings";
    let wrapper = "com/viaversion/viaversion/protocol/packet/PacketWrapperImpl";
    let packet = "com/viaversion/viaversion/api/protocol/packet/PacketWrapper";
    let backwards = "com/viaversion/viabackwards/api/rewriters/SoundRewriter";
    let types = "com/viaversion/viaversion/api/type/Types";
    let field_set = pool.reference(
        10,
        "java/lang/reflect/Field",
        "set",
        "(Ljava/lang/Object;Ljava/lang/Object;)V",
    );
    let mapping = pool.reference(
        9,
        &protocol_name,
        "MAPPINGS",
        "Lcom/viaversion/viabackwards/api/data/BackwardsMappingData;",
    );
    let empty = pool.reference(
        10,
        "java/util/Collections",
        "emptyList",
        "()Ljava/util/List;",
    );
    let int_of = pool.reference(10, intmap, "of", &format!("([II)L{intmap};"));
    let full_init = pool.reference(
        10,
        full,
        "<init>",
        "(Ljava/util/List;Ljava/util/List;Lcom/viaversion/viaversion/api/data/Mappings;)V",
    );
    code.reference(0x13, pool.class(mapping_class));
    code.reference(0x13, pool.string("soundMappings"));
    code.reference(0xb6, get_field);
    code.op(&[0x59, 0x04]);
    code.reference(0xb6, field_access);
    code.reference(0xb2, mapping);
    code.reference(0xbb, pool.class(full));
    code.op(&[0x59]);
    code.reference(0xb8, empty);
    code.reference(0xb8, empty);
    code.op(&[0x03, 0xbc, 10, 0x03]);
    code.reference(0xb8, int_of);
    code.reference(0xb7, full_init);
    code.reference(0xb6, field_set);
    let backwards_init = pool.reference(
        10,
        backwards,
        "<init>",
        "(Lcom/viaversion/viabackwards/api/BackwardsProtocol;)V",
    );
    code.reference(0xbb, pool.class(backwards));
    code.op(&[0x59, 0x2c]);
    code.reference(0xc0, pool.class(&protocol_name));
    code.reference(0xb7, backwards_init);
    code.op(&[0x4e]);
    let wrapper_init = pool.reference(
        10,
        wrapper,
        "<init>",
        "(ILio/netty/buffer/ByteBuf;Lcom/viaversion/viaversion/api/connection/UserConnection;)V",
    );
    let sound_type = pool.reference(
        9,
        types,
        "SOUND_EVENT",
        "Lcom/viaversion/viaversion/api/type/types/misc/HolderType;",
    );
    let write = pool.reference(
        11,
        packet,
        "write",
        "(Lcom/viaversion/viaversion/api/type/Type;Ljava/lang/Object;)V",
    );
    let reset = pool.reference(11, packet, "resetReader", "()V");
    let handler = pool.reference(
        10,
        backwards,
        "getSoundHandler1_19_3",
        "()Lcom/viaversion/viaversion/api/protocol/remapper/PacketHandler;",
    );
    let handle = pool.reference(
        11,
        "com/viaversion/viaversion/api/protocol/remapper/PacketHandler",
        "handle",
        &format!("(L{packet};)V"),
    );
    let get = pool.reference(
        11,
        packet,
        "get",
        "(Lcom/viaversion/viaversion/api/type/Type;I)Ljava/lang/Object;",
    );
    let cancelled = pool.reference(11, packet, "isCancelled", "()Z");
    let bool_value = pool.reference(10, "java/lang/Boolean", "valueOf", "(Z)Ljava/lang/Boolean;");
    for (id, name) in RULES {
        code.reference(0xb2, output);
        code.reference(0x13, pool.string(&format!("packet handler {id} -> {name}")));
        code.reference(0xb6, print);
        code.reference(0xbb, pool.class(wrapper));
        code.op(&[0x59, 0x03, 0x01, 0x01]);
        code.reference(0xb7, wrapper_init);
        code.op(&[0x3a, 4]);
        code.op(&[0x19, 4]);
        code.reference(0xb2, sound_type);
        code.op(&[0x11]);
        code.bytes.extend(id.to_be_bytes());
        code.reference(0xb8, of_id);
        code.interface(write, 3);
        code.op(&[0x19, 4]);
        code.interface(reset, 1);
        code.op(&[0x2d]);
        code.reference(0xb6, handler);
        code.op(&[0x19, 4]);
        code.interface(handle, 2);
        code.reference(0xb2, output);
        code.op(&[0x19, 4]);
        code.reference(0xb2, sound_type);
        code.op(&[0x03]);
        code.interface(get, 3);
        code.reference(0xb6, print);
        code.reference(0xb2, output);
        code.op(&[0x19, 4]);
        code.interface(cancelled, 1);
        code.reference(0xb8, bool_value);
        code.reference(0xb6, print);
    }
    code.op(&[0xb1]);
    let data = code.finish(&mut pool, 10, 5)?;
    let methods = vec![Member {
        access: 0x0009,
        name: pool.utf("main"),
        desc: pool.utf("([Ljava/lang/String;)V"),
        attributes: vec![Attribute {
            name: pool.utf("Code"),
            data,
        }],
    }];
    let mut output = vec![];
    u32(&mut output, 0xcafebabe);
    u16(&mut output, 0);
    u16(&mut output, 61);
    pool.write(&mut output)?;
    u16(&mut output, 0x0021);
    u16(&mut output, class);
    u16(&mut output, object);
    u16(&mut output, 0);
    write_members(&mut output, &[]);
    write_members(&mut output, &methods);
    write_attributes(&mut output, &[]);
    std::fs::create_dir_all(&directory)?;
    std::fs::write(
        std::path::Path::new(&directory).join("FastPackAudioProbe.class"),
        output,
    )?;
    Ok(())
}
