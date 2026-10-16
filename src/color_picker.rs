use super::{App, Message, rgb_hsv};
use iced::widget::{button, checkbox, column, container, row, space, text};
use iced::{Alignment, Border, Color, Element, Length};

#[cfg(test)]
#[path = "color_picker_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ColorTarget {
    Glint,
    Crosshair,
    ItemOutline,
    FishingLine,
    BlockOutline,
}

impl ColorTarget {
    pub fn title(self) -> &'static str {
        match self {
            Self::Glint => "附魔闪烁颜色",
            Self::Crosshair => "准星颜色",
            Self::ItemOutline => "物品描边颜色",
            Self::FishingLine => "鱼竿线颜色",
            Self::BlockOutline => "瞄准方块边框颜色",
        }
    }
}

impl App {
    pub(super) fn color_for(&self, target: ColorTarget) -> [u8; 3] {
        match target {
            ColorTarget::Glint => self.active_glint_color(),
            ColorTarget::Crosshair => {
                if self.crosshair_drawing {
                    self.crosshair_pen_color
                } else {
                    self.crosshair_tint.unwrap_or([255; 3])
                }
            }
            ColorTarget::ItemOutline => self.item_editor.outline_color(),
            ColorTarget::FishingLine => self.fishing_line_color,
            ColorTarget::BlockOutline => self.block_outline_color,
        }
    }
    pub(super) fn picker_color(&self) -> [u8; 3] {
        self.color_for(self.color_target)
    }
    pub(super) fn open_color_picker(&mut self, target: ColorTarget) {
        self.commit_glint_color();
        self.color_target = target;
        let color = self.picker_color();
        self.glint_hsv = rgb_hsv(color, 0.0);
        self.glint_hex = fast_pack::compat::glint::color_hex(color);
        for (motion, channel) in self.glint_color_motion.iter_mut().zip(color) {
            motion.set(channel as f32 / 255.0, self.now, true);
        }
        self.glint_picker = true;
        self.glint_picker_motion.set(1.0, self.now, self.reduced);
        self.glint_preview_started = self.now;
    }
    pub(super) fn set_picker_color(&mut self, color: [u8; 3]) {
        match self.color_target {
            ColorTarget::Glint => self.set_glint_color(color),
            ColorTarget::Crosshair => {
                self.crosshair_pen_color = color;
                self.crosshair_color = fast_pack::compat::glint::color_hex(color);
                if !self.crosshair_drawing {
                    self.crosshair_tint = Some(color);
                }
                self.sync_crosshair_preview();
            }
            ColorTarget::ItemOutline => self.item_editor.set_outline_color(color),
            ColorTarget::FishingLine => {
                self.fishing_line_color = color;
                self.fishing_line = Some(color);
            }
            ColorTarget::BlockOutline => {
                self.block_outline_color = color;
                self.block_outline = Some(color);
            }
        }
        self.glint_hsv = rgb_hsv(color, self.glint_hsv[0]);
        self.glint_hex = fast_pack::compat::glint::color_hex(color);
        for (motion, channel) in self.glint_color_motion.iter_mut().zip(color) {
            motion.set(channel as f32 / 255.0, self.now, self.reduced);
        }
        self.glint_edited = Some(self.now);
    }
    pub(super) fn palette_button(
        &self,
        _id: u8,
        target: ColorTarget,
        enabled: bool,
    ) -> Element<'_, Message> {
        let color = self.color_for(target);
        let c = self.colors();
        let swatch = container(space().width(16).height(16)).style(move |_| {
            iced::widget::container::Style {
                background: Some(Color::from_rgb8(color[0], color[1], color[2]).into()),
                border: Border {
                    radius: 4.0.into(),
                    width: 1.0,
                    color: c.line,
                },
                ..Default::default()
            }
        });
        button(
            row![swatch, text("调色").size(13)]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .padding([8, 10])
        .on_press_maybe(enabled.then_some(Message::PickColor(target)))
        .style(move |_, status| iced::widget::button::Style {
            background: Some(c.field.into()),
            text_color: if enabled { c.foreground } else { c.muted },
            border: Border {
                radius: 8.0.into(),
                width: 1.0,
                color: if status == iced::widget::button::Status::Hovered {
                    c.accent
                } else {
                    c.line
                },
            },
            ..Default::default()
        })
        .into()
    }
    pub(super) fn line_color_controls(&self) -> Element<'_, Message> {
        let supported = fast_pack::pack_format_of(&self.target).is_ok_and(|format| format >= 7);
        let enabled = !self.busy() && supported;
        let fishing = row![
            checkbox(supported && self.fishing_line.is_some())
                .label("自定义鱼竿线颜色")
                .size(17)
                .text_size(13)
                .on_toggle_maybe(enabled.then_some(Message::FishingLine)),
            self.palette_button(94, ColorTarget::FishingLine, enabled)
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let outline = row![
            checkbox(supported && self.block_outline.is_some())
                .label("自定义瞄准方块边框颜色")
                .size(17)
                .text_size(13)
                .on_toggle_maybe(enabled.then_some(Message::BlockOutline)),
            self.palette_button(95, ColorTarget::BlockOutline, enabled)
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let mut controls = column![fishing, outline].spacing(10);
        controls = controls.push(self.label(if supported {
            "支持原版线条渲染；光影或客户端的渲染覆盖可能改变效果。"
        } else {
            "目标版本早于 1.17，普通材质包不能修改鱼线与方块选中边框颜色。"
        }));
        container(controls).width(Length::Fill).into()
    }
}
