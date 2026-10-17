use super::{App, ColorTarget, Description, Event, Format, Message, TextField, TextStyle};
use iced::advanced::text::highlighter;
use iced::widget::{self, button, column, container, row, space, text, text_editor, tooltip};
use iced::{Alignment, Border, Color, Element, Font};
use std::ops::Range;

const FORMATS: [Format; 5] = [
    Format::Bold,
    Format::Italic,
    Format::Underlined,
    Format::Strikethrough,
    Format::Obfuscated,
];
const COLOR_NAMES: [&str; 16] = [
    "黑色", "深蓝", "深绿", "湖蓝", "深红", "深紫", "金色", "浅灰", "深灰", "蓝色", "绿色", "青色",
    "红色", "粉色", "黄色", "白色",
];

fn small_button(c: super::super::Colors, status: button::Status) -> button::Style {
    button::Style {
        background: Some(c.panel.into()),
        text_color: if status == button::Status::Disabled {
            c.muted
        } else {
            c.foreground
        },
        border: Border {
            radius: 7.0.into(),
            width: 1.0,
            color: if status == button::Status::Hovered {
                c.accent
            } else {
                c.line
            },
        },
        ..Default::default()
    }
}

fn font(style: TextStyle) -> Font {
    Font {
        family: if style.obfuscated {
            iced::font::Family::Monospace
        } else {
            iced::font::Family::Name(crate::windows_compat::font_family())
        },
        weight: if style.bold {
            iced::font::Weight::Bold
        } else {
            iced::font::Weight::Normal
        },
        style: if style.italic {
            iced::font::Style::Italic
        } else {
            iced::font::Style::Normal
        },
        ..Font::DEFAULT
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Highlight {
    color: Option<[u8; 3]>,
    style: TextStyle,
}

#[derive(Clone, PartialEq)]
struct Settings(Vec<Vec<(Range<usize>, Highlight)>>);

struct Highlighter {
    settings: Settings,
    line: usize,
}

impl highlighter::Highlighter for Highlighter {
    type Settings = Settings;
    type Highlight = Highlight;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Highlight)>;
    fn new(settings: &Settings) -> Self {
        Self {
            settings: settings.clone(),
            line: 0,
        }
    }
    fn update(&mut self, settings: &Settings) {
        self.settings = settings.clone();
        self.line = 0;
    }
    fn change_line(&mut self, line: usize) {
        self.line = self.line.min(line);
    }
    fn current_line(&self) -> usize {
        self.line
    }
    fn highlight_line(&mut self, _: &str) -> Self::Iterator<'_> {
        let highlights = self.settings.0.get(self.line).cloned().unwrap_or_default();
        self.line += 1;
        highlights.into_iter()
    }
}

fn settings(document: &Description, version: &str) -> Settings {
    let mut lines: Vec<Vec<(Range<usize>, Highlight)>> = vec![Vec::new()];
    let mut column = 0;
    for (index, character) in document.text.chars().enumerate() {
        if character == '\n' {
            lines.push(Vec::new());
            column = 0;
            continue;
        }
        let style = document.styles.get(index).copied().unwrap_or_default();
        let color = document
            .colors
            .get(index)
            .copied()
            .flatten()
            .map(|rgb| fast_pack::overlay::display_color(rgb, version));
        let length = character.len_utf8();
        let highlight = Highlight { color, style };
        let line = lines.last_mut().unwrap();
        if let Some((range, previous)) = line
            .last_mut()
            .filter(|(_, previous)| *previous == highlight)
        {
            let _ = previous;
            range.end += length;
        } else {
            line.push((column..column + length, highlight));
        }
        column += length;
    }
    Settings(lines)
}

fn format_highlight(highlight: &Highlight, _: &iced::Theme) -> highlighter::Format<Font> {
    highlighter::Format {
        color: highlight.color.map(|[r, g, b]| Color::from_rgb8(r, g, b)),
        font: Some(font(highlight.style)),
    }
}

impl App {
    pub(super) fn overlay_text_field(&self, field: TextField) -> Element<'_, Message> {
        let c = self.colors();
        let enabled = !self.busy();
        let (label, document, content, ink, target, id, version) = match field {
            TextField::Name => (
                "覆盖包名称",
                self.overlay.name_document(),
                &self.overlay.name_content,
                self.overlay.name_ink,
                ColorTarget::PackName,
                "overlay-name",
                "1.8.9",
            ),
            TextField::Description => (
                "简介",
                self.overlay.description.clone(),
                &self.overlay.content,
                self.overlay.ink,
                ColorTarget::Description,
                "overlay-description",
                self.target.as_str(),
            ),
        };
        let mut editor = text_editor(content)
            .id(id)
            .height(if field == TextField::Name { 44 } else { 100 })
            .padding(12)
            .size(14)
            .font(font(TextStyle::default()))
            .highlight_with::<Highlighter>(settings(&document, version), format_highlight)
            .style(move |_, status| text_editor::Style {
                background: super::super::rgb(0x111820).into(),
                border: Border {
                    radius: 10.0.into(),
                    width: 1.0,
                    color: if matches!(status, text_editor::Status::Focused { .. }) {
                        c.accent
                    } else {
                        c.line
                    },
                },
                placeholder: Color::from_rgb8(160, 176, 194),
                value: if field == TextField::Name {
                    Color::WHITE
                } else {
                    Color::from_rgb8(170, 170, 170)
                },
                selection: super::super::mix(c.field, c.accent, 0.4),
            });
        if enabled {
            editor = editor.on_action(move |action| {
                Message::Overlay(match field {
                    TextField::Name => Event::NameEdit(action),
                    TextField::Description => Event::Description(action),
                })
            });
        }
        let mut colors = row![].spacing(5).align_y(Alignment::Center);
        for (index, rgb) in fast_pack::overlay::LEGACY_COLORS.into_iter().enumerate() {
            let selected = fast_pack::overlay::display_color(ink, version) == rgb;
            let swatch = button(space().width(18).height(18))
                .padding(2)
                .on_press_maybe(enabled.then_some(Message::Overlay(Event::Color(field, rgb))))
                .style(move |_, status| button::Style {
                    background: Some(Color::from_rgb8(rgb[0], rgb[1], rgb[2]).into()),
                    border: Border {
                        radius: 5.0.into(),
                        width: if selected { 2.0 } else { 1.0 },
                        color: if selected || status == button::Status::Hovered {
                            c.accent
                        } else {
                            c.line
                        },
                    },
                    ..Default::default()
                });
            colors = colors.push(tooltip(
                swatch,
                container(text(COLOR_NAMES[index]).size(12)).padding(7),
                tooltip::Position::Top,
            ));
        }
        {
            colors = colors.push(
                button(text("更多颜色").size(12))
                    .padding([6, 9])
                    .on_press_maybe(enabled.then_some(Message::PickColor(target)))
                    .style(move |_, status| small_button(c, status)),
            );
        }
        let mut styles = row![].spacing(6).align_y(Alignment::Center);
        for format in FORMATS {
            let selected = self.overlay.style_active(field, format);
            styles = styles.push(
                button(text(format.label()).size(12))
                    .padding([6, 9])
                    .on_press_maybe(
                        enabled.then_some(Message::Overlay(Event::Style(field, format))),
                    )
                    .style(move |_, status| button::Style {
                        background: Some(
                            if selected {
                                super::super::mix(c.panel, c.accent, 0.18)
                            } else if status == button::Status::Hovered {
                                c.field
                            } else {
                                c.panel
                            }
                            .into(),
                        ),
                        text_color: if selected { c.accent } else { c.foreground },
                        border: Border {
                            radius: 7.0.into(),
                            width: 1.0,
                            color: if selected { c.accent } else { c.line },
                        },
                        ..Default::default()
                    }),
            );
        }
        styles = styles.push(
            button(text("清除格式").size(12))
                .padding([6, 9])
                .on_press_maybe(enabled.then_some(Message::Overlay(Event::ClearFormat(field))))
                .style(move |_, status| small_button(c, status)),
        );
        column![
            self.title(label, 13),
            editor,
            colors,
            styles,
            self.label(if field == TextField::Name {
                "点色块修改整段，选中文字只改选中部分 · 名称使用原版 16 色"
            } else {
                "点色块修改整段，选中文字只改选中部分"
            })
        ]
        .spacing(8)
        .into()
    }

    pub(super) fn formatted_preview(
        &self,
        document: &Description,
        version: &str,
        size: u16,
        default: Color,
    ) -> Element<'static, Message> {
        let frame = if self.reduced {
            0
        } else {
            self.now
                .saturating_duration_since(self.rainbow_preview_started)
                .as_millis() as usize
                / 110
        };
        let spans: Vec<_> = document
            .formatted_runs(version)
            .into_iter()
            .map(|run| {
                let text = if run.style.obfuscated {
                    run.text
                        .chars()
                        .enumerate()
                        .map(|(index, c)| {
                            if c.is_whitespace() {
                                c
                            } else if c.is_ascii() {
                                b"ABCDEFGHJKMNPQRSTVWXYZ23456789"[(frame + index * 7) % 28] as char
                            } else {
                                ['口', '田', '由', '甲', '申', '电', '回', '目']
                                    [(frame + index * 3) % 8]
                            }
                        })
                        .collect()
                } else {
                    run.text
                };
                widget::span::<(), Font>(text)
                    .color(
                        run.color
                            .map_or(default, |[r, g, b]| Color::from_rgb8(r, g, b)),
                    )
                    .font(font(run.style))
                    .underline(run.style.underlined)
                    .strikethrough(run.style.strikethrough)
            })
            .collect();
        widget::rich_text(spans).size(u32::from(size)).into()
    }

    pub(crate) fn overlay_text_animation_active(&self) -> bool {
        self.overlay_mode
            && !self.reduced
            && !self.glint_picker
            && !self.learn_more
            && [
                self.overlay.name_document(),
                self.overlay.description.clone(),
            ]
            .iter()
            .any(|document| document.styles.iter().any(|style| style.obfuscated))
    }
}
