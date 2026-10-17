use super::{App, ColorTarget, Message, WorkEvent};
use crate::motion::Tween;
use fast_pack::overlay::{self, Description};
use iced::widget::{
    self, checkbox, column, container, pick_list, row, space, text, text_editor, text_input,
};
use iced::{Alignment, Border, Color, Element, Length, Task};
use std::ops::Range;
use std::path::PathBuf;
use std::time::Instant;

#[cfg(test)]
#[path = "overlay_editor_tests.rs"]
mod tests;

#[derive(Clone, Debug)]
pub(super) enum Event {
    Name(String),
    Description(text_editor::Action),
    PaintAll,
    ClearColor,
    ChooseIcon,
    IconPicked(Option<PathBuf>),
    ResetIcon,
    Zip(bool),
    OpenOutput,
}

pub(super) struct Composer {
    pub name: String,
    pub content: text_editor::Content,
    pub description: Description,
    pub ink: [u8; 3],
    pub icon: Option<Vec<u8>>,
    pub icon_preview: widget::image::Handle,
    pub icon_revision: u64,
    pub icon_pending: bool,
    pub as_zip: bool,
    pub running: bool,
    pub percent: u8,
    pub progress: Tween,
    pub step: String,
    pub result: Option<overlay::Report>,
    pub error: Option<String>,
    pub edited: Option<Instant>,
}

impl Composer {
    pub fn new() -> Self {
        let text = "我的自定义覆盖包";
        Self {
            name: "我的覆盖包".into(),
            content: text_editor::Content::with_text(text),
            description: Description::plain(text),
            ink: [170; 3],
            icon: None,
            icon_preview: default_icon(),
            icon_revision: 0,
            icon_pending: false,
            as_zip: true,
            running: false,
            percent: 0,
            progress: Tween::new(0.0, 180),
            step: String::new(),
            result: None,
            error: None,
            edited: None,
        }
    }

    pub fn from_draft(draft: overlay::Draft) -> Self {
        let mut composer = Self::new();
        composer.name = draft.name.chars().take(80).collect();
        composer.description = draft.description;
        composer.description.text = composer
            .description
            .text
            .chars()
            .take(overlay::DESCRIPTION_LIMIT)
            .collect();
        composer
            .description
            .colors
            .resize(composer.description.text.chars().count(), None);
        composer.content = text_editor::Content::with_text(&composer.description.text);
        composer.ink = draft.ink;
        composer.as_zip = draft.as_zip;
        if let Some(icon) = draft
            .icon
            .and_then(|bytes| overlay::prepare_icon(&bytes).ok())
        {
            composer.icon_preview = widget::image::Handle::from_bytes(icon.clone());
            composer.icon = Some(icon);
        }
        composer
    }

    pub fn draft(&self) -> overlay::Draft {
        overlay::Draft {
            name: self.name.clone(),
            description: self.description.clone(),
            icon: self.icon.clone(),
            ink: self.ink,
            as_zip: self.as_zip,
        }
    }

    pub fn selection(&self) -> Option<Range<usize>> {
        let cursor = self.content.cursor();
        let start = offset(&self.description.text, cursor.position);
        let end = offset(&self.description.text, cursor.selection?);
        (start != end).then_some(start.min(end)..start.max(end))
    }

    pub fn set_color(&mut self, color: [u8; 3]) {
        self.ink = color;
        if let Some(selection) = self.selection() {
            self.description.paint(selection, Some(color));
        }
    }

    fn edit(&mut self, action: text_editor::Action) {
        let editing = action.is_edit();
        let cursor = self.content.cursor();
        let before = offset(&self.description.text, cursor.position);
        let anchor = self.selection().map_or(before, |selection| selection.start);
        self.content.perform(action);
        if editing {
            let after = self.content.text();
            let new_position = offset(&after, self.content.cursor().position);
            if let Err(error) = self
                .description
                .edit(after, anchor.min(new_position), self.ink)
            {
                self.content = text_editor::Content::with_text(&self.description.text);
                self.content.move_to(cursor);
                self.error = Some(error.to_string());
            } else {
                self.error = None;
            }
        }
    }
}

// Iced's columns are UTF-8 byte indices. Colors are indexed by Unicode scalars.
fn offset(text: &str, position: iced::advanced::text::editor::Position) -> usize {
    let mut count = 0;
    for (index, line) in text.split('\n').enumerate() {
        if index == position.line {
            let mut column = position.column.min(line.len());
            while !line.is_char_boundary(column) {
                column -= 1;
            }
            return count + line[..column].chars().count();
        }
        count += line.chars().count() + 1;
    }
    text.chars().count()
}

fn default_icon() -> widget::image::Handle {
    widget::image::Handle::from_bytes(include_bytes!("../assets/app.png").as_slice())
}

impl App {
    pub(super) fn customization_source(&self) -> Option<PathBuf> {
        if self.overlay_mode {
            None
        } else {
            self.source.clone()
        }
    }

    pub(super) fn overlay_update(&mut self, event: Event) -> Task<Message> {
        if self.busy() {
            return Task::none();
        }
        let changed = match &event {
            Event::Description(action) => action.is_edit(),
            Event::Name(_)
            | Event::PaintAll
            | Event::ClearColor
            | Event::ResetIcon
            | Event::Zip(_) => true,
            _ => false,
        };
        match event {
            Event::Name(name) => self.overlay.name = name.chars().take(80).collect(),
            Event::Description(action) => self.overlay.edit(action),
            Event::PaintAll => {
                let length = self.overlay.description.text.chars().count();
                self.overlay
                    .description
                    .paint(0..length, Some(self.overlay.ink));
            }
            Event::ClearColor => {
                let range = self
                    .overlay
                    .selection()
                    .unwrap_or(0..self.overlay.description.text.chars().count());
                self.overlay.description.paint(range, None);
            }
            Event::ChooseIcon => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("选择覆盖包图标")
                            .add_filter("图片", &["png", "jpg", "jpeg"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_owned())
                    },
                    |path| Message::Overlay(Event::IconPicked(path)),
                );
            }
            Event::IconPicked(Some(path)) => {
                self.overlay.icon_revision += 1;
                self.overlay.icon_pending = true;
                let revision = self.overlay.icon_revision;
                let sender = self.sender.clone();
                std::thread::spawn(move || {
                    let result = (|| -> anyhow::Result<_> {
                        anyhow::ensure!(
                            std::fs::metadata(&path)?.len() <= 16 * 1024 * 1024,
                            "图标文件不能超过 16 MB"
                        );
                        overlay::prepare_icon(&std::fs::read(path)?)
                    })()
                    .map_err(|error| format!("{error:#}"));
                    let _ = sender.send(WorkEvent::OverlayIcon(revision, result));
                });
            }
            Event::ResetIcon => {
                self.overlay.icon_revision += 1;
                self.overlay.icon_pending = false;
                self.overlay.icon = None;
                self.overlay.icon_preview = default_icon();
            }
            Event::Zip(zip) => self.overlay.as_zip = zip,
            Event::OpenOutput => {
                if let Some(result) = &self.overlay.result
                    && let Some(parent) = result.output.parent()
                {
                    let _ = std::process::Command::new("explorer.exe")
                        .arg(parent)
                        .spawn();
                }
            }
            Event::IconPicked(None) => {}
        }
        if changed {
            self.overlay.edited = Some(self.now);
        }
        Task::none()
    }

    pub(super) fn overlay_destination(&self) -> anyhow::Result<PathBuf> {
        let directory = self
            .parsed_export_directory()?
            .ok_or_else(|| anyhow::anyhow!("请先选择导出目录"))?;
        anyhow::ensure!(!self.overlay.name.trim().is_empty(), "请填写覆盖包名称");
        let name = overlay::filename(&self.overlay.name, &self.target, self.overlay.as_zip);
        let first = directory.join(&name);
        if !first.exists() {
            return Ok(first);
        }
        let stem = if self.overlay.as_zip {
            name.strip_suffix(".zip").unwrap()
        } else {
            &name
        };
        for index in 2..10000 {
            let path = directory.join(format!(
                "{stem} ({index}){}",
                if self.overlay.as_zip { ".zip" } else { "" }
            ));
            if !path.exists() {
                return Ok(path);
            }
        }
        anyhow::bail!("同名覆盖包过多，请更换名称")
    }

    pub(super) fn start_overlay(&mut self) {
        if self.busy() {
            return;
        }
        self.close_glint_picker();
        let request = (|| -> anyhow::Result<_> {
            anyhow::ensure!(!self.overlay.icon_pending, "图标正在处理，请稍候");
            let format = fast_pack::pack_format_of(&self.target)?;
            let destination = self.overlay_destination()?;
            let crosshair = if self.custom_crosshair {
                anyhow::ensure!(!self.crosshair_pending, "准星预设正在处理，请稍候");
                Some(
                    self.selected_crosshair()
                        .ok_or_else(|| anyhow::anyhow!("请先选择准星预设，或在画布上绘制准星"))?,
                )
            } else {
                None
            };
            let options = overlay::Options {
                version: self.target.clone(),
                description: self.overlay.description.clone(),
                icon: self.overlay.icon.clone(),
                as_zip: self.overlay.as_zip,
                glint_color: self.custom_glint.then_some(self.glint_color),
                fishing_line_color: (format >= 7).then_some(self.fishing_line).flatten(),
                block_outline_color: (format >= 7).then_some(self.block_outline).flatten(),
                crosshair,
                show_attack_indicator: self.show_attack_indicator,
                custom_items: self.item_editor.applied(&self.target),
                rainbow: fast_pack::compat::rainbow::Settings {
                    glint: self.custom_glint && self.rainbow.glint,
                    crosshair: self.custom_crosshair && self.rainbow.crosshair,
                    fishing_line: format >= 7
                        && self.fishing_line.is_some()
                        && self.rainbow.fishing_line,
                    block_outline: format >= 7
                        && self.block_outline.is_some()
                        && self.rainbow.block_outline,
                },
            };
            Ok((destination, options))
        })();
        let (destination, options) = match request {
            Ok(request) => request,
            Err(error) => {
                self.overlay.error = Some(error.to_string());
                return;
            }
        };
        self.directory_edited = None;
        self.remember_export_directory();
        self.overlay.running = true;
        self.overlay.error = None;
        self.overlay.result = None;
        self.overlay.percent = 0;
        self.overlay.progress.set(0.0, self.now, true);
        self.overlay.step = "正在准备覆盖包".into();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = overlay::build(&destination, &options, |step, value| {
                let _ = sender.send(WorkEvent::OverlayProgress(step.into(), value));
            })
            .map_err(|error| format!("{error:#}"));
            let _ = sender.send(WorkEvent::OverlayDone(result));
        });
    }

    pub(super) fn overlay_body(&self) -> widget::Column<'_, Message> {
        let c = self.colors();
        let composer = &self.overlay;
        let enabled = !self.busy();
        let mut name = text_input("覆盖包名称", &composer.name)
            .padding(11)
            .size(14)
            .style(|_, status| self.input_style(status));
        if enabled {
            name = name.on_input(|value| Message::Overlay(Event::Name(value)));
        }
        let mut description = text_editor(&composer.content)
            .id("overlay-description")
            .placeholder("输入简介，选中文字后调色…")
            .height(100)
            .padding(12)
            .size(14)
            .font(iced::Font::with_name(crate::windows_compat::font_family()))
            .style(move |_, status| text_editor::Style {
                background: c.field.into(),
                border: Border {
                    radius: 10.0.into(),
                    width: 1.0,
                    color: if matches!(status, text_editor::Status::Focused { .. }) {
                        c.accent
                    } else {
                        c.line
                    },
                },
                placeholder: c.muted,
                value: c.foreground,
                selection: super::mix(c.field, c.accent, 0.4),
            });
        if enabled {
            description =
                description.on_action(|action| Message::Overlay(Event::Description(action)));
        }
        let editor = column![
            self.label("覆盖包名称"),
            name,
            self.label("简介"),
            description,
            row![
                self.palette_button(113, ColorTarget::Description, enabled),
                self.action(
                    114,
                    "整段应用",
                    enabled.then_some(Message::Overlay(Event::PaintAll)),
                    false
                ),
                self.action(
                    115,
                    "清除颜色",
                    enabled.then_some(Message::Overlay(Event::ClearColor)),
                    false
                )
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            self.label(if overlay::supports_hex(&self.target) {
                "选中文字后调色；未选择文字时，颜色用于后续输入。"
            } else {
                "选中文字后调色；此版本的简介使用游戏原版 16 色。"
            })
        ]
        .spacing(10);
        let spans: Vec<_> = composer
            .description
            .runs(&self.target)
            .into_iter()
            .map(|(value, color)| {
                widget::span::<(), iced::Font>(value).color(
                    color.map_or(Color::from_rgb8(170, 170, 170), |[r, g, b]| {
                        Color::from_rgb8(r, g, b)
                    }),
                )
            })
            .collect();
        let preview_text: Element<'_, Message> = widget::rich_text(spans).size(14).into();
        let preview = container(
            row![
                widget::image(composer.icon_preview.clone())
                    .width(54)
                    .height(54),
                column![
                    text(if composer.name.is_empty() {
                        "覆盖包名称"
                    } else {
                        &composer.name
                    })
                    .size(16)
                    .color(Color::WHITE),
                    widget::scrollable(preview_text).height(64)
                ]
                .spacing(6)
                .width(Length::Fill)
            ]
            .spacing(12)
            .align_y(Alignment::Start),
        )
        .width(Length::Fill)
        .padding(16)
        .style(move |_| container::Style {
            background: Some(super::rgb(0x11151a).into()),
            border: Border {
                radius: 12.0.into(),
                width: 1.0,
                color: c.line,
            },
            ..Default::default()
        });
        let preview = column![
            self.label("游戏内预览"),
            preview,
            row![
                self.action(
                    116,
                    if composer.icon_pending {
                        "正在处理…"
                    } else {
                        "选择图标"
                    },
                    enabled.then_some(Message::Overlay(Event::ChooseIcon)),
                    false
                ),
                self.action(
                    117,
                    "默认图标",
                    (enabled && composer.icon.is_some())
                        .then_some(Message::Overlay(Event::ResetIcon)),
                    false
                )
            ]
            .spacing(8),
            self.label("PNG / JPG · 自动居中适配方形图标"),
            self.label(format!(
                "Minecraft {} · {} 个字符",
                self.target,
                composer.description.text.chars().count()
            ))
        ]
        .spacing(12);
        let metadata: Element<'_, Message> = if self.width < 940.0 {
            column![editor, preview].spacing(22).into()
        } else {
            row![
                container(editor).width(Length::FillPortion(3)),
                container(preview).width(Length::FillPortion(2))
            ]
            .spacing(26)
            .into()
        };
        let versions: Vec<_> = fast_pack::tables()
            .pack_formats
            .iter()
            .map(|(version, _)| version.clone())
            .collect();
        let mut directory = text_input("选择覆盖包的保存文件夹", &self.export_directory_input)
            .padding(11)
            .size(13)
            .style(|_, status| self.input_style(status));
        if enabled {
            directory = directory
                .on_input(Message::Output)
                .on_submit(Message::CommitExportDirectory);
        }
        let version = pick_list(versions, Some(self.target.clone()), Message::Target)
            .padding(11)
            .width(Length::Fill)
            .style(|_, status| self.picker_style(status, 40));
        let settings = row![
            column![self.label("游戏版本"), version]
                .spacing(8)
                .width(190),
            column![
                self.label("导出目录"),
                row![
                    directory,
                    self.action(118, "选择", enabled.then_some(Message::BrowseOutput), false)
                ]
                .spacing(8)
            ]
            .spacing(8)
            .width(Length::Fill),
            checkbox(composer.as_zip)
                .label("ZIP 格式")
                .size(17)
                .text_size(13)
                .on_toggle_maybe(enabled.then_some(|value| Message::Overlay(Event::Zip(value))))
        ]
        .spacing(18)
        .align_y(Alignment::End);
        let filename = overlay::filename(&composer.name, &self.target, composer.as_zip);
        let format = fast_pack::pack_format_of(&self.target).unwrap_or(0);
        let features = usize::from(self.custom_glint)
            + usize::from(self.custom_crosshair)
            + usize::from(format >= 7 && self.fishing_line.is_some())
            + usize::from(format >= 7 && self.block_outline.is_some())
            + self.item_editor.applied_count(&self.target)
            + usize::from(format >= 2 && !self.show_attack_indicator);
        let mut body = column![
            column![
                self.title("覆盖包制作", 28),
                self.label("选择版本和功能，直接制作。游戏内将覆盖包放在其他材质包上方。")
            ]
            .spacing(8),
            self.panel(column![self.title("图标与简介", 15), metadata].spacing(18)),
            self.panel(
                column![
                    self.title("制作设置", 15),
                    settings,
                    self.label(format!("导出文件：{filename}")),
                    self.memory_controls()
                ]
                .spacing(15)
            ),
        ]
        .spacing(18);
        if let Some(error) = &composer.error {
            body = body.push(self.panel(text(error).size(14).color(c.danger)));
        }
        if let Some(result) = &composer.result {
            let mut result_view = column![
                row![
                    text("覆盖包已保存").size(17).color(c.success),
                    space().width(Length::Fill),
                    self.action(
                        119,
                        "打开导出目录",
                        Some(Message::Overlay(Event::OpenOutput)),
                        false
                    )
                ]
                .align_y(Alignment::Center),
                self.label(result.output.display().to_string()),
                self.label(format!("{} 个文件 · 原有材质包未改动", result.files))
            ]
            .spacing(10);
            for warning in &result.assets.warnings {
                result_view = result_view.push(self.label(warning));
            }
            body = body.push(self.panel(result_view));
        }
        body.push(self.panel(column![
            row![self.title("覆盖内容", 15), space().width(Length::Fill), self.label(format!("已选 {features} 项"))].align_y(Alignment::Center),
            self.glint_controls(), self.line_color_controls(), self.crosshair_controls(), self.items_view(),
            self.label("物品自定义基于所选版本的原版资源制作。未勾选的功能不会写入覆盖包。"),
            if fast_pack::pack_format_of(&self.target).is_ok_and(|format| format < 18) {
                self.label("1.20.1 及更早版本的准星与 HUD 共用图集，制作准星时会同时使用原版 HUD 底图。")
            } else { self.label("此版本使用独立准星贴图，可保留下层材质包的其他 HUD 图标。") }
        ].spacing(16)))
    }
}
