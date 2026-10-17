use super::{App, Message, rgb_hsv};
use iced::widget::{checkbox, column, container, row, space};
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
    Description,
}

impl ColorTarget {
    pub fn title(self) -> &'static str {
        match self {
            Self::Glint => "附魔闪烁颜色",
            Self::Crosshair => "准星颜色",
            Self::ItemOutline => "物品描边颜色",
            Self::FishingLine => "鱼竿线颜色",
            Self::BlockOutline => "瞄准方块边框颜色",
            Self::Description => "简介文字颜色",
        }
    }
}

impl App {
    pub(super) fn rainbow_for(&self, target: ColorTarget) -> bool {
        match target {
            ColorTarget::Glint => self.rainbow.glint,
            ColorTarget::Crosshair => self.rainbow.crosshair,
            ColorTarget::ItemOutline => self.item_editor.outline_rainbow(),
            ColorTarget::FishingLine => self.rainbow.fishing_line,
            ColorTarget::BlockOutline => self.rainbow.block_outline,
            ColorTarget::Description => false,
        }
    }
    fn palette_rainbow(&self, target: ColorTarget) -> bool {
        self.rainbow_for(target)
            && match target {
                ColorTarget::Glint => self.custom_glint,
                ColorTarget::Crosshair => self.custom_crosshair,
                ColorTarget::ItemOutline => self.item_editor.outline_palette_visible(),
                ColorTarget::FishingLine => self.fishing_line.is_some(),
                ColorTarget::BlockOutline => self.block_outline.is_some(),
                ColorTarget::Description => false,
            }
    }
    pub(super) fn rainbow_preview_color(&self) -> [u8; 3] {
        let phase = if self.reduced {
            0.0
        } else {
            self.now
                .saturating_duration_since(self.rainbow_preview_started)
                .as_secs_f32()
                / 6.0
        };
        fast_pack::compat::rainbow::color(phase)
    }
    fn palette_color(&self, target: ColorTarget) -> [u8; 3] {
        if self.palette_rainbow(target) {
            self.rainbow_preview_color()
        } else {
            self.color_for(target)
        }
    }
    fn palette_caption(&self, target: ColorTarget) -> Option<String> {
        if self.palette_rainbow(target) {
            Some("动态彩色".into())
        } else if target == ColorTarget::Glint {
            Some(if self.custom_glint {
                fast_pack::compat::glint::color_hex(self.glint_color)
            } else {
                "原版紫色".into()
            })
        } else {
            None
        }
    }
    pub(super) fn rainbow_preview_active(&self) -> bool {
        if self.reduced || self.learn_more {
            return false;
        }
        if self.glint_picker {
            return self.rainbow_for(self.color_target);
        }
        if !self.overlay_mode
            && (self.phase == super::Phase::Done || self.advanced_motion.value(self.now) < 0.001)
        {
            return false;
        }
        [
            ColorTarget::Glint,
            ColorTarget::Crosshair,
            ColorTarget::ItemOutline,
            ColorTarget::FishingLine,
            ColorTarget::BlockOutline,
        ]
        .into_iter()
        .any(|target| self.palette_rainbow(target))
    }
    pub(super) fn set_rainbow(&mut self, enabled: bool) {
        match self.color_target {
            ColorTarget::Glint => {
                self.rainbow.glint = enabled;
                if enabled {
                    self.custom_glint = true;
                }
            }
            ColorTarget::Crosshair => {
                self.rainbow.crosshair = enabled;
                if enabled {
                    self.custom_crosshair = true;
                }
            }
            ColorTarget::ItemOutline => self.item_editor.set_outline_rainbow(enabled),
            ColorTarget::FishingLine => {
                self.rainbow.fishing_line = enabled;
                if enabled {
                    self.fishing_line = Some(self.fishing_line_color);
                }
            }
            ColorTarget::BlockOutline => {
                self.rainbow.block_outline = enabled;
                if enabled {
                    self.block_outline = Some(self.block_outline_color);
                }
            }
            ColorTarget::Description => return,
        }
        self.glint_preview_started = self.now;
        self.glint_edited = Some(self.now);
        self.commit_glint_color();
    }
    pub(super) fn rainbow_hint(&self, target: ColorTarget) -> &'static str {
        match target {
            ColorTarget::Glint => "彩虹渐变随附魔光效流动；关闭后使用所选的固定颜色。",
            ColorTarget::Crosshair
                if fast_pack::pack_format_of(&self.target).is_ok_and(|format| format < 18) =>
            {
                "此版本的动态准星需要 OptiFine 自定义动画；颜色设置自动保存。"
            }
            _ => "开启后在游戏中循环变色；关闭后使用所选的固定颜色。设置自动保存。",
        }
    }
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
            ColorTarget::Description => self.overlay.ink,
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
            ColorTarget::Description => self.overlay.set_color(color),
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
        id: u8,
        target: ColorTarget,
        enabled: bool,
    ) -> Element<'_, Message> {
        let color = self.palette_color(target);
        let c = self.colors();
        let swatch = container(space().width(16).height(16)).style(move |_| {
            iced::widget::container::Style {
                background: Some(Color::from_rgb8(color[0], color[1], color[2]).into()),
                border: Border {
                    radius: 5.0.into(),
                    width: 1.0,
                    color: c.line,
                },
                ..Default::default()
            }
        });
        let mut control = row![swatch].spacing(10).align_y(Alignment::Center);
        if let Some(caption) = self.palette_caption(target) {
            control = control.push(self.label(caption));
        }
        control
            .push(self.action(
                id,
                "调色",
                enabled.then_some(Message::PickColor(target)),
                false,
            ))
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
