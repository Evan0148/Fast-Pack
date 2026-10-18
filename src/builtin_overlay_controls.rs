use super::{App, Message};
use fast_pack::builtin_overlays::Feature;
use iced::widget::{checkbox, column, row, space};
use iced::{Element, Length};

impl App {
    pub(super) fn builtin_overlay_controls(&self) -> Element<'_, Message> {
        let mut choices = column![
            row![
                self.title("内置覆盖功能", 15),
                space().width(Length::Fill),
                self.label(format!("已选 {} 项", self.builtin_overlays.count()))
            ],
            self.label("内置功能无需导入，勾选后可用于转换或制作覆盖包。")
        ]
        .spacing(12);
        for feature in Feature::ALL {
            let enabled = !self.busy();
            choices = choices.push(
                column![
                    checkbox(self.builtin_overlays.enabled(feature))
                        .label(feature.title())
                        .size(17)
                        .text_size(13)
                        .on_toggle_maybe(
                            enabled.then_some(move |value| Message::BuiltinOverlay(feature, value))
                        ),
                    self.label(feature.description())
                ]
                .spacing(4),
            );
        }
        choices=choices.push(self.label("等级数字：1.21.5+ 原版支持；1.18–1.21.4 需要 CIT 与 Forge 场景模型。更低版本保留基础外观。昵称效果沿用游戏已有文字绘制，光影或客户端覆盖可能使其不生效。"));
        self.panel(choices)
    }
}
