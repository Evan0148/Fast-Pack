use crate::packfs::{self as fs, PathExt, WalkDir};
use anyhow::Result;
use serde_json::Value;
use std::path::Path;

fn clean(bytes: &[u8]) -> Option<Vec<u8>> {
    let bom = bytes.starts_with(&[0xef, 0xbb, 0xbf]);
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    if !bom && !bytes.windows(2).any(|pair| matches!(pair, b"//" | b"/*")) {
        return None;
    }
    let mut output = bytes.to_vec();
    let (mut quoted, mut escaped, mut changed, mut i) = (false, false, bom, 0);
    while i < bytes.len() {
        let byte = bytes[i];
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            i += 1;
            continue;
        }
        if byte == b'"' {
            quoted = true;
            i += 1;
            continue;
        }
        if bytes.get(i..i + 2) == Some(b"//") {
            changed = true;
            while i < bytes.len() && !matches!(bytes[i], b'\r' | b'\n') {
                output[i] = b' ';
                i += 1;
            }
        } else if bytes.get(i..i + 2) == Some(b"/*") {
            changed = true;
            output[i] = b' ';
            output[i + 1] = b' ';
            i += 2;
            loop {
                if i + 1 >= bytes.len() {
                    return None;
                }
                if &bytes[i..i + 2] == b"*/" {
                    output[i] = b' ';
                    output[i + 1] = b' ';
                    i += 2;
                    break;
                }
                if !matches!(bytes[i], b'\r' | b'\n') {
                    output[i] = b' ';
                }
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    changed.then_some(output)
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Value> {
    Ok(serde_json::from_slice(
        &clean(bytes).unwrap_or_else(|| bytes.to_vec()),
    )?)
}

pub(crate) fn normalize(root: &Path) -> Result<Vec<String>> {
    if !root.join("assets").pack_is_dir() {
        return Ok(Vec::new());
    }
    let mut changed = 0;
    for entry in WalkDir::new(root.join("assets")).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_file()
            || entry
                .path()
                .extension()
                .is_none_or(|extension| extension != "json")
        {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if !["/models/", "/items/", "/blockstates/"]
            .iter()
            .any(|kind| relative.contains(kind))
        {
            continue;
        }
        let original = fs::read(entry.path())?;
        let Some(cleaned) = clean(&original) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&cleaned) else {
            continue;
        };
        if !value.is_object() {
            continue;
        }
        crate::compat::assets::backup(root, entry.path())?;
        fs::write_atomic(entry.path(), &serde_json::to_vec_pretty(&value)?)?;
        changed += 1;
    }
    Ok(if changed > 0 {
        vec![format!(
            "模型 JSON：规范化 {changed} 个带 BOM / 注释的文件，保留已有尺寸、几何与贴图引用再进行版本转换"
        )]
    } else {
        Vec::new()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comments_do_not_damage_urls_escaped_quotes_or_unicode() {
        let original=b"\xef\xbb\xbf{ /* header */ \"credit\":\"https://example.invalid/a//b\\\"/*keep*/\",//note\n\"display\":{\"gui\":{\"scale\":[0.5,0.5,0.5]}}}";
        let value: Value = serde_json::from_slice(&clean(original).unwrap()).unwrap();
        assert_eq!(value["credit"], "https://example.invalid/a//b\"/*keep*/");
        assert_eq!(value["display"]["gui"]["scale"][0], 0.5);
        assert!(clean(br#"{"url":"https://example.invalid"}"#).is_none());
        assert!(clean(b"{\"a\":1}/*unfinished").is_none());
    }
}
