use crate::motion::Tween;
use anyhow::{Context, Result};
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
    SourceVersion(String),
    Swap,
    Output(String),
    BrowseOutput,
    OutputPicked(Option<PathBuf>),
    Zip(bool),
    Description(String),
    PreserveLang(bool),
    PreserveCase(bool),
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

struct App {
    phase: Phase,
    source: Option<PathBuf>,
    info: Option<PackInfo>,
    sky_preview: Option<widget::image::Handle>,
    sky_layers: usize,
    source_version: String,
    target: String,
    output: String,
    description: String,
    as_zip: bool,
    preserve_lang: bool,
    preserve_case: bool,
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
    hover: HashMap<u8, Tween>,
    width: f32,
    generation: u64,
    sender: Sender<WorkEvent>,
    receiver: Receiver<WorkEvent>,
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
        let mut app = Self {
            phase: Phase::Empty,
            source: None,
            info: None,
            sky_preview: None,
            sky_layers: 0,
            source_version: "自动检测".into(),
            target: "26.2".into(),
            output: String::new(),
            description: String::new(),
            as_zip: true,
            preserve_lang: false,
            preserve_case: false,
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
            hover: HashMap::new(),
            width: 1040.0,
            generation: 0,
            sender,
            receiver,
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
        ]
        .into_iter()
        .any(|value| value.active(self.now))
            || self.hover.values().any(|value| value.active(self.now))
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
            || self.capture.is_some()
            || self.smoke
        {
            let duration = if self.animation_active() { 16 } else { 40 };
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
        std::thread::spawn(move || {
            let info = probe_pack(&path);
            let has_sky = info.features.sky_count() > 0;
            let _ = sender.send(WorkEvent::Probed(generation, info));
            if has_sky {
                let preview =
                    fast_pack::compat::sky::preview(&path).map_err(|error| format!("{error:#}"));
                let _ = sender.send(WorkEvent::Preview(generation, preview));
            }
        });
    }
    fn update_output(&mut self) {
        if let Some(source) = &self.source {
            self.output = suggest_output(source, &self.target, self.as_zip)
                .display()
                .to_string();
        }
    }
    fn log(&mut self, value: String) {
        if self.logs.len() >= 500 {
            self.logs.drain(..100);
        }
        self.logs.push(value);
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
                let mut reset_scroll = false;
                let events: Vec<_> = self.receiver.try_iter().collect();
                for event in events {
                    match event {
                        WorkEvent::Probed(id, info) if id == self.generation => {
                            if info.ok {
                                self.target = info.suggested_target.clone();
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
                            if self.closing || (self.smoke && self.capture.is_none()) {
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
                    if self.busy() {
                        self.closing = true;
                        self.log("转换完成并清理临时文件后关闭窗口".into());
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
                self.target = target;
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
            Message::Output(output) if !self.busy() => self.output = output,
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
                    self.update_output();
                } else {
                    self.error = Some("请先指定具体源版本".into());
                }
            }
            Message::BrowseOutput if !self.busy() => {
                let zip = self.as_zip;
                let name = Path::new(&self.output)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                return Task::perform(
                    async move {
                        let dialog = rfd::AsyncFileDialog::new().set_title("Fast Pack · 输出位置");
                        if zip {
                            dialog
                                .set_file_name(name)
                                .add_filter("ZIP", &["zip"])
                                .save_file()
                                .await
                                .map(|file| file.path().to_owned())
                        } else {
                            dialog
                                .pick_folder()
                                .await
                                .map(|file| file.path().to_owned())
                        }
                    },
                    Message::OutputPicked,
                );
            }
            Message::OutputPicked(Some(path)) if !self.busy() => {
                self.output = path.display().to_string()
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
            Message::CancelOverwrite => self.confirm_overwrite = false,
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
                        color: Color::TRANSPARENT,
                        offset: Vector::ZERO,
                        blur_radius: 0.0,
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
    fn picker_style(&self, status: widget::pick_list::Status) -> widget::pick_list::Style {
        let c = self.colors();
        let active = matches!(
            status,
            widget::pick_list::Status::Opened { .. } | widget::pick_list::Status::Hovered
        );
        widget::pick_list::Style {
            text_color: c.foreground,
            placeholder_color: c.muted,
            handle_color: c.muted,
            background: c.field.into(),
            border: Border {
                radius: 10.0.into(),
                width: 1.0,
                color: if active { c.accent } else { c.line },
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
                ..Font::with_name("Microsoft YaHei UI")
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
                pick_list(
                    versions,
                    Some(self.source_version.clone()),
                    Message::SourceVersion
                )
                .width(Length::Fill)
                .padding(10)
                .style(|_, status| self.picker_style(status))
            ]
            .spacing(12);
            content = content.push(container(metadata).height(80.0 * amount).clip(true));
        }
        if let Some(path) = &self.source {
            content = content.push(tooltip(
                self.label(shorten(&path.display().to_string(), 48)),
                container(text(path.display().to_string()).size(12)).padding(10),
                tooltip::Position::Bottom,
            ));
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

    fn target_panel(&self) -> Element<'_, Message> {
        let c = self.colors();
        let versions: Vec<_> = tables()
            .pack_formats
            .iter()
            .map(|(label, _)| label.clone())
            .collect();
        let source = self
            .info
            .as_ref()
            .and_then(|info| info.version_label.clone())
            .unwrap_or_else(|| "待导入".into());
        let route = row![
            column![
                self.label("源版本"),
                self.title(
                    if self.source_version == "自动检测" {
                        source
                    } else {
                        self.source_version.clone()
                    },
                    22
                )
            ]
            .spacing(6),
            space().width(Length::Fill),
            self.action(
                5,
                "⇄",
                (!self.busy() && self.info.is_some()).then_some(Message::Swap),
                false
            ),
            space().width(Length::Fill),
            column![
                self.label("目标版本"),
                text(&self.target).size(22).color(c.accent)
            ]
            .spacing(6)
        ]
        .align_y(Alignment::Center);
        let mut output = text_input("选择转换结果的保存路径", &self.output)
            .padding(12)
            .size(13)
            .style(|_, status| self.input_style(status));
        if !self.busy() {
            output = output.on_input(Message::Output);
        }
        let zip = checkbox(self.as_zip)
            .label("打包为 ZIP 压缩包")
            .size(17)
            .text_size(13)
            .on_toggle_maybe((!self.busy()).then_some(Message::Zip));
        let content = column![
            self.title("转换目标", 14),
            self.label("目标游戏版本"),
            pick_list(versions, Some(self.target.clone()), Message::Target)
                .width(Length::Fill)
                .padding(12)
                .style(|_, status| self.picker_style(status)),
            route,
            self.label("输出位置"),
            row![
                output,
                self.action(
                    6,
                    "浏览",
                    (!self.busy()).then_some(Message::BrowseOutput),
                    false
                )
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            zip,
            self.label("实体、模型、GUI、字体与天空自动适配")
        ]
        .spacing(16);
        self.panel(content)
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
        let options: Element<'_, Message> = if self.width < 940.0 {
            column![lang, case, reduced].spacing(12).into()
        } else {
            row![lang, case, reduced].spacing(28).into()
        };
        let content = column![
            self.title("更多转换选项", 14),
            self.label("材质包描述"),
            input,
            options,
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
            .height((if self.width < 940.0 { 350.0 } else { 272.0 }) * amount)
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
        let intro = column![
            self.title(
                if self.phase == Phase::Done {
                    "你的材质包已准备好。"
                } else {
                    "材质包，换个版本。"
                },
                28
            ),
            self.label(if self.phase == Phase::Done {
                "转换结果已保存，原材质包没有改动。"
            } else {
                "导入材质包，选好目标版本，即可开始转换。"
            })
        ]
        .spacing(9);
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
        if self.busy() {
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
            body = body.push(
                column![
                    row![
                        self.label(&self.step),
                        space().width(Length::Fill),
                        self.label(format!("{}%", value.round() as u8))
                    ],
                    progress
                ]
                .spacing(12),
            );
        }
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
        if self.learn_more {
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
        .default_font(Font::with_name("Microsoft YaHei UI"))
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
