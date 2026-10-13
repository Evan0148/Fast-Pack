use crate::motion::Tween;
use anyhow::{Context, Result, ensure};
use fast_pack::converter::load_extra_renames;
use fast_pack::{
    ConversionOptions, ConversionReport, PackInfo, convert_pack, pack_format_of, probe_pack,
    suggest_output, tables,
};
use iced::widget::{
    self, button, checkbox, column, container, mouse_area, pick_list, progress_bar, row,
    scrollable, space, text, text_input, tooltip,
};
use iced::{
    Alignment, Background, Border, Color, Element, Font, Length, Padding, Shadow, Size,
    Subscription, Task, Theme, Vector, window,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
enum Message {
    Tick(Instant),
    Frame(Instant),
    WindowEvent(window::Id, window::Event),
    OpenFile,
    OpenFolder,
    InputPicked(Option<PathBuf>),
    Target(String),
    AutoSave(bool),
    AutoSavePath(bool),
    SourceVersion(String),
    Swap,
    Output(String),
    CommitExportDirectory,
    BrowseOutput,
    OutputPicked(Option<PathBuf>),
    Zip(bool),
    Description(String),
    PreserveLang(bool),
    PreserveCase(bool),
    CustomGlint(bool),
    GlintPicker,
    CloseGlintPicker,
    GlintPreset([u8; 3]),
    GlintSaturationValue(f32, f32),
    GlintHue(f32),
    GlintHex(String),
    Advanced,
    Details,
    LearnMore,
    CloseLearnMore,
    OpenSource,
    Mapping,
    MappingPicked(Option<PathBuf>),
    Start,
    ConfirmOverwrite,
    CancelOverwrite,
    NewPack,
    SaveReport,
    ReportPicked(Option<PathBuf>),
    OpenOutput,
    ClearLogs,
    Theme,
    ReducedMotion(bool),
    Hover(u8, bool),
    Captured(window::Screenshot),
}

enum WorkEvent {
    Probed(u64, PackInfo),
    Preview(
        u64,
        std::result::Result<Option<fast_pack::compat::sky::Preview>, String>,
    ),
    Progress(String, u8),
    Finished(Box<std::result::Result<ConversionReport, String>>),
    Mapping(std::result::Result<Vec<(String, String)>, String>),
    Preferences(std::result::Result<(), String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Empty,
    Identifying,
    Ready,
    Running,
    Done,
    Failed,
}

#[derive(Clone, Copy)]
struct Colors {
    background: Color,
    panel: Color,
    field: Color,
    foreground: Color,
    muted: Color,
    line: Color,
    accent: Color,
    on_accent: Color,
    success: Color,
    danger: Color,
}

fn rgb(value: u32) -> Color {
    Color::from_rgb8((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

fn mix(a: Color, b: Color, fraction: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * fraction,
        g: a.g + (b.g - a.g) * fraction,
        b: a.b + (b.b - a.b) * fraction,
        a: a.a + (b.a - a.a) * fraction,
    }
}

fn rgb_hsv(color: [u8; 3], previous_hue: f32) -> [f32; 3] {
    let [red, green, blue] = color.map(|channel| channel as f32 / 255.0);
    let maximum = red.max(green).max(blue);
    let minimum = red.min(green).min(blue);
    let difference = maximum - minimum;
    let hue = if difference < f32::EPSILON {
        previous_hue
    } else if maximum == red {
        ((green - blue) / difference / 6.0).rem_euclid(1.0)
    } else if maximum == green {
        ((blue - red) / difference + 2.0) / 6.0
    } else {
        ((red - green) / difference + 4.0) / 6.0
    };
    [
        hue,
        if maximum == 0.0 {
            0.0
        } else {
            difference / maximum
        },
        maximum,
    ]
}

fn hsv_rgb([hue, saturation, value]: [f32; 3]) -> [u8; 3] {
    let hue = hue.rem_euclid(1.0) * 6.0;
    let fraction = hue.fract();
    let low = value * (1.0 - saturation);
    let falling = value * (1.0 - fraction * saturation);
    let rising = value * (1.0 - (1.0 - fraction) * saturation);
    let channels = match hue as u32 {
        0 => [value, rising, low],
        1 => [falling, value, low],
        2 => [low, value, rising],
        3 => [low, falling, value],
        4 => [rising, low, value],
        _ => [value, low, falling],
    };
    channels.map(|channel| (channel * 255.0).round().clamp(0.0, 255.0) as u8)
}

impl Colors {
    fn at(dark: f32) -> Self {
        let blend = |light, deep| mix(rgb(light), rgb(deep), dark);
        Self {
            background: blend(0xf3f6fa, 0x10151b),
            panel: blend(0xffffff, 0x18212b),
            field: blend(0xf7f9fc, 0x101820),
            foreground: blend(0x152030, 0xe7eef6),
            muted: blend(0x526477, 0xa0b0c2),
            line: blend(0xdce5ee, 0x2b3949),
            accent: blend(0x086e85, 0x70d9ee),
            on_accent: blend(0xffffff, 0x102027),
            success: blend(0x137652, 0x83dcb4),
            danger: blend(0xbb3544, 0xff94a5),
        }
    }
}

fn hint_style(colors: Colors) -> container::Style {
    container::Style {
        background: Some(colors.panel.into()),
        text_color: Some(colors.foreground),
        border: Border {
            radius: 9.0.into(),
            width: 1.0,
            color: colors.line,
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.16),
            offset: Vector::new(0.0, 3.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    }
}

struct App {
    phase: Phase,
    source: Option<PathBuf>,
    info: Option<PackInfo>,
    sky_preview: Option<widget::image::Handle>,
    sky_layers: usize,
    source_version: String,
    target: String,
    target_selected: bool,
    auto_save: bool,
    saved_target: Option<String>,
    auto_save_path: bool,
    saved_export_directory: Option<PathBuf>,
    export_directory_input: String,
    directory_edited: Option<Instant>,
    output: String,
    description: String,
    as_zip: bool,
    preserve_lang: bool,
    preserve_case: bool,
    custom_glint: bool,
    glint_color: [u8; 3],
    glint_hsv: [f32; 3],
    glint_hex: String,
    glint_edited: Option<Instant>,
    glint_picker: bool,
    glint_picker_motion: Tween,
    glint_color_motion: [Tween; 3],
    glint_preview_started: Instant,
    extra: Vec<(String, String)>,
    report: Option<String>,
    logs: Vec<String>,
    error: Option<String>,
    step: String,
    actual_progress: u8,
    advanced: bool,
    details: bool,
    learn_more: bool,
    dark: bool,
    reduced: bool,
    now: Instant,
    progress: Tween,
    advanced_motion: Tween,
    result_motion: Tween,
    load_motion: Tween,
    theme_motion: Tween,
    auto_save_motion: Tween,
    auto_save_path_motion: Tween,
    hover: HashMap<u8, Tween>,
    width: f32,
    generation: u64,
    sender: Sender<WorkEvent>,
    receiver: Receiver<WorkEvent>,
    preferences_sender: Sender<crate::preferences::Preferences>,
    preferences_pending: usize,
    mapping_pending: bool,
    window: Option<window::Id>,
    closing: bool,
    confirm_overwrite: bool,
    smoke: bool,
    smoke_output: Option<PathBuf>,
    capture: Option<PathBuf>,
    capture_stage: String,
    capture_requested: bool,
    settled_at: Instant,
}

impl App {
    fn boot() -> (Self, Task<Message>) {
        let (sender, receiver) = mpsc::channel();
        let now = Instant::now();
        let smoke = std::env::args().any(|value| value == "--smoke-test");
        let capture = std::env::var_os("FAST_PACK_CAPTURE").map(PathBuf::from);
        let dark = std::env::var("FAST_PACK_CAPTURE_THEME").as_deref() != Ok("light");
        let capture_stage =
            std::env::var("FAST_PACK_CAPTURE_STAGE").unwrap_or_else(|_| "empty".into());
        let preferences = if !cfg!(test) && !smoke && capture.is_none() {
            crate::preferences::load()
        } else {
            crate::preferences::Preferences::default()
        };
        let saved_target = preferences.target_version;
        let auto_save = preferences.auto_save;
        let auto_save_path = preferences.auto_save_path;
        let saved_export_directory = preferences.export_directory;
        let active_glint_color = fast_pack::compat::glint::VANILLA_COLOR;
        let export_directory_input = saved_export_directory
            .as_ref()
            .filter(|_| auto_save_path)
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let target_selected = auto_save && saved_target.is_some();
        let (preferences_sender, preferences_receiver) =
            mpsc::channel::<crate::preferences::Preferences>();
        if !cfg!(test) && !smoke && capture.is_none() {
            let sender = sender.clone();
            std::thread::spawn(move || {
                for preferences in preferences_receiver {
                    let result = crate::preferences::save(&preferences)
                        .map_err(|error| format!("{error:#}"));
                    if sender.send(WorkEvent::Preferences(result)).is_err() {
                        break;
                    }
                }
            });
        }
        let mut app = Self {
            phase: Phase::Empty,
            source: None,
            info: None,
            sky_preview: None,
            sky_layers: 0,
            source_version: "自动检测".into(),
            target: saved_target
                .as_ref()
                .filter(|_| auto_save)
                .cloned()
                .unwrap_or_else(|| fast_pack::LATEST_VERSION.into()),
            target_selected,
            auto_save,
            saved_target,
            auto_save_path,
            saved_export_directory,
            export_directory_input,
            directory_edited: None,
            output: String::new(),
            description: String::new(),
            as_zip: true,
            preserve_lang: false,
            preserve_case: false,
            custom_glint: false,
            glint_color: preferences.glint_color,
            glint_hsv: rgb_hsv(active_glint_color, 0.0),
            glint_hex: fast_pack::compat::glint::color_hex(active_glint_color),
            glint_edited: None,
            glint_picker: false,
            glint_picker_motion: Tween::new(0.0, 220),
            glint_color_motion: active_glint_color
                .map(|channel| Tween::new(channel as f32 / 255.0, 180)),
            glint_preview_started: now,
            extra: Vec::new(),
            report: None,
            logs: Vec::new(),
            error: None,
            step: String::new(),
            actual_progress: 0,
            advanced: false,
            details: false,
            learn_more: false,
            dark,
            reduced: system_reduced_motion(),
            now,
            progress: Tween::new(0.0, 220),
            advanced_motion: Tween::new(0.0, 230),
            result_motion: Tween::new(0.0, 280),
            load_motion: Tween::new(0.0, 240),
            theme_motion: Tween::new(if dark { 1.0 } else { 0.0 }, 180),
            auto_save_motion: Tween::new(if auto_save { 1.0 } else { 0.0 }, 220),
            auto_save_path_motion: Tween::new(if auto_save_path { 1.0 } else { 0.0 }, 220),
            hover: HashMap::new(),
            width: 1040.0,
            generation: 0,
            sender,
            receiver,
            preferences_sender,
            preferences_pending: 0,
            mapping_pending: false,
            window: None,
            closing: false,
            confirm_overwrite: false,
            smoke,
            smoke_output: std::env::var_os("FAST_PACK_SMOKE_OUTPUT").map(PathBuf::from),
            capture,
            capture_stage,
            capture_requested: false,
            settled_at: now,
        };
        if let Some(path) = std::env::var_os("FAST_PACK_SMOKE_INPUT") {
            app.load(PathBuf::from(path));
        }
        if app.capture_stage == "advanced" {
            app.advanced = true;
            app.advanced_motion.set(1.0, now, true);
        }
        (app, Task::none())
    }

    fn busy(&self) -> bool {
        self.phase == Phase::Running
    }
    fn colors(&self) -> Colors {
        Colors::at(self.theme_motion.value(self.now))
    }
    fn animation_active(&self) -> bool {
        [
            &self.progress,
            &self.advanced_motion,
            &self.result_motion,
            &self.load_motion,
            &self.theme_motion,
            &self.auto_save_motion,
            &self.auto_save_path_motion,
            &self.glint_picker_motion,
        ]
        .into_iter()
        .any(|value| value.active(self.now))
            || self.hover.values().any(|value| value.active(self.now))
            || self
                .glint_color_motion
                .iter()
                .any(|value| value.active(self.now))
    }
    fn subscription(&self) -> Subscription<Message> {
        let events =
            iced::event::listen_with(|event, _, id| match event {
                iced::Event::Window(event) => Some(Message::WindowEvent(id, event)),
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::CancelOverwrite),
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key, modifiers, ..
                }) if modifiers.control() => match key.as_ref() {
                    iced::keyboard::Key::Character("o") => Some(if modifiers.shift() {
                        Message::OpenFolder
                    } else {
                        Message::OpenFile
                    }),
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter) => {
                        Some(Message::Start)
                    }
                    _ => None,
                },
                _ => None,
            });
        if self.animation_active()
            || self.phase == Phase::Identifying
            || self.busy()
            || self.mapping_pending
            || self.preferences_pending > 0
            || self.directory_edited.is_some()
            || self.glint_edited.is_some()
            || self.glint_picker && !self.reduced
            || self.capture.is_some()
            || self.smoke
        {
            let duration = if self.animation_active() || self.glint_picker && !self.reduced {
                16
            } else {
                40
            };
            let mut subscriptions = vec![
                events,
                iced::time::every(Duration::from_millis(duration)).map(Message::Tick),
            ];
            if self.animation_active() {
                subscriptions.push(window::frames().map(Message::Frame));
            }
            Subscription::batch(subscriptions)
        } else {
            events
        }
    }
    fn theme(&self) -> Theme {
        let c = self.colors();
        Theme::custom(
            "Fast Pack",
            iced::theme::Palette {
                background: c.background,
                text: c.foreground,
                primary: c.accent,
                success: c.success,
                warning: rgb(0xd6a859),
                danger: c.danger,
            },
        )
    }

    fn load(&mut self, path: PathBuf) {
        if self.busy() {
            return;
        }
        self.generation += 1;
        self.source = Some(path.clone());
        self.info = None;
        self.sky_preview = None;
        self.sky_layers = 0;
        self.report = None;
        self.error = None;
        self.phase = Phase::Identifying;
        self.logs.clear();
        self.result_motion.set(0.0, self.now, self.reduced);
        self.load_motion.set(0.0, self.now, self.reduced);
        let generation = self.generation;
        let sender = self.sender.clone();
        let preview_path = path.clone();
        let preview_sender = sender.clone();
        std::thread::spawn(move || {
            let preview = fast_pack::compat::sky::preview(&preview_path)
                .map_err(|error| format!("{error:#}"));
            let _ = preview_sender.send(WorkEvent::Preview(generation, preview));
        });
        std::thread::spawn(move || {
            let info = probe_pack(&path);
            let _ = sender.send(WorkEvent::Probed(generation, info));
        });
    }
    fn update_output(&mut self) {
        if let Some(source) = &self.source {
            let Ok(directory) = self.parsed_export_directory() else {
                self.output.clear();
                return;
            };
            let suggested = suggest_output(source, &self.target, self.as_zip);
            let output = if let Some(directory) = directory {
                let name = suggested.file_name().unwrap_or_default();
                let candidate = directory.join(name);
                if candidate
                    .to_string_lossy()
                    .replace('\\', "/")
                    .eq_ignore_ascii_case(&source.to_string_lossy().replace('\\', "/"))
                {
                    directory.join("Fast Pack").join(name)
                } else {
                    candidate
                }
            } else {
                suggested
            };
            self.output = output.display().to_string();
        }
    }
    fn parsed_export_directory(&self) -> Result<Option<PathBuf>> {
        let input = self.export_directory_input.trim().trim_matches('"');
        if input.is_empty() {
            return Ok(None);
        }
        let path = PathBuf::from(input);
        ensure!(path.is_absolute(), "请选择完整的导出目录");
        ensure!(!path.is_file(), "导出位置应是文件夹，不能是文件");
        Ok(Some(path))
    }
    fn remember_export_directory(&mut self) {
        if self.auto_save_path
            && let Ok(directory) = self.parsed_export_directory()
            && directory != self.saved_export_directory
        {
            self.saved_export_directory = directory;
            self.save_preferences();
        }
    }
    fn remember_target(&mut self) {
        self.target_selected = true;
        if self.auto_save {
            self.saved_target = Some(self.target.clone());
            self.save_preferences();
        }
    }
    fn save_preferences(&mut self) {
        if !cfg!(test) && !self.smoke && self.capture.is_none() {
            if self
                .preferences_sender
                .send(crate::preferences::Preferences {
                    auto_save: self.auto_save,
                    target_version: self.saved_target.clone(),
                    auto_save_path: self.auto_save_path,
                    export_directory: self.saved_export_directory.clone(),
                    custom_glint: self.custom_glint,
                    glint_color: self.glint_color,
                })
                .is_ok()
            {
                self.preferences_pending += 1;
            } else {
                self.log("偏好设置未能保存：保存任务不可用".into());
            }
        }
    }

    fn log(&mut self, value: String) {
        if self.logs.len() >= 500 {
            self.logs.drain(..100);
        }
        self.logs.push(value);
    }
    fn active_glint_color(&self) -> [u8; 3] {
        if self.custom_glint {
            self.glint_color
        } else {
            fast_pack::compat::glint::VANILLA_COLOR
        }
    }
    fn sync_glint_color(&mut self) {
        let color = self.active_glint_color();
        self.glint_hsv = rgb_hsv(color, self.glint_hsv[0]);
        self.glint_hex = fast_pack::compat::glint::color_hex(color);
        for (motion, channel) in self.glint_color_motion.iter_mut().zip(color) {
            motion.set(channel as f32 / 255.0, self.now, self.reduced);
        }
    }
    fn set_glint_color(&mut self, color: [u8; 3]) {
        self.custom_glint = true;
        self.glint_color = color;
        self.sync_glint_color();
        self.glint_edited = Some(self.now);
    }
    fn commit_glint_color(&mut self) {
        if self.glint_edited.take().is_some() {
            self.save_preferences();
        }
    }
    fn close_glint_picker(&mut self) {
        self.glint_picker = false;
        self.glint_picker_motion.set(0.0, self.now, self.reduced);
        self.glint_hex = fast_pack::compat::glint::color_hex(self.active_glint_color());
        self.commit_glint_color();
    }
    fn fail(&mut self, value: String) {
        self.error = Some(value.clone());
        self.phase = Phase::Failed;
        self.log(format!("转换失败：{value}"));
        self.result_motion.set(1.0, self.now, self.reduced);
        self.settled_at = self.now;
    }
    fn start(&mut self) {
        if self.busy() || self.phase == Phase::Identifying {
            return;
        }
        self.close_glint_picker();
        if let Err(error) = self.parsed_export_directory() {
            self.error = Some(error.to_string());
            return;
        }
        self.directory_edited = None;
        self.remember_export_directory();
        let Some(source) = self.source.clone() else {
            self.error = Some("请先导入材质包".into());
            return;
        };
        if !self.info.as_ref().is_some_and(|value| value.ok) {
            self.error = Some("请导入有效的材质包".into());
            return;
        }
        if self.output.trim().is_empty() {
            self.error = Some("请选择输出路径".into());
            return;
        }
        if self.as_zip && !self.output.to_lowercase().ends_with(".zip") {
            self.output.push_str(".zip");
        }
        let output = PathBuf::from(self.output.trim());
        let source_format = if self.source_version == "自动检测" {
            None
        } else {
            pack_format_of(&self.source_version).ok()
        };
        let Ok(target_format) = pack_format_of(&self.target) else {
            self.error = Some("目标版本无效".into());
            return;
        };
        let options = ConversionOptions {
            src_format: source_format,
            tgt_format: target_format,
            target_version: Some(self.target.clone()),
            description: (!self.description.trim().is_empty())
                .then(|| self.description.trim().into()),
            as_zip: self.as_zip,
            lowercase: !self.preserve_case,
            convert_lang: !self.preserve_lang,
            extra_renames: self.extra.clone(),
            glint_color: self.custom_glint.then_some(self.glint_color),
            restore_vanilla_glint: !self.custom_glint,
        };
        self.phase = Phase::Running;
        self.error = None;
        self.report = None;
        self.logs.clear();
        self.actual_progress = 0;
        self.progress.set(0.0, self.now, true);
        self.result_motion.set(0.0, self.now, true);
        self.step = "正在准备".into();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                convert_pack(&source, &output, &options, |step, percent| {
                    let _ = sender.send(WorkEvent::Progress(step.into(), percent));
                })
            }))
            .map_err(|_| "转换线程意外停止".to_string())
            .and_then(|value| value.map_err(|error| format!("{error:#}")));
            let _ = sender.send(WorkEvent::Finished(Box::new(result)));
        });
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        self.now = Instant::now();
        match message {
            Message::Frame(now) => self.now = now,
            Message::Tick(now) => {
                self.now = now;
                if self.glint_edited.is_some_and(|edited| {
                    now.saturating_duration_since(edited) >= Duration::from_millis(350)
                }) {
                    self.commit_glint_color();
                }
                if self.directory_edited.is_some_and(|edited| {
                    now.saturating_duration_since(edited) >= Duration::from_millis(350)
                }) {
                    self.directory_edited = None;
                    self.remember_export_directory();
                }
                let mut reset_scroll = false;
                let events: Vec<_> = self.receiver.try_iter().collect();
                for event in events {
                    match event {
                        WorkEvent::Probed(id, info) if id == self.generation => {
                            if info.ok {
                                if !self.target_selected {
                                    self.target = info.suggested_target.clone();
                                }
                                self.source_version = info
                                    .version_label
                                    .as_ref()
                                    .filter(|label| {
                                        tables()
                                            .pack_formats
                                            .iter()
                                            .any(|(value, _)| value == *label)
                                    })
                                    .cloned()
                                    .unwrap_or_else(|| "自动检测".into());
                                self.log(format!(
                                    "识别完成：{} 个文件，{} 张纹理",
                                    info.file_count, info.texture_count
                                ));
                                self.info = Some(info);
                                self.phase = Phase::Ready;
                                self.update_output();
                                self.settled_at = now;
                                self.load_motion.set(1.0, now, self.reduced);
                                if let Ok(target) = std::env::var("FAST_PACK_SMOKE_TARGET") {
                                    self.target = target;
                                    self.update_output();
                                }
                                if let Some(output) = &self.smoke_output {
                                    self.output = output.display().to_string();
                                    self.start();
                                }
                            } else {
                                self.info = Some(info.clone());
                                self.fail(
                                    info.error
                                        .unwrap_or_else(|| "没有找到 assets 或 pack.mcmeta".into()),
                                );
                            }
                        }
                        WorkEvent::Preview(id, result) if id == self.generation => match result {
                            Ok(Some(preview)) => {
                                self.sky_layers = preview.names.len();
                                let image = image::imageops::thumbnail(&preview.image, 768, 512);
                                self.sky_preview = Some(widget::image::Handle::from_rgba(
                                    image.width(),
                                    image.height(),
                                    image.into_raw(),
                                ));
                                if self.capture.is_some() {
                                    self.settled_at = now;
                                }
                            }
                            Err(error) => self.log(format!("天空预览：{error}")),
                            _ => {}
                        },
                        WorkEvent::Progress(step, percent) => {
                            self.step = step.clone();
                            self.actual_progress = percent;
                            self.progress.set(percent as f32, now, self.reduced);
                            self.log(format!("{percent:3}%  {step}"));
                        }
                        WorkEvent::Finished(result) => {
                            match *result {
                                Ok(report) => {
                                    self.report = Some(report.render());
                                    self.phase = Phase::Done;
                                    reset_scroll = true;
                                    self.actual_progress = 100;
                                    self.progress.set(100.0, now, self.reduced);
                                    self.result_motion.set(1.0, now, self.reduced);
                                    self.settled_at = now;
                                    self.log("转换完成，原文件已保留".into());
                                    if let Some(path) = &self.smoke_output {
                                        let _ = std::fs::write(
                                            path.with_extension("gui-report.json"),
                                            serde_json::to_string_pretty(&report)
                                                .unwrap_or_default(),
                                        );
                                    }
                                }
                                Err(error) => self.fail(error),
                            }
                            self.custom_glint = false;
                            self.glint_edited = None;
                            self.sync_glint_color();
                            self.save_preferences();
                            if self.closing && self.preferences_pending == 0
                                || self.smoke && self.capture.is_none()
                            {
                                return iced::exit();
                            }
                        }
                        WorkEvent::Mapping(result) => {
                            self.mapping_pending = false;
                            match result {
                                Ok(mapping) => self.extra = mapping,
                                Err(error) => self.error = Some(error),
                            }
                        }
                        WorkEvent::Preferences(result) => {
                            self.preferences_pending = self.preferences_pending.saturating_sub(1);
                            if let Err(error) = result {
                                self.log(format!("偏好设置未能保存：{error}"));
                            }
                            if self.closing && !self.busy() && self.preferences_pending == 0 {
                                return iced::exit();
                            }
                        }
                        _ => {}
                    }
                }
                if self.smoke
                    && self.smoke_output.is_none()
                    && self.capture.is_none()
                    && now.saturating_duration_since(self.settled_at) > Duration::from_millis(800)
                {
                    return iced::exit();
                }
                let can_capture = match self.capture_stage.as_str() {
                    "sky" => self.sky_preview.is_some(),
                    "running" => self.busy(),
                    "done" => self.phase == Phase::Done,
                    "failed" => self.phase == Phase::Failed,
                    "loaded" | "advanced" => self.phase == Phase::Ready,
                    _ => self.phase == Phase::Empty,
                };
                if self.capture.is_some()
                    && !self.capture_requested
                    && can_capture
                    && now.saturating_duration_since(self.settled_at) > Duration::from_millis(1000)
                    && let Some(window) = self.window
                {
                    self.capture_requested = true;
                    return window::screenshot(window).map(Message::Captured);
                }
                if reset_scroll {
                    return widget::operation::snap_to(
                        "fast-pack-content",
                        widget::operation::RelativeOffset {
                            x: Some(0.0),
                            y: Some(0.0),
                        },
                    );
                }
            }
            Message::WindowEvent(id, event) => match event {
                window::Event::Opened { size, .. } => {
                    self.window = Some(id);
                    self.width = size.width;
                }
                window::Event::Resized(size) => self.width = size.width,
                window::Event::FileDropped(path) => self.load(path),
                window::Event::FileHovered(_) => {
                    self.hover
                        .entry(1)
                        .or_insert_with(|| Tween::new(0.0, 150))
                        .set(1.0, self.now, self.reduced);
                }
                window::Event::FilesHoveredLeft => {
                    self.hover
                        .entry(1)
                        .or_insert_with(|| Tween::new(0.0, 150))
                        .set(0.0, self.now, self.reduced);
                }
                window::Event::CloseRequested => {
                    self.commit_glint_color();
                    self.directory_edited = None;
                    self.remember_export_directory();
                    if self.busy() || self.preferences_pending > 0 {
                        self.closing = true;
                        self.log(if self.busy() {
                            "转换完成并清理临时文件后关闭窗口".into()
                        } else {
                            "设置保存完成后关闭窗口".into()
                        });
                    } else {
                        return iced::exit();
                    }
                }
                _ => {}
            },
            Message::OpenFile if !self.busy() => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Fast Pack · 导入材质包")
                            .add_filter("ZIP", &["zip"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_owned())
                    },
                    Message::InputPicked,
                );
            }
            Message::OpenFolder if !self.busy() => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Fast Pack · 选择材质包文件夹")
                            .pick_folder()
                            .await
                            .map(|file| file.path().to_owned())
                    },
                    Message::InputPicked,
                );
            }
            Message::InputPicked(Some(path)) => self.load(path),
            Message::Target(target) if !self.busy() => {
                if !tables()
                    .pack_formats
                    .iter()
                    .any(|(label, _)| label == &target)
                {
                    return Task::none();
                }
                self.target = target;
                self.remember_target();
                self.update_output();
                self.report = None;
                self.error = None;
                if self.info.is_some() {
                    self.phase = Phase::Ready;
                    self.result_motion.set(0.0, self.now, self.reduced);
                }
            }
            Message::SourceVersion(version) if !self.busy() => {
                self.source_version = version;
                self.report = None;
                self.error = None;
            }
            Message::AutoSave(value) if !self.busy() => {
                self.auto_save = value;
                self.auto_save_motion
                    .set(if value { 1.0 } else { 0.0 }, self.now, self.reduced);
                if value {
                    self.remember_target();
                } else {
                    self.save_preferences();
                }
            }
            Message::AutoSavePath(value) if !self.busy() => {
                self.auto_save_path = value;
                self.auto_save_path_motion.set(
                    if value { 1.0 } else { 0.0 },
                    self.now,
                    self.reduced,
                );
                self.directory_edited = None;
                if value {
                    if self.export_directory_input.trim().is_empty()
                        && let Some(directory) = &self.saved_export_directory
                    {
                        self.export_directory_input = directory.display().to_string();
                    }
                    if let Ok(directory) = self.parsed_export_directory() {
                        self.saved_export_directory = directory;
                    }
                    self.update_output();
                }
                self.save_preferences();
            }
            Message::CustomGlint(value) if !self.busy() => {
                self.custom_glint = value;
                self.sync_glint_color();
                self.glint_edited = None;
                self.save_preferences();
            }
            Message::GlintPicker if !self.busy() => {
                self.glint_picker = true;
                self.glint_picker_motion.set(1.0, self.now, self.reduced);
                self.glint_preview_started = self.now;
            }
            Message::CloseGlintPicker => self.close_glint_picker(),
            Message::GlintPreset(color) if !self.busy() => self.set_glint_color(color),
            Message::GlintSaturationValue(saturation, value) if !self.busy() => {
                let hsv = [
                    self.glint_hsv[0],
                    saturation.clamp(0.0, 1.0),
                    value.clamp(0.0, 1.0),
                ];
                self.set_glint_color(hsv_rgb(hsv));
                self.glint_hsv = hsv;
            }
            Message::GlintHue(hue) if !self.busy() => {
                let hsv = [hue.clamp(0.0, 1.0), self.glint_hsv[1], self.glint_hsv[2]];
                self.set_glint_color(hsv_rgb(hsv));
                self.glint_hsv = hsv;
            }
            Message::GlintHex(value) if !self.busy() => {
                if let Ok(color) = fast_pack::compat::glint::parse_color(&value) {
                    self.set_glint_color(color);
                }
                self.glint_hex = value;
            }
            Message::Output(output) if !self.busy() => {
                self.export_directory_input = output;
                self.directory_edited = self.auto_save_path.then_some(self.now);
                self.update_output();
                self.error = None;
            }
            Message::CommitExportDirectory if !self.busy() => {
                self.directory_edited = None;
                if let Err(error) = self.parsed_export_directory() {
                    self.error = Some(error.to_string());
                } else {
                    self.remember_export_directory();
                    self.update_output();
                    self.error = None;
                }
            }
            Message::Description(value) if !self.busy() => self.description = value,
            Message::PreserveLang(value) if !self.busy() => self.preserve_lang = value,
            Message::PreserveCase(value) if !self.busy() => self.preserve_case = value,
            Message::Zip(value) if !self.busy() => {
                self.as_zip = value;
                self.update_output();
            }
            Message::Swap if !self.busy() => {
                let source = if self.source_version == "自动检测" {
                    self.info
                        .as_ref()
                        .and_then(|info| info.version_label.clone())
                } else {
                    Some(self.source_version.clone())
                };
                if let Some(source) = source.filter(|label| {
                    tables()
                        .pack_formats
                        .iter()
                        .any(|(value, _)| value == label)
                }) {
                    self.source_version = self.target.clone();
                    self.target = source;
                    self.remember_target();
                    self.update_output();
                } else {
                    self.error = Some("请先指定具体源版本".into());
                }
            }
            Message::BrowseOutput if !self.busy() => {
                let directory = self.parsed_export_directory().ok().flatten().or_else(|| {
                    self.source
                        .as_ref()
                        .and_then(|path| path.parent().map(Path::to_owned))
                });
                return Task::perform(
                    async move {
                        let mut dialog =
                            rfd::AsyncFileDialog::new().set_title("Fast Pack · 选择导出目录");
                        if let Some(directory) = directory {
                            dialog = dialog.set_directory(directory);
                        }
                        dialog
                            .pick_folder()
                            .await
                            .map(|file| file.path().to_owned())
                    },
                    Message::OutputPicked,
                );
            }
            Message::OutputPicked(Some(path)) if !self.busy() => {
                self.export_directory_input = path.display().to_string();
                self.directory_edited = None;
                self.update_output();
                self.remember_export_directory();
                self.error = None;
            }
            Message::Advanced => {
                self.advanced = !self.advanced;
                self.advanced_motion.set(
                    if self.advanced { 1.0 } else { 0.0 },
                    self.now,
                    self.reduced,
                );
            }
            Message::Details => self.details = !self.details,
            Message::Mapping if !self.busy() => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Fast Pack · 自定义映射")
                            .add_filter("JSON", &["json"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_owned())
                    },
                    Message::MappingPicked,
                );
            }
            Message::MappingPicked(Some(path)) => {
                self.mapping_pending = true;
                let sender = self.sender.clone();
                std::thread::spawn(move || {
                    let result = load_extra_renames(&path).map_err(|error| format!("{error:#}"));
                    let _ = sender.send(WorkEvent::Mapping(result));
                });
            }
            Message::Start if !self.busy() && self.phase != Phase::Identifying => {
                if Path::new(self.output.trim()).exists() {
                    self.confirm_overwrite = true;
                } else {
                    self.start();
                }
            }
            Message::ConfirmOverwrite => {
                self.confirm_overwrite = false;
                self.start();
            }
            Message::CancelOverwrite => {
                self.confirm_overwrite = false;
                self.learn_more = false;
                self.close_glint_picker();
            }
            Message::NewPack => {
                self.generation += 1;
                self.sky_preview = None;
                self.sky_layers = 0;
                self.phase = Phase::Empty;
                self.source = None;
                self.info = None;
                self.report = None;
                self.error = None;
                self.logs.clear();
                self.output.clear();
                self.result_motion.set(0.0, self.now, self.reduced);
                return widget::operation::snap_to(
                    "fast-pack-content",
                    widget::operation::RelativeOffset {
                        x: Some(0.0),
                        y: Some(0.0),
                    },
                );
            }
            Message::SaveReport if self.report.is_some() => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Fast Pack · 保存报告")
                            .set_file_name("Fast Pack 转换报告.txt")
                            .add_filter("文本", &["txt"])
                            .save_file()
                            .await
                            .map(|file| file.path().to_owned())
                    },
                    Message::ReportPicked,
                );
            }
            Message::ReportPicked(Some(path)) => {
                if let Some(report) = &self.report
                    && let Err(error) = std::fs::write(path, report)
                {
                    self.error = Some(error.to_string());
                }
            }
            Message::OpenOutput => {
                let path = PathBuf::from(&self.output);
                let directory = if path.is_dir() {
                    path
                } else {
                    path.parent().unwrap_or(Path::new(".")).to_owned()
                };
                let _ = std::process::Command::new("explorer.exe")
                    .arg(directory)
                    .spawn();
            }
            Message::ClearLogs => self.logs.clear(),
            Message::LearnMore => self.learn_more = true,
            Message::CloseLearnMore => self.learn_more = false,
            Message::OpenSource => {
                let _ = std::process::Command::new("explorer.exe")
                    .arg("https://github.com/LongCold-ColdEternity-Team/Fast-Pack")
                    .spawn();
            }
            Message::Theme => {
                self.dark = !self.dark;
                self.theme_motion
                    .set(if self.dark { 1.0 } else { 0.0 }, self.now, self.reduced);
            }
            Message::ReducedMotion(value) => {
                self.reduced = value;
                if value {
                    self.progress.snap();
                    self.advanced_motion.snap();
                    self.result_motion.snap();
                    self.load_motion.snap();
                    self.theme_motion.snap();
                    self.auto_save_motion.snap();
                    self.auto_save_path_motion.snap();
                    self.glint_picker_motion.snap();
                    for motion in &mut self.glint_color_motion {
                        motion.snap();
                    }
                    for value in self.hover.values_mut() {
                        value.snap();
                    }
                }
            }
            Message::Hover(id, value) => self
                .hover
                .entry(id)
                .or_insert_with(|| Tween::new(0.0, 150))
                .set(if value { 1.0 } else { 0.0 }, self.now, self.reduced),
            Message::Captured(screenshot) => {
                if let Some(path) = &self.capture {
                    let _ = save_screenshot(path, &screenshot);
                }
                return iced::exit();
            }
            _ => {}
        }
        Task::none()
    }

    fn action<'a>(
        &self,
        id: u8,
        label: impl Into<String>,
        message: Option<Message>,
        primary: bool,
    ) -> Element<'a, Message> {
        let c = self.colors();
        let hover = self
            .hover
            .get(&id)
            .map(|value| value.value(self.now))
            .unwrap_or(0.0);
        let widget = button(text(label.into()).size(14))
            .padding([11, 17])
            .on_press_maybe(message)
            .style(move |_, status| {
                let base = if primary { c.accent } else { c.panel };
                let background = mix(
                    base,
                    if primary { c.foreground } else { c.accent },
                    hover * 0.1,
                );
                let disabled = status == button::Status::Disabled;
                button::Style {
                    background: Some(Background::Color(if disabled {
                        mix(background, c.background, 0.55)
                    } else {
                        background
                    })),
                    text_color: if disabled {
                        c.muted
                    } else if primary {
                        c.on_accent
                    } else {
                        c.foreground
                    },
                    border: Border {
                        radius: 10.0.into(),
                        width: if primary { 0.0 } else { 1.0 },
                        color: mix(c.line, c.accent, hover * 0.7),
                    },
                    shadow: Shadow {
                        color: Color::from_rgba(
                            c.accent.r,
                            c.accent.g,
                            c.accent.b,
                            if disabled { 0.0 } else { hover * 0.12 },
                        ),
                        offset: Vector::ZERO,
                        blur_radius: hover * 12.0,
                    },
                    ..Default::default()
                }
            });
        mouse_area(widget)
            .on_enter(Message::Hover(id, true))
            .on_exit(Message::Hover(id, false))
            .into()
    }

    fn label<'a>(&self, value: impl Into<String>) -> widget::Text<'a> {
        text(value.into()).size(12).color(self.colors().muted)
    }
    fn input_style(&self, status: widget::text_input::Status) -> widget::text_input::Style {
        let c = self.colors();
        let focused = matches!(status, widget::text_input::Status::Focused { .. });
        widget::text_input::Style {
            background: c.field.into(),
            border: Border {
                radius: 10.0.into(),
                width: 1.0,
                color: if focused { c.accent } else { c.line },
            },
            icon: c.muted,
            placeholder: c.muted,
            value: c.foreground,
            selection: mix(c.panel, c.accent, 0.25),
        }
    }
    fn picker_style(&self, status: widget::pick_list::Status, id: u8) -> widget::pick_list::Style {
        let c = self.colors();
        let hover = self
            .hover
            .get(&id)
            .map(|value| value.value(self.now))
            .unwrap_or(0.0);
        let active = matches!(
            status,
            widget::pick_list::Status::Opened { .. } | widget::pick_list::Status::Hovered
        );
        widget::pick_list::Style {
            text_color: c.foreground,
            placeholder_color: c.muted,
            handle_color: mix(c.muted, c.accent, hover * 0.65),
            background: c.field.into(),
            border: Border {
                radius: 10.0.into(),
                width: 1.0,
                color: if active || hover > 0.01 {
                    mix(c.line, c.accent, 0.55 + hover * 0.45)
                } else {
                    c.line
                },
            },
        }
    }
    fn title<'a>(&self, value: impl Into<String>, size: u32) -> widget::Text<'a> {
        text(value.into())
            .size(size)
            .font(Font {
                weight: if size >= 17 {
                    iced::font::Weight::Semibold
                } else {
                    iced::font::Weight::Medium
                },
                ..Font::with_name(crate::windows_compat::font_family())
            })
            .color(self.colors().foreground)
    }

    fn panel<'a>(&self, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
        let c = self.colors();
        container(content)
            .padding(24)
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(c.panel.into()),
                text_color: Some(c.foreground),
                border: Border {
                    color: c.line,
                    width: 1.0,
                    radius: 16.0.into(),
                },
                shadow: Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.05),
                    offset: Vector::new(0.0, 5.0),
                    blur_radius: 16.0,
                },
                ..Default::default()
            })
            .into()
    }

    fn source_panel(&self) -> Element<'_, Message> {
        let c = self.colors();
        let hover = self
            .hover
            .get(&1)
            .map(|motion| motion.value(self.now))
            .unwrap_or(0.0);
        let name = self
            .source
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|value| value.to_string_lossy().into_owned());
        let caption = match self.phase {
            Phase::Identifying => "正在后台识别…",
            Phase::Empty => "支持 ZIP 或已解压文件夹",
            Phase::Failed if !self.info.as_ref().is_some_and(|info| info.ok) => {
                "未识别为有效的材质包"
            }
            _ => "原文件保留，转换结果另存",
        };
        let cube = widget::canvas(Artwork {
            color: c.accent,
            kind: ArtworkKind::Package,
        })
        .width(54)
        .height(56);
        let file_body = column![
            cube,
            self.title(
                name.as_deref()
                    .map(|name| shorten(name, 34))
                    .unwrap_or_else(|| "把材质包拖到这里".into()),
                18
            ),
            self.label(caption),
            row![
                self.action(
                    2,
                    if self.source.is_some() {
                        "更换 ZIP"
                    } else {
                        "选择 ZIP"
                    },
                    (!self.busy()).then_some(Message::OpenFile),
                    false
                ),
                self.action(
                    3,
                    "选择文件夹",
                    (!self.busy()).then_some(Message::OpenFolder),
                    false
                )
            ]
            .spacing(10)
        ]
        .spacing(14)
        .align_x(Alignment::Center);
        let drop = container(file_body)
            .padding(24)
            .center_x(Length::Fill)
            .style(move |_| container::Style {
                background: Some(mix(c.field, c.accent, 0.03 + 0.07 * hover).into()),
                border: Border {
                    color: mix(c.line, c.accent, hover),
                    width: 1.0,
                    radius: 13.0.into(),
                },
                ..Default::default()
            });
        let mut content = column![
            self.title("源材质包", 14),
            mouse_area(drop)
                .on_enter(Message::Hover(1, true))
                .on_exit(Message::Hover(1, false))
        ]
        .spacing(18);
        if let Some(info) = &self.info {
            let format = info.version_label.as_deref().unwrap_or("自动检测");
            let mut versions = vec!["自动检测".to_string()];
            versions.extend(tables().pack_formats.iter().map(|(label, _)| label.clone()));
            let amount = self.load_motion.value(self.now);
            let mut muted = c.muted;
            muted.a = amount;
            let metadata = column![
                row![
                    text(format!("源版本 {format}")).size(12).color(muted),
                    space().width(Length::Fill),
                    text(format!(
                        "{} 文件 · {} 张纹理",
                        info.file_count, info.texture_count
                    ))
                    .size(12)
                    .color(muted)
                ]
                .spacing(8),
                mouse_area(
                    pick_list(
                        versions,
                        Some(self.source_version.clone()),
                        Message::SourceVersion
                    )
                    .width(Length::Fill)
                    .padding(10)
                    .style(|_, status| self.picker_style(status, 41))
                )
                .on_enter(Message::Hover(41, true))
                .on_exit(Message::Hover(41, false))
            ]
            .spacing(12);
            content = content.push(container(metadata).height(80.0 * amount).clip(true));
        }
        if let Some(path) = &self.source {
            content = content.push(
                tooltip(
                    self.label(shorten(&path.display().to_string(), 48)),
                    container(text(path.display().to_string()).size(12)).padding(10),
                    tooltip::Position::Bottom,
                )
                .style(move |_| hint_style(c)),
            );
        }
        if let Some(preview) = &self.sky_preview {
            content = content.push(
                column![
                    self.label(format!("天空纹理预览 · {} 层自动识别", self.sky_layers)),
                    widget::image(preview.clone())
                        .width(Length::Fill)
                        .height(160)
                        .content_fit(iced::ContentFit::Contain),
                    self.label("原始六面贴图 · 随目标版本自动转换")
                ]
                .spacing(8),
            );
        }
        self.panel(content)
    }

    fn memory_switch<'a>(
        &self,
        id: u8,
        label: &'a str,
        enabled: bool,
        amount: f32,
        message: Message,
        hint: &'a str,
    ) -> Element<'a, Message> {
        let c = self.colors();
        let hover = self
            .hover
            .get(&id)
            .map(|value| value.value(self.now))
            .unwrap_or(0.0);
        let content = row![
            text(label).size(12).color(c.foreground),
            space().width(Length::Fill),
            text(if enabled { "开" } else { "关" })
                .size(10)
                .color(mix(c.muted, c.accent, amount)),
            widget::canvas(AutoSaveSwitch {
                amount,
                hover,
                colors: c,
                disabled: self.busy(),
            })
            .width(58)
            .height(34)
        ]
        .spacing(5)
        .align_y(Alignment::Center);
        let control = button(content)
            .padding([4, 8])
            .width(Length::Fill)
            .on_press_maybe((!self.busy()).then_some(message))
            .style(move |_, status| {
                let disabled = status == button::Status::Disabled;
                button::Style {
                    background: Some(mix(c.field, c.accent, amount * 0.035 + hover * 0.045).into()),
                    text_color: c.foreground,
                    border: Border {
                        color: mix(c.line, c.accent, amount * 0.3 + hover * 0.35),
                        width: 1.0,
                        radius: 12.0.into(),
                    },
                    shadow: Shadow {
                        color: Color::from_rgba(
                            c.accent.r,
                            c.accent.g,
                            c.accent.b,
                            if disabled { 0.0 } else { hover * 0.045 },
                        ),
                        blur_radius: 10.0 * hover,
                        offset: Vector::ZERO,
                    },
                    ..Default::default()
                }
            });
        tooltip(
            mouse_area(control)
                .on_enter(Message::Hover(id, true))
                .on_exit(Message::Hover(id, false)),
            container(text(hint).size(12)).padding(9).max_width(320),
            tooltip::Position::Bottom,
        )
        .style(move |_| hint_style(c))
        .into()
    }

    fn memory_controls(&self) -> Element<'_, Message> {
        row![
            self.label("自动保存"),
            self.memory_switch(
                34,
                "版本",
                self.auto_save,
                self.auto_save_motion.value(self.now),
                Message::AutoSave(!self.auto_save),
                "记住目标版本。关闭后本次版本保留，重启时恢复自动推荐。",
            ),
            self.memory_switch(
                35,
                "导出目录",
                self.auto_save_path,
                self.auto_save_path_motion.value(self.now),
                Message::AutoSavePath(!self.auto_save_path),
                "只记住导出文件夹，每个包自动生成新文件名。关闭后重启恢复保存在源包旁。",
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    }

    fn target_panel(&self) -> Element<'_, Message> {
        let c = self.colors();
        let versions: Vec<_> = tables()
            .pack_formats
            .iter()
            .map(|(label, _)| label.clone())
            .collect();
        let mut output = text_input("留空时保存在源材质包旁", &self.export_directory_input)
            .padding(10)
            .size(13)
            .style(|_, status| self.input_style(status));
        if !self.busy() {
            output = output
                .on_input(Message::Output)
                .on_submit(Message::CommitExportDirectory);
        }
        let filename = if self.output.is_empty() {
            "文件名按版本和原包名自动生成".into()
        } else {
            format!(
                "将导出 {}",
                shorten(
                    &Path::new(&self.output)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    28,
                )
            )
        };
        let preview = tooltip(
            self.label(filename),
            container(
                text(if self.output.is_empty() {
                    "导入材质包后自动生成文件名"
                } else {
                    &self.output
                })
                .size(12),
            )
            .padding(9)
            .max_width(420),
            tooltip::Position::Bottom,
        )
        .style(move |_| hint_style(c));
        let content = column![
            self.title("转换目标", 14),
            self.label(
                if fast_pack::tables::version_profile(&self.target)
                    .is_some_and(|profile| profile.kind.starts_with("snapshot"))
                {
                    "目标游戏版本 · 26.4 当前为官方快照"
                } else {
                    "目标游戏版本"
                }
            ),
            row![
                pick_list(versions, Some(self.target.clone()), Message::Target)
                    .width(Length::Fill)
                    .padding(11)
                    .style(|_, status| self.picker_style(status, 40)),
                tooltip(
                    self.action(
                        5,
                        "⇄",
                        (!self.busy() && self.info.is_some()).then_some(Message::Swap),
                        false,
                    ),
                    container(text("交换源版本与目标版本").size(12)).padding(9),
                    tooltip::Position::Top,
                )
                .delay(Duration::from_millis(350))
                .style(move |_| hint_style(c)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            self.label("导出目录"),
            row![
                output,
                self.action(
                    6,
                    "选择",
                    (!self.busy()).then_some(Message::BrowseOutput),
                    false,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            preview,
            self.memory_controls(),
        ]
        .spacing(13);
        self.panel(content)
    }

    fn glint_controls(&self) -> Element<'_, Message> {
        let c = self.colors();
        let active = self.active_glint_color();
        let color = Color::from_rgb8(active[0], active[1], active[2]);
        let swatch = container(space().width(16).height(16)).style(move |_| container::Style {
            background: Some(color.into()),
            border: Border {
                radius: 5.0.into(),
                width: 1.0,
                color: c.line,
            },
            ..Default::default()
        });
        row![
            checkbox(self.custom_glint)
                .label("自定义附魔颜色")
                .size(17)
                .text_size(13)
                .on_toggle_maybe((!self.busy()).then_some(Message::CustomGlint)),
            swatch,
            self.label(if self.custom_glint {
                fast_pack::compat::glint::color_hex(self.glint_color)
            } else {
                "原版紫色".into()
            }),
            self.action(
                33,
                "调色",
                (!self.busy()).then_some(Message::GlintPicker),
                false
            ),
            space().width(Length::Fill)
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    }

    fn glint_dialog(&self) -> Element<'_, Message> {
        let c = self.colors();
        let presets = [
            ("白色", [255, 255, 255]),
            ("经典紫", fast_pack::compat::glint::VANILLA_COLOR),
            ("冰蓝", [91, 217, 255]),
            ("薄荷绿", [105, 239, 172]),
            ("金色", [255, 201, 92]),
            ("粉色", [255, 134, 204]),
            ("红色", [255, 86, 100]),
        ];
        let palette = row(presets.into_iter().map(|(name, color)| {
            let selected = self.active_glint_color() == color;
            let swatch = container(space().width(26).height(26)).style(move |_| container::Style {
                background: Some(Color::from_rgb8(color[0], color[1], color[2]).into()),
                border: Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
            let choice = button(swatch)
                .padding(5)
                .on_press(Message::GlintPreset(color))
                .style(move |_, status| button::Style {
                    background: Some(
                        if selected {
                            mix(c.field, c.accent, 0.12)
                        } else {
                            c.field
                        }
                        .into(),
                    ),
                    border: Border {
                        radius: 12.0.into(),
                        width: if selected || status == button::Status::Hovered {
                            2.0
                        } else {
                            1.0
                        },
                        color: if selected || status == button::Status::Hovered {
                            c.accent
                        } else {
                            c.line
                        },
                    },
                    ..Default::default()
                });
            tooltip(choice, text(name).size(12), tooltip::Position::Top)
                .style(move |_| hint_style(c))
                .into()
        }))
        .spacing(12);
        let mut controls = column![
            self.label("自定义色值"),
            text_input("#FFFFFF", &self.glint_hex)
                .on_input(Message::GlintHex)
                .padding(10)
                .size(15)
                .style(|_, status| self.input_style(status))
        ]
        .spacing(12);
        controls = controls.push(self.label(format!(
            "RGB  {} · {} · {}",
            self.active_glint_color()[0],
            self.active_glint_color()[1],
            self.active_glint_color()[2]
        )));
        let valid = fast_pack::compat::glint::parse_color(&self.glint_hex).is_ok();
        controls = controls.push(
            text(if valid {
                "颜色自动保存 · 仅本次启用"
            } else {
                "输入 3 位或 6 位 HEX 色值"
            })
            .size(12)
            .color(if valid { c.muted } else { c.danger }),
        );
        let color = Color {
            r: self.glint_color_motion[0].value(self.now),
            g: self.glint_color_motion[1].value(self.now),
            b: self.glint_color_motion[2].value(self.now),
            a: 1.0,
        };
        let preview = column![
            widget::canvas(GlintPreview {
                color,
                colors: c,
                elapsed: if self.reduced {
                    0.0
                } else {
                    self.now
                        .saturating_duration_since(self.glint_preview_started)
                        .as_secs_f32()
                },
            })
            .width(116)
            .height(108),
            self.label("颜色示意")
        ]
        .spacing(8)
        .align_x(Alignment::Center);
        container(
            self.panel(
                column![
                    row![
                        self.title("附魔闪烁颜色", 22),
                        space().width(Length::Fill),
                        self.action(34, "完成", Some(Message::CloseGlintPicker), true)
                    ]
                    .align_y(Alignment::Center),
                    self.label("选择喜欢的颜色；取消勾选自定义颜色恢复原版紫色。"),
                    palette,
                    row![
                        column![
                            widget::canvas(GlintColorCard {
                                hsv: self.glint_hsv,
                                colors: c
                            })
                            .width(272)
                            .height(228),
                            self.label("左侧选颜色与明暗 · 右侧切换色相")
                        ]
                        .spacing(8),
                        column![preview, controls].spacing(14).width(Length::Fill)
                    ]
                    .spacing(18)
                    .align_y(Alignment::Center),
                    checkbox(self.custom_glint)
                        .label("转换时应用自定义颜色")
                        .text_size(13)
                        .on_toggle(Message::CustomGlint)
                ]
                .spacing(18),
            ),
        )
        .width(self.width.min(640.0) - 56.0)
        .into()
    }

    fn advanced_view(&self) -> Element<'_, Message> {
        let amount = self.advanced_motion.value(self.now);
        if amount < 0.001 {
            return space().height(0).into();
        }
        let mut input = text_input("留空则沿用原包", &self.description)
            .padding(12)
            .size(14)
            .style(|_, status| self.input_style(status));
        if !self.busy() {
            input = input.on_input(Message::Description);
        }
        let lang = checkbox(self.preserve_lang)
            .label("保留语言格式")
            .size(17)
            .text_size(13)
            .on_toggle_maybe((!self.busy()).then_some(Message::PreserveLang));
        let case = checkbox(self.preserve_case)
            .label("保留文件名大小写")
            .size(17)
            .text_size(13)
            .on_toggle_maybe((!self.busy()).then_some(Message::PreserveCase));
        let reduced = checkbox(self.reduced)
            .label("减少动态效果")
            .size(17)
            .text_size(13)
            .on_toggle(Message::ReducedMotion);
        let zip = checkbox(self.as_zip)
            .label("输出为 ZIP")
            .size(17)
            .text_size(13)
            .on_toggle_maybe((!self.busy()).then_some(Message::Zip));
        let options: Element<'_, Message> = if self.width < 940.0 {
            column![zip, lang, case, reduced].spacing(12).into()
        } else {
            row![zip, lang, case, reduced].spacing(28).into()
        };
        let content = column![
            self.title("更多转换选项", 14),
            self.label("材质包描述"),
            input,
            options,
            self.glint_controls(),
            row![
                self.action(
                    8,
                    "导入自定义映射",
                    (!self.busy()).then_some(Message::Mapping),
                    false
                ),
                self.label(format!("{} 条映射", self.extra.len()))
            ]
            .spacing(14)
            .align_y(Alignment::Center)
        ]
        .spacing(14);
        container(self.panel(content))
            .height(if amount >= 0.999 {
                Length::Shrink
            } else {
                Length::Fixed((if self.width < 940.0 { 448.0 } else { 334.0 }) * amount)
            })
            .clip(true)
            .into()
    }

    fn result_view(&self) -> Element<'_, Message> {
        if self.phase != Phase::Done && self.error.is_none() {
            return space().height(0).into();
        }
        let c = self.colors();
        let amount = self
            .result_motion
            .value(self.now)
            .max(if self.error.is_some() { 1.0 } else { 0.0 });
        let failed = self.error.is_some();
        let tone = if failed { c.danger } else { c.success };
        let caption = if let Some(error) = &self.error {
            error.clone()
        } else {
            "原文件已保留，转换详情可在程序内查看。".into()
        };
        let icon = widget::canvas(Artwork {
            color: tone,
            kind: if failed {
                ArtworkKind::Error
            } else {
                ArtworkKind::Check
            },
        })
        .width(36)
        .height(38);
        let mut content = column![
            row![
                icon,
                column![
                    text(if failed {
                        "需要处理"
                    } else {
                        "转换完成"
                    })
                    .size(17)
                    .color(tone),
                    self.label(caption)
                ]
                .spacing(6)
            ]
            .spacing(14)
            .align_y(Alignment::Center)
        ]
        .spacing(16);
        if !failed {
            content = content.push(
                row![
                    self.action(20, "打开输出目录", Some(Message::OpenOutput), false),
                    self.action(21, "保存报告", Some(Message::SaveReport), false)
                ]
                .spacing(10),
            );
        }
        container(self.panel(content))
            .height(if failed {
                Length::Shrink
            } else {
                Length::Fixed(158.0 * amount)
            })
            .clip(true)
            .into()
    }

    fn view(&self) -> Element<'_, Message> {
        let c = self.colors();
        let logo = widget::image(brand_handle())
            .width(38)
            .height(38)
            .content_fit(iced::ContentFit::Contain);
        let header = container(
            row![
                logo,
                column![
                    self.title("Fast Pack", 18),
                    self.label("Minecraft Java 资源包转换")
                ]
                .spacing(3),
                space().width(Length::Fill),
                self.action(
                    10,
                    if self.dark { "浅色" } else { "深色" },
                    Some(Message::Theme),
                    false
                ),
                self.action(18, "了解详细", Some(Message::LearnMore), false)
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding([18, 28]);
        let mut intro = column![self.title(
            if self.phase == Phase::Done {
                "转换完成"
            } else {
                "材质包转换"
            },
            28
        )]
        .spacing(9);
        if self.phase == Phase::Done {
            intro = intro.push(self.label("转换结果已保存，原材质包没有改动。"));
        }
        let panels: Element<'_, Message> = if self.width < 940.0 {
            column![self.source_panel(), self.target_panel()]
                .spacing(18)
                .into()
        } else {
            row![
                container(self.source_panel()).width(Length::FillPortion(1)),
                container(self.target_panel()).width(Length::FillPortion(1))
            ]
            .spacing(20)
            .into()
        };
        let mut body = column![intro].spacing(20);
        if self.phase == Phase::Done {
            body = body.push(self.result_view()).push(
                self.panel(
                    column![
                        self.title("输出材质包", 14),
                        self.title(
                            Path::new(&self.output)
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy(),
                            18
                        ),
                        self.label(self.output.clone()),
                        self.label(format!("目标 Minecraft {}", self.target))
                    ]
                    .spacing(12),
                ),
            );
        } else {
            body = body
                .push(panels)
                .push(self.action(
                    7,
                    if self.advanced {
                        "收起转换选项 ↑"
                    } else {
                        "更多转换选项 ↓"
                    },
                    Some(Message::Advanced),
                    false,
                ))
                .push(self.advanced_view());
            if self.error.is_some() {
                body = body.push(self.result_view());
            }
        }
        let fixed_progress: Element<'_, Message> = if self.busy() {
            let value = self
                .progress
                .value(self.now)
                .min(self.actual_progress as f32);
            let progress = progress_bar(0.0..=100.0, value).girth(6).style(move |_| {
                widget::progress_bar::Style {
                    background: c.line.into(),
                    bar: c.accent.into(),
                    border: Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                }
            });
            container(
                column![
                    row![
                        self.label(&self.step),
                        space().width(Length::Fill),
                        self.label(format!("{}%", value.round() as u8))
                    ]
                    .align_y(Alignment::Center),
                    progress
                ]
                .spacing(8)
                .width(Length::Fill),
            )
            .padding([10, 28])
            .width(Length::Fill)
            .into()
        } else {
            space().height(0).into()
        };
        let status = match self.phase {
            Phase::Empty => "数据仅在本机处理",
            Phase::Identifying => "正在后台识别，你仍可操作界面",
            Phase::Ready => "准备就绪 · 将生成新的材质包",
            Phase::Running => {
                if self.closing {
                    "完成后关闭窗口"
                } else {
                    "转换在后台进行 · 原文件保留"
                }
            }
            Phase::Done => "转换结果已保存",
            Phase::Failed => "请检查输入或输出路径",
        };
        let action = if self.phase == Phase::Done {
            self.action(12, "转换另一个包", Some(Message::NewPack), true)
        } else {
            self.action(
                12,
                if self.busy() {
                    "正在转换…"
                } else if self.phase == Phase::Identifying {
                    "正在识别…"
                } else {
                    "开始转换 →"
                },
                (self.phase == Phase::Ready
                    || self.phase == Phase::Failed
                        && self.info.as_ref().is_some_and(|value| value.ok))
                .then_some(Message::Start),
                true,
            )
        };
        let footer = container(
            row![self.label(status), space().width(Length::Fill), action]
                .align_y(Alignment::Center),
        )
        .padding([18, 28]);
        if !self.logs.is_empty() {
            body = body.push(self.action(
                13,
                if self.details {
                    "收起转换详情 ↑"
                } else {
                    "转换详情 ↓"
                },
                Some(Message::Details),
                false,
            ));
        }
        if self.details {
            let log = self.logs.join("\n");
            let mut entries = column![self.label(log)].spacing(12);
            if let Some(report) = &self.report {
                entries = entries.push(self.label(report));
            }
            entries = entries.push(self.action(14, "清空日志", Some(Message::ClearLogs), false));
            body = body.push(self.panel(entries));
        }
        let main = container(
            scrollable(container(body).padding(Padding {
                top: 24.0,
                right: 28.0,
                bottom: 28.0,
                left: 28.0,
            }))
            .id("fast-pack-content"),
        )
        .width(Length::Fill)
        .height(Length::Fill);
        let base: Element<'_, Message> = container(column![
            header,
            widget::rule::horizontal(1),
            main,
            widget::rule::horizontal(1),
            fixed_progress,
            footer
        ])
        .height(Length::Fill)
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(c.background.into()),
            text_color: Some(c.foreground),
            ..Default::default()
        })
        .into();
        if self.glint_picker || self.glint_picker_motion.value(self.now) > 0.001 {
            let amount = self.glint_picker_motion.value(self.now);
            let overlay = container(self.glint_dialog())
                .padding(Padding {
                    top: (1.0 - amount) * 24.0,
                    ..Padding::ZERO
                })
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55 * amount).into()),
                    ..Default::default()
                });
            widget::stack![base, widget::opaque(overlay)].into()
        } else if self.learn_more {
            let c = self.colors();
            let names = ["ColdEternity Team", "Long_Cold", "Wind_"];
            let developers = row(names.into_iter().enumerate().map(|(index, name)| {
                container(
                    column![
                        widget::image(developer_handle(index))
                            .width(70)
                            .height(70)
                            .content_fit(iced::ContentFit::Contain),
                        self.title(name, 14)
                    ]
                    .spacing(10)
                    .align_x(Alignment::Center),
                )
                .width(Length::FillPortion(1))
                .into()
            }))
            .spacing(16);
            let dialog=container(self.panel(column![
                row![self.title("了解 Fast Pack",24),space().width(Length::Fill),self.action(19,"关闭",Some(Message::CloseLearnMore),false)].align_y(Alignment::Center),
                self.label(format!("版本 {} · 免费的 Minecraft Java 材质包转换工具",env!("CARGO_PKG_VERSION"))),
                self.title("开发者",14),developers,
                widget::rule::horizontal(1),
                self.title("开发交流群",14),
                text("QQ：663420633").size(17).color(c.foreground),
                text("进群答案 / 密码：快手/B站/抖音").size(15).color(c.foreground),
                self.title("源码地址",14),
                text("https://github.com/LongCold-ColdEternity-Team/Fast-Pack").size(13).color(c.accent),
                self.action(20,"打开源码仓库",Some(Message::OpenSource),true),
                self.label("ColdEternity Team Source-Available License 1.0"),
                self.label("允许免费非商业二改；衍生可执行文件必须同步公开完整源码并保留作者、许可证和原始仓库信息。")
            ].spacing(16))).width(680);
            let overlay = container(dialog)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
                    ..Default::default()
                });
            widget::stack![base, widget::opaque(overlay)].into()
        } else if self.confirm_overwrite {
            let dialog = container(
                self.panel(
                    column![
                        self.title("替换已有输出？", 20),
                        self.label("已有结果将被本次转换替换，源材质包不会更改。"),
                        self.label(shorten(&self.output, 58)),
                        row![
                            self.action(31, "返回", Some(Message::CancelOverwrite), false),
                            self.action(32, "覆盖并转换", Some(Message::ConfirmOverwrite), true)
                        ]
                        .spacing(12)
                    ]
                    .spacing(20),
                ),
            )
            .width(480);
            let overlay = container(dialog)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
                    ..Default::default()
                });
            widget::stack![base, widget::opaque(overlay)].into()
        } else {
            base
        }
    }
}

struct GlintColorCard {
    hsv: [f32; 3],
    colors: Colors,
}

#[derive(Default)]
struct GlintColorCardState {
    dragging_hue: Option<bool>,
}

impl GlintColorCard {
    fn regions(size: Size) -> (iced::Rectangle, iced::Rectangle) {
        let side = (size.height - 12.0).min(size.width - 52.0).max(1.0);
        (
            iced::Rectangle {
                x: 6.0,
                y: 6.0,
                width: side,
                height: side,
            },
            iced::Rectangle {
                x: side + 20.0,
                y: 6.0,
                width: 24.0,
                height: side,
            },
        )
    }
}

impl widget::canvas::Program<Message> for GlintColorCard {
    type State = GlintColorCardState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &widget::canvas::Event,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<widget::canvas::Action<Message>> {
        use iced::mouse::{Button, Event};
        let (square, hue_bar) = Self::regions(bounds.size());
        match event {
            widget::canvas::Event::Mouse(Event::ButtonPressed(Button::Left)) => {
                let point = cursor.position_in(bounds)?;
                state.dragging_hue = if square.contains(point) {
                    Some(false)
                } else if hue_bar.contains(point) {
                    Some(true)
                } else {
                    return None;
                };
            }
            widget::canvas::Event::Mouse(Event::ButtonReleased(Button::Left)) => {
                return state
                    .dragging_hue
                    .take()
                    .map(|_| widget::canvas::Action::capture());
            }
            widget::canvas::Event::Mouse(Event::CursorMoved { .. })
                if state.dragging_hue.is_some() => {}
            _ => return None,
        }
        let point = cursor.position()? - Vector::new(bounds.x, bounds.y);
        let message = if state.dragging_hue? {
            Message::GlintHue(1.0 - ((point.y - hue_bar.y) / hue_bar.height).clamp(0.0, 1.0))
        } else {
            Message::GlintSaturationValue(
                ((point.x - square.x) / square.width).clamp(0.0, 1.0),
                1.0 - ((point.y - square.y) / square.height).clamp(0.0, 1.0),
            )
        };
        Some(widget::canvas::Action::publish(message).and_capture())
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        let (square, hue_bar) = Self::regions(bounds.size());
        let point = cursor.position_in(bounds);
        if state.dragging_hue.is_some() || point.is_some_and(|point| square.contains(point)) {
            iced::mouse::Interaction::Crosshair
        } else if point.is_some_and(|point| hue_bar.contains(point)) {
            iced::mouse::Interaction::Pointer
        } else {
            iced::mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        _: &Self::State,
        renderer: &iced::Renderer,
        _: &Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<widget::canvas::Geometry> {
        use widget::canvas::{Frame, Path, Stroke, gradient};
        let mut frame = Frame::new(renderer, bounds.size());
        let (square, hue_bar) = Self::regions(bounds.size());
        let [red, green, blue] = hsv_rgb([self.hsv[0], 1.0, 1.0]);
        let pure = Color::from_rgb8(red, green, blue);
        let square_path = Path::rounded_rectangle(square.position(), square.size(), 3.0.into());
        frame.fill(
            &square_path,
            gradient::Linear::new(
                square.position(),
                iced::Point::new(square.x + square.width, square.y),
            )
            .add_stop(0.0, Color::WHITE)
            .add_stop(1.0, pure),
        );
        frame.fill(
            &square_path,
            gradient::Linear::new(
                square.position(),
                iced::Point::new(square.x, square.y + square.height),
            )
            .add_stop(0.0, Color::TRANSPARENT)
            .add_stop(1.0, Color::BLACK),
        );
        frame.stroke(
            &square_path,
            Stroke::default()
                .with_color(self.colors.line)
                .with_width(1.0),
        );
        let bar_path = Path::rounded_rectangle(hue_bar.position(), hue_bar.size(), 3.0.into());
        let mut spectrum = gradient::Linear::new(
            hue_bar.position(),
            iced::Point::new(hue_bar.x, hue_bar.y + hue_bar.height),
        );
        for index in 0..=6 {
            let amount = index as f32 / 6.0;
            let [red, green, blue] = hsv_rgb([1.0 - amount, 1.0, 1.0]);
            spectrum = spectrum.add_stop(amount, Color::from_rgb8(red, green, blue));
        }
        frame.fill(&bar_path, spectrum);
        frame.stroke(
            &bar_path,
            Stroke::default()
                .with_color(self.colors.line)
                .with_width(1.0),
        );
        let marker = iced::Point::new(
            square.x + self.hsv[1] * square.width,
            square.y + (1.0 - self.hsv[2]) * square.height,
        );
        let marker_path = Path::circle(marker, 5.5);
        let [red, green, blue] = hsv_rgb(self.hsv);
        frame.fill(&marker_path, Color::from_rgb8(red, green, blue));
        frame.stroke(
            &marker_path,
            Stroke::default()
                .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.7))
                .with_width(4.0),
        );
        frame.stroke(
            &marker_path,
            Stroke::default().with_color(Color::WHITE).with_width(2.0),
        );
        let hue_marker = Path::rectangle(
            iced::Point::new(
                hue_bar.x - 3.0,
                hue_bar.y + (1.0 - self.hsv[0]) * hue_bar.height - 2.0,
            ),
            Size::new(hue_bar.width + 6.0, 4.0),
        );
        frame.stroke(
            &hue_marker,
            Stroke::default()
                .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.7))
                .with_width(4.0),
        );
        frame.stroke(
            &hue_marker,
            Stroke::default().with_color(Color::WHITE).with_width(2.0),
        );
        vec![frame.into_geometry()]
    }
}

struct GlintPreview {
    color: Color,
    colors: Colors,
    elapsed: f32,
}

impl widget::canvas::Program<Message> for GlintPreview {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<widget::canvas::Geometry> {
        use widget::canvas::{Frame, Path};
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill(
            &Path::rounded_rectangle(iced::Point::ORIGIN, bounds.size(), 12.0.into()),
            self.colors.field,
        );
        let unit = bounds.width.min(bounds.height) / 24.0;
        for y in 0_i32..20 {
            for x in 0_i32..20 {
                let diagonal = x + y;
                let blade =
                    (8..=18).contains(&x) && (1..=11).contains(&y) && (18..=20).contains(&diagonal);
                let guard =
                    (5..=11).contains(&x) && (9..=15).contains(&y) && (-5..=-3).contains(&(x - y));
                let handle =
                    (3..=7).contains(&x) && (13..=17).contains(&y) && (19..=20).contains(&diagonal);
                let pommel = (2..=4).contains(&x) && (16..=18).contains(&y);
                if !(blade || guard || handle || pommel) {
                    continue;
                }
                let phase = x as f32 * 0.48 + y as f32 * 0.24 - self.elapsed * 2.8;
                let shine = ((phase.sin() + 1.0) * 0.5).powi(3);
                let base = if handle || pommel {
                    rgb(0x667785)
                } else {
                    rgb(0xa4b9c7)
                };
                let color = mix(base, self.color, 0.35 + shine * 0.65);
                frame.fill_rectangle(
                    iced::Point::new((x + 2) as f32 * unit, (y + 2) as f32 * unit),
                    Size::new(unit + 0.2, unit + 0.2),
                    color,
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

struct AutoSaveSwitch {
    amount: f32,
    hover: f32,
    colors: Colors,
    disabled: bool,
}

impl widget::canvas::Program<Message> for AutoSaveSwitch {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<widget::canvas::Geometry> {
        use widget::canvas::{Frame, Path, Stroke};
        let mut frame = Frame::new(renderer, bounds.size());
        let track = Path::rounded_rectangle(
            iced::Point::new(4.0, 4.0),
            Size::new(50.0, 26.0),
            13.0.into(),
        );
        let track_color = mix(self.colors.line, self.colors.accent, self.amount);
        frame.fill(
            &track,
            if self.disabled {
                mix(track_color, self.colors.background, 0.5)
            } else {
                track_color
            },
        );
        frame.stroke(
            &track,
            Stroke::default()
                .with_color(mix(self.colors.line, self.colors.accent, self.hover * 0.5))
                .with_width(1.0),
        );
        let center = iced::Point::new(17.0 + self.amount * 24.0, 17.0);
        let radius = 9.5 + if self.disabled { 0.0 } else { self.hover * 0.5 };
        frame.fill(
            &Path::circle(iced::Point::new(center.x, center.y + 1.0), radius + 0.5),
            Color::from_rgba(0.0, 0.0, 0.0, 0.13),
        );
        frame.fill(
            &Path::circle(center, radius),
            mix(
                self.colors.foreground,
                Color::WHITE,
                if self.disabled { 0.4 } else { 0.94 },
            ),
        );
        vec![frame.into_geometry()]
    }
}

#[derive(Clone, Copy)]
enum ArtworkKind {
    Package,
    Check,
    Error,
}
struct Artwork {
    color: Color,
    kind: ArtworkKind,
}

impl widget::canvas::Program<Message> for Artwork {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<widget::canvas::Geometry> {
        use widget::canvas::{Frame, Path, Stroke};
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;
        let path = Path::new(|builder| match self.kind {
            ArtworkKind::Package => {
                builder.move_to(iced::Point::new(w * 0.5, h * 0.08));
                builder.line_to(iced::Point::new(w * 0.91, h * 0.29));
                builder.line_to(iced::Point::new(w * 0.91, h * 0.71));
                builder.line_to(iced::Point::new(w * 0.5, h * 0.94));
                builder.line_to(iced::Point::new(w * 0.09, h * 0.71));
                builder.line_to(iced::Point::new(w * 0.09, h * 0.29));
                builder.close();
                builder.move_to(iced::Point::new(w * 0.09, h * 0.29));
                builder.line_to(iced::Point::new(w * 0.5, h * 0.51));
                builder.line_to(iced::Point::new(w * 0.91, h * 0.29));
                builder.move_to(iced::Point::new(w * 0.5, h * 0.51));
                builder.line_to(iced::Point::new(w * 0.5, h * 0.94));
                builder.move_to(iced::Point::new(w * 0.3, h * 0.18));
                builder.line_to(iced::Point::new(w * 0.7, h * 0.41));
            }
            ArtworkKind::Check => {
                builder.move_to(iced::Point::new(w * 0.16, h * 0.52));
                builder.line_to(iced::Point::new(w * 0.42, h * 0.77));
                builder.line_to(iced::Point::new(w * 0.88, h * 0.21));
            }
            ArtworkKind::Error => {
                builder.move_to(iced::Point::new(w * 0.5, h * 0.15));
                builder.line_to(iced::Point::new(w * 0.5, h * 0.65));
                builder.move_to(iced::Point::new(w * 0.5, h * 0.78));
                builder.line_to(iced::Point::new(w * 0.5, h * 0.85));
            }
        });
        frame.stroke(
            &path,
            Stroke::default().with_color(self.color).with_width(2.5),
        );
        vec![frame.into_geometry()]
    }
}

fn shorten(value: &str, maximum: usize) -> String {
    let chars: Vec<_> = value.chars().collect();
    if chars.len() <= maximum {
        return value.into();
    }
    format!(
        "{}…{}",
        chars[..maximum * 2 / 3].iter().collect::<String>(),
        chars[chars.len() - maximum / 3..]
            .iter()
            .collect::<String>()
    )
}

fn system_reduced_motion() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
    };
    let mut enabled = 1i32;
    unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut enabled as *mut i32).cast(),
            0,
        );
    }
    enabled == 0
}

fn save_screenshot(path: &Path, screenshot: &window::Screenshot) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(file, screenshot.size.width, screenshot.size.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&screenshot.rgba)?;
    Ok(())
}

fn app_icon() -> Option<window::Icon> {
    let bitmap = image::load_from_memory(include_bytes!("../assets/app.png"))
        .ok()?
        .to_rgba8();
    let bitmap = image::imageops::resize(&bitmap, 64, 64, image::imageops::FilterType::Lanczos3);
    window::icon::from_rgba(bitmap.into_raw(), 64, 64).ok()
}

fn brand_handle() -> widget::image::Handle {
    static BRAND: std::sync::LazyLock<widget::image::Handle> = std::sync::LazyLock::new(|| {
        let bitmap = image::load_from_memory(include_bytes!("../assets/app.png"))
            .expect("内置 logo 无效")
            .to_rgba8();
        widget::image::Handle::from_rgba(bitmap.width(), bitmap.height(), bitmap.into_raw())
    });
    BRAND.clone()
}

fn developer_handle(index: usize) -> widget::image::Handle {
    static DEVELOPERS: std::sync::LazyLock<[widget::image::Handle; 3]> =
        std::sync::LazyLock::new(|| {
            [
                include_bytes!("../assets/developers/coldeternity-team.png").as_slice(),
                include_bytes!("../assets/developers/long-cold.png").as_slice(),
                include_bytes!("../assets/developers/wind.png").as_slice(),
            ]
            .map(|bytes| {
                let bitmap = image::load_from_memory(bytes)
                    .expect("开发者头像无效")
                    .to_rgba8();
                widget::image::Handle::from_rgba(bitmap.width(), bitmap.height(), bitmap.into_raw())
            })
        });
    DEVELOPERS[index].clone()
}

pub fn run() -> Result<()> {
    let smoke = std::env::args().any(|value| value == "--smoke-test");
    let size = std::env::var("FAST_PACK_CAPTURE_SIZE")
        .ok()
        .and_then(|value| {
            value
                .split_once('x')
                .map(|(w, h)| (w.parse::<f32>().ok(), h.parse::<f32>().ok()))
        })
        .and_then(|(w, h)| Some(Size::new(w?, h?)))
        .unwrap_or_else(|| Size::new(1040.0, 780.0));
    let settings = window::Settings {
        size,
        min_size: Some(Size::new(800.0, 680.0)),
        position: window::Position::Centered,
        visible: !smoke,
        icon: app_icon(),
        exit_on_close_request: false,
        ..Default::default()
    };
    iced::application(App::boot, App::update, App::view)
        .title("Fast Pack")
        .theme(App::theme)
        .subscription(App::subscription)
        .window(settings)
        .default_font(Font::with_name(crate::windows_compat::font_family()))
        .antialiasing(true)
        .run()
        .context("无法启动 Fast Pack 界面")
}

pub fn show_error(value: &str) {
    rfd::MessageDialog::new()
        .set_title("Fast Pack")
        .set_description(value)
        .set_level(rfd::MessageLevel::Error)
        .show();
}

#[cfg(test)]
#[path = "gui_tests.rs"]
mod tests;
