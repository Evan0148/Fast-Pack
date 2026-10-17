use super::{Description, LEGACY_COLORS, display_color, legacy_index};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub underlined: bool,
    pub strikethrough: bool,
    pub obfuscated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Bold,
    Italic,
    Underlined,
    Strikethrough,
    Obfuscated,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bold => "加粗",
            Self::Italic => "斜体",
            Self::Underlined => "下划线",
            Self::Strikethrough => "删除线",
            Self::Obfuscated => "随机字符",
        }
    }
    fn code(self) -> char {
        match self {
            Self::Bold => 'l',
            Self::Italic => 'o',
            Self::Underlined => 'n',
            Self::Strikethrough => 'm',
            Self::Obfuscated => 'k',
        }
    }
}

pub const FORMATS: [Format; 5] = [
    Format::Bold,
    Format::Italic,
    Format::Underlined,
    Format::Strikethrough,
    Format::Obfuscated,
];

impl TextStyle {
    pub fn get(self, format: Format) -> bool {
        match format {
            Format::Bold => self.bold,
            Format::Italic => self.italic,
            Format::Underlined => self.underlined,
            Format::Strikethrough => self.strikethrough,
            Format::Obfuscated => self.obfuscated,
        }
    }
    pub fn set(&mut self, format: Format, value: bool) {
        *match format {
            Format::Bold => &mut self.bold,
            Format::Italic => &mut self.italic,
            Format::Underlined => &mut self.underlined,
            Format::Strikethrough => &mut self.strikethrough,
            Format::Obfuscated => &mut self.obfuscated,
        } = value;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub color: Option<[u8; 3]>,
    pub style: TextStyle,
}

impl Description {
    pub fn normalize_lines(&mut self) {
        if !self.text.contains('\r') {
            return;
        }
        let chars: Vec<_> = self.text.chars().collect();
        let mut normalized = Description::default();
        for (index, character) in chars.iter().copied().enumerate() {
            if character == '\r' && chars.get(index + 1) == Some(&'\n') {
                continue;
            }
            normalized
                .text
                .push(if character == '\r' { '\n' } else { character });
            normalized
                .colors
                .push(self.colors.get(index).copied().flatten());
            normalized
                .styles
                .push(self.styles.get(index).copied().unwrap_or_default());
        }
        *self = normalized;
    }

    pub fn formatted_runs(&self, version: &str) -> Vec<Run> {
        self.parse_runs(version).0
    }

    fn parse_runs(&self, version: &str) -> (Vec<Run>, Option<(Option<[u8; 3]>, TextStyle)>) {
        let characters: Vec<_> = self.text.chars().collect();
        let mut runs: Vec<Run> = Vec::new();
        let mut codes = None;
        let mut index = 0;
        while index < characters.len() {
            let color = self.colors.get(index).copied().flatten();
            let style = self.styles.get(index).copied().unwrap_or_default();
            if characters[index] == '§'
                && let Some(code) = characters.get(index + 1)
            {
                let code = code.to_ascii_lowercase();
                let mut state = codes.unwrap_or((color, style));
                let recognized = if let Some(value) = code.to_digit(16) {
                    state = (Some(LEGACY_COLORS[value as usize]), TextStyle::default());
                    true
                } else if code == 'r' {
                    state = (None, TextStyle::default());
                    true
                } else if let Some(format) =
                    FORMATS.into_iter().find(|format| format.code() == code)
                {
                    state.1.set(format, true);
                    true
                } else {
                    false
                };
                if recognized {
                    codes = Some(state);
                    index += 2;
                    continue;
                }
            }
            let (color, style) = codes.unwrap_or((color, style));
            let color = color.map(|color| display_color(color, version));
            if let Some(run) = runs
                .last_mut()
                .filter(|run| run.color == color && run.style == style)
            {
                run.text.push(characters[index]);
            } else {
                runs.push(Run {
                    text: characters[index].to_string(),
                    color,
                    style,
                });
            }
            index += 1;
        }
        (runs, codes)
    }

    /// Resolve pasted/typed native formatting into editable text and per-character styles.
    pub fn materialize_codes(&mut self) -> Option<(Option<[u8; 3]>, TextStyle)> {
        let (runs, state) = self.parse_runs("1.16");
        let state = state?;
        self.text.clear();
        self.colors.clear();
        self.styles.clear();
        for run in runs {
            let length = run.text.chars().count();
            self.text.push_str(&run.text);
            self.colors.extend(std::iter::repeat_n(run.color, length));
            self.styles.extend(std::iter::repeat_n(run.style, length));
        }
        Some(state)
    }

    pub fn format(&mut self, range: Range<usize>, format: Format, enabled: bool) {
        let length = self.text.chars().count();
        self.styles.resize(length, TextStyle::default());
        let start = range.start.min(length);
        let end = range.end.min(length).max(start);
        for style in &mut self.styles[start..end] {
            style.set(format, enabled);
        }
    }

    pub fn reset(&mut self, range: Range<usize>) {
        self.paint(range.clone(), None);
        let length = self.text.chars().count();
        self.styles.resize(length, TextStyle::default());
        let start = range.start.min(length);
        let end = range.end.min(length).max(start);
        self.styles[start..end].fill(TextStyle::default());
    }

    pub fn edit_styled(
        &mut self,
        text: String,
        anchor: usize,
        ink: [u8; 3],
        style: TextStyle,
    ) -> anyhow::Result<()> {
        let before: Vec<_> = self.text.chars().collect();
        let after: Vec<_> = text.chars().collect();
        anyhow::ensure!(
            after.len() <= super::DESCRIPTION_LIMIT,
            "文字最多 {} 个字符",
            super::DESCRIPTION_LIMIT
        );
        let prefix = before
            .iter()
            .zip(&after)
            .take(anchor)
            .take_while(|(a, b)| a == b)
            .count();
        let suffix = before[prefix..]
            .iter()
            .rev()
            .zip(after[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        self.colors.resize(before.len(), None);
        self.styles.resize(before.len(), TextStyle::default());
        let range = prefix..before.len() - suffix;
        let length = after.len() - prefix - suffix;
        self.colors.splice(range.clone(), vec![Some(ink); length]);
        self.styles.splice(range, vec![style; length]);
        self.text = text;
        Ok(())
    }

    pub(super) fn styled_component(&self, version: &str) -> Value {
        let runs = self.formatted_runs(version);
        if runs.is_empty() {
            return Value::String(String::new());
        }
        if !super::supports_hex(version) {
            return legacy(&runs, usize::MAX).into();
        }
        let extra: Vec<_> = runs
            .into_iter()
            .map(|run| {
                let mut component = json!({"text":run.text});
                if let Some(color) = run.color {
                    component["color"] = crate::compat::glint::color_hex(color).into();
                }
                for (key, value) in [
                    ("bold", run.style.bold),
                    ("italic", run.style.italic),
                    ("underlined", run.style.underlined),
                    ("strikethrough", run.style.strikethrough),
                    ("obfuscated", run.style.obfuscated),
                ] {
                    if value {
                        component[key] = Value::Bool(true);
                    }
                }
                component
            })
            .collect();
        json!({"text":"", "extra":extra})
    }
}

fn legacy(runs: &[Run], budget: usize) -> String {
    let mut output = String::new();
    let mut previous = (None, TextStyle::default());
    let mut units: usize = 0;
    'runs: for run in runs {
        let current = (run.color, run.style);
        let mut prefix = String::new();
        if current != previous {
            prefix.push('§');
            prefix.push(run.color.map_or('r', |color| {
                b"0123456789abcdef"[legacy_index(color)] as char
            }));
            for format in FORMATS {
                if run.style.get(format) {
                    prefix.push('§');
                    prefix.push(format.code());
                }
            }
        }
        for (index, character) in run.text.chars().enumerate() {
            let prefix = if index == 0 { prefix.as_str() } else { "" };
            let needed = prefix.encode_utf16().count() + character.len_utf16();
            if units.saturating_add(needed).saturating_add(2) > budget {
                break 'runs;
            }
            output.push_str(prefix);
            output.push(character);
            units += needed;
            previous = current;
        }
    }
    if previous != (None, TextStyle::default()) {
        output.push_str("§r");
    }
    output
}

/// Minecraft uses the filename as the list title; it supports legacy 16-color formatting here.
pub fn formatted_filename(name: &Description, version: &str, as_zip: bool) -> String {
    let mut runs = name.formatted_runs("1.8.9");
    let mut count = 0;
    for run in &mut runs {
        run.text = run
            .text
            .chars()
            .take(80usize.saturating_sub(count))
            .map(|c| {
                if c.is_control() || "<>:\"/\\|?*".contains(c) {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        count += run.text.chars().count();
    }
    if runs
        .iter()
        .all(|run| run.text.trim_matches([' ', '.']).is_empty())
    {
        runs = vec![Run {
            text: "覆盖包".into(),
            color: None,
            style: TextStyle::default(),
        }];
    }
    let prefix = format!("[{version}] ");
    let extension = if as_zip { ".zip" } else { "" };
    // Leave space for the extension, the collision suffix, and Windows' component limit.
    let budget = 240usize.saturating_sub(prefix.encode_utf16().count() + extension.len() + 8);
    format!(
        "{prefix}{}{extension}",
        legacy(&runs, budget).trim_end_matches([' ', '.'])
    )
}
