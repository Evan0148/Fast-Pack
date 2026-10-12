use anyhow::{Result, bail};
use std::collections::BTreeMap;

pub type Properties = BTreeMap<String, String>;

fn decode(value: &str) -> Result<String> {
    let mut result = Vec::<u16>::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            result.extend(c.to_string().encode_utf16());
            continue;
        }
        let escaped = match chars.next() {
            Some('u') => {
                let code: String = chars.by_ref().take(4).collect();
                if code.len() != 4 {
                    bail!("无效的 properties Unicode 转义");
                }
                result.push(u16::from_str_radix(&code, 16)?);
                continue;
            }
            Some('t') => '\t',
            Some('r') => '\r',
            Some('n') => '\n',
            Some('f') => '\u{c}',
            Some(c) => c,
            None => '\\',
        };
        result.extend(escaped.to_string().encode_utf16());
    }
    Ok(String::from_utf16(&result)?)
}

pub fn parse(bytes: &[u8]) -> Result<Properties> {
    let input = String::from_utf8(bytes.to_vec())
        .unwrap_or_else(|_| bytes.iter().map(|byte| char::from(*byte)).collect());
    let input = input.trim_start_matches('\u{feff}');
    let mut result = Properties::new();
    let mut logical = String::new();
    for line in input.lines() {
        let line = if logical.is_empty() {
            line
        } else {
            line.trim_start()
        };
        logical.push_str(line);
        let slashes = logical.chars().rev().take_while(|c| *c == '\\').count();
        if slashes % 2 == 1 {
            logical.pop();
            continue;
        }
        let line = logical.trim_start();
        if !line.is_empty() && !line.starts_with(['#', '!']) {
            let mut escaped = false;
            let mut split = None;
            for (offset, c) in line.char_indices() {
                if escaped {
                    escaped = false;
                    continue;
                }
                if c == '\\' {
                    escaped = true;
                    continue;
                }
                if c == '=' || c == ':' || c.is_ascii_whitespace() {
                    split = Some(offset);
                    break;
                }
            }
            let (key, value) = if let Some(index) = split {
                let key = &line[..index];
                let value = line[index..].trim_start();
                let value = value.strip_prefix(['=', ':']).unwrap_or(value).trim_start();
                (key, value)
            } else {
                (line, "")
            };
            result.insert(decode(key)?, decode(value)?);
        }
        logical.clear();
    }
    if !logical.trim().is_empty() {
        bail!("properties 续行缺少下一行");
    }
    Ok(result)
}

fn encode(value: &str, key: bool) -> String {
    let mut result = String::new();
    for (i, c) in value.chars().enumerate() {
        match c {
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '=' | ':' | '#' | '!' if key => {
                result.push('\\');
                result.push(c);
            }
            ' ' if key || i == 0 => result.push_str("\\ "),
            c if !c.is_ascii() => {
                for unit in c.to_string().encode_utf16() {
                    result.push_str(&format!("\\u{unit:04x}"));
                }
            }
            _ => result.push(c),
        }
    }
    result
}

pub fn write(values: &Properties) -> String {
    values
        .iter()
        .map(|(key, value)| format!("{}={}\n", encode(key, true), encode(value, false)))
        .collect()
}
