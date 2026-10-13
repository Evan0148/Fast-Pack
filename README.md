<div align="center">
  <img src="assets/branding/fast-pack-logo.png" width="120" alt="Fast Pack Logo">
  <h1>Fast Pack</h1>
  <p><strong>让材质包跟上你的 Minecraft 版本。</strong></p>
  <p>纯 Rust · 多版本自动适配 · 本地处理 · 免费非商业使用</p>

  <a href="https://github.com/LongCold-ColdEternity-Team/Fast-Pack/releases/latest"><img src="https://img.shields.io/github/v/release/LongCold-ColdEternity-Team/Fast-Pack?style=flat-square&color=087b91" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-Source--Available-087b91?style=flat-square" alt="Source-Available License"></a>
  <a href="https://github.com/LongCold-ColdEternity-Team/Fast-Pack/stargazers"><img src="https://img.shields.io/github/stars/LongCold-ColdEternity-Team/Fast-Pack?style=flat-square&color=70d9ee" alt="Stars"></a>
</div>

Fast Pack 是面向 Minecraft Java Edition 的材质包版本转换工具，由 **ColdEternity Team、Long_Cold、Wind_** 开发。导入 ZIP 或已解压的材质包，选择目标版本后，程序自动处理资源目录、纹理命名、实体 UV、模型与方块状态、GUI 图集、字体、天空以及材质包内的渲染资源兼容规则。

**原始仓库：[LongCold-ColdEternity-Team/Fast-Pack](https://github.com/LongCold-ColdEternity-Team/Fast-Pack)**  
**下载成品：[GitHub Releases](https://github.com/LongCold-ColdEternity-Team/Fast-Pack/releases/latest)**

## 项目特点

- **多版本自动适配。** 覆盖 1.8.9 至 26.2 的 29 个版本标签，支持升级、降级和同版本整理。目标版本的规则自动执行，常见布局无需玩家补文件、改路径或调整透明度。
- **按渲染代际处理资源。** 识别材质包中的核心与后处理着色器，优先匹配包内的目标版本变体；对已识别的引用规则进行处理，跨代且无可确认兼容变体的覆盖自动备份回退。程序会明确记录原效果未被保留的情况。
- **天空资源跨格式迁移。** 自动处理 MCPatcher、OptiFine 的目录与 `source` 引用，重排六面贴图朝向，并支持已识别的静态 FSB / Nuit 天空结构与反向转换。旧包转到新版本时，天空资源也参与转换。
- **实体贴图按像素重建。** 箱子 UV 重排、双箱左右拆分、旧船到新船的纹理映射、旗帜和盾牌透明掩码转换都由程序执行，支持标准布局的高清比例。
- **GUI 与字体一起迁移。** 根据 88 条精灵配方拆分或合并 GUI 图集，生成窄版效果列表，适配游戏模式槽位与附魔光效，处理旧 Unicode 字体页。
- **自定义附魔闪烁颜色。** 在“更多转换选项”中选择白色、经典紫、冰蓝、薄荷绿、金色、粉色、红色，也可以输入任意 HEX 色值或拖动方形调色卡。调色面板采用方形饱和度 / 明暗卡与竖向色相条，提供流动光效示意，颜色与开关自动保存；跨版本转换自动处理旧版渲染器的紫色染色，保留原图案、透明度与动画元数据。
- **旧版工具与装备保留材质。** 对照官方资源补齐 227 个旧版物品纹理名称，递归处理嵌套贴图与模型引用，区分方块和物品的映射。支持木质工具、弓、鱼竿、染料、药水、唱片等旧名称，以及弓 / 鱼竿状态和时钟 / 罗盘帧。
- **只有贴图的旧包自动补模型。** 转到 1.13–1.21.3 时，为上传包中已有的物品贴图补齐缺失模型，明确绑定装备、工具、弓、鱼竿等资源；1.20 起同时补齐相应盔甲纹饰模型。参考表包含 254 个基础物品模型与 200 个纹饰变体，已有自定义模型优先保留。
- **补齐遗漏的旧方块名称。** 增加 147 条经官方资源核对的别名，处理铁砧、门、轨道、植物生长阶段、火焰、玻璃板、红石和其他旧路径。新版目录中仍残留旧名称的转换结果也会自动整理，并同步 PNG 动画元数据及模型纹理引用。
- **冲突时保留旧版实际使用的原图。** 旧包同时包含已知的旧名称和新版名称时，优先迁移旧版标准路径中的贴图，并备份被替换的重复文件；原模型明确引用的目标贴图优先保留，避免覆盖作者指定的资源。
- **旧版自定义手持姿态自动换算。** 对照官方 1.8.9 与 1.20.1 模型提取 254 个平面物品模型的 8 组姿态基准，换算第一 / 第三人称的旋转、位移、缩放和左右手姿态，保留作者相对基准设置的大小。带有 Fast Pack 原模型备份的旧转换结果可自动修复，避免剑竖直、过大及左手姿态错误。
- **新版手持模型降级。** 降到 1.8.9 时，递归展开平面物品的父模型继承，合并实际生效的姿态与贴图引用，再按旧版基准换算。没有单独写 `display` 的剑、工具与食物也会补齐姿态；1.21.4+ 已识别的默认物品定义重新绑定到旧版模型入口。
- **创造背包与药水图标还原。** 按目标版本重建标签页：1.8.9–1.19.2 使用六列 28px，1.19.3–1.20.1 使用七列 26px，同时合并滚动条。识别误切到独立精灵里的红蓝 `SEL` 占位区，从同组干净边框重建完整标签页；1.8.9 的创造人物窗口和装备格按旧坐标重定位。独立药水效果图标写回旧版背包图集，缺失且原图集未提供的符号自动补齐。
- **方向方块降级。** 发射器和投掷器的六种朝向还原为旧版状态引用，补齐缺失的水平 / 垂直模型，并保留垂直纹理名称；已识别的单模型 multipart 分支会转换到旧版 variants。
- **背包合成区坐标迁移。** 1.8.9 的输入格、箭头、结果格随新版布局整体右移 10px、上移 8px；人物背景框收窄 3px，补副手槽与配方书按钮，按贴图比例处理高清资源。
- **现代桌面体验。** 深色 / 浅色主题、拖放导入、后台识别与转换、真实进度、天空贴图预览，以及约 150–280ms 的界面过渡。支持减少动态效果。
- **紧凑的自动保存开关。** 主页将“版本 / 导出目录”两个开关排在同一行，分别控制版本记忆与导出文件夹记忆；滑块、轨道颜色及悬停效果平滑过渡。设置位于 Windows 用户的 `%LOCALAPPDATA%\Fast Pack\settings.json`，程序目录和材质包目录无需配置文件。
- **导出到固定文件夹。** 选好导出目录后，后续导入、切换版本和重新打开程序都会继续导出到该目录，每个包使用自己的 `[目标版本]原包名.zip`，不复用上一次的文件名。只保存导出目录，不记录导入路径。
- **保留输入原包。** 转换在临时工作副本中完成，结果另存。改写的资源会在输出包内保留原始备份，帮助后续转换恢复内容。
- **输出 ZIP 更干净。** v1.1.0 起不再向材质包内加入 `转换报告.txt`；输入包已有的同名根目录报告也会从新输出中移除。转换详情仍可在程序内查看或另存。
- **GUI、CLI 和 Rust 库共用转换逻辑。** 桌面玩家可以直接操作界面，开发者也可以批量调用命令行或集成转换库。

Fast Pack 的兼容规则负责转换渲染资源。自定义天空的显示取决于客户端已有的天空渲染能力；未知 GLSL 的自动回退使用目标游戏的默认渲染，原有着色器效果不会被保留。

## 下载与使用

1. 打开 [最新 Release](https://github.com/LongCold-ColdEternity-Team/Fast-Pack/releases/latest)，下载 `FastPack-v<版本号>.exe`，或下载 Windows x64 便携包。当前版本为 `FastPack-v1.1.12.exe`。
2. 双击下载的程序，把材质包 ZIP 或文件夹拖入窗口。
3. 选择目标版本与导出目录，点击“开始转换”。资源适配默认自动执行。
4. 完成后，通过“打开输出目录”取得转换结果，再将材质包交给对应版本的游戏加载。

运行桌面版无需解释器或浏览器。材质包处理在本机完成，程序不会把上传内容发送到远端服务。“了解详细”中的源码按钮会在用户点击后打开 GitHub 页面。

右上角的 **“了解详细”** 展示开发者头像、版本、开发交流群、进群答案、源码仓库与许可证摘要。

主页“自动保存”一行提供“版本”和“导出目录”两个开关，默认开启。版本开关开启后选择目标版本会自动保存，比如选择 `1.20.1`，下次打开仍默认 `1.20.1`。关闭后本次版本继续使用，重启恢复默认版本与自动推荐。

导出目录通过“选择”按钮选择文件夹，也可以输入完整目录路径。开启目录记忆后，选择的文件夹会自动保存；手动输入支持回车、短暂停止输入或开始转换时保存。下次打开会恢复该目录。关闭后本次目录保留，重启恢复保存在源包旁；清空目录则恢复默认位置。只记住导出文件夹，每次的文件名依照当前版本与当前原包名重新生成。不存在的导出目录会在转换时创建。

两个开关的状态均在重启后保留，旧版已经保存的版本自动兼容。“减少动态效果”会关闭开关过渡动画。长目录在输入框内显示，文件名预览缩短显示，完整导出路径可悬停查看。

设置文件缺失、损坏或保存的版本不再受支持时，程序使用默认版本；首次运行且尚未选择版本时保留原有自动推荐逻辑。

### 自定义附魔闪烁颜色

展开 **“更多转换选项”**，开启 **“自定义附魔颜色”**，点击颜色右侧紧邻的 **“调色”** 按钮。可以直接选白色等预设，输入 `#RRGGBB` / `#RGB`，或拖动调色卡。右侧竖向色谱选择色相，左侧方形区域横向调整饱和度、纵向调整明暗；点击预设、输入有效色值或拖动调色卡会自动启用自定义颜色。

颜色和自定义开关默认自动记住，后续转换与重启继续使用。取消勾选自定义颜色后，色块、调色卡与动画预览同步恢复原版紫色，下一次转换也会将输入包的附魔光效统一恢复紫色。之前选择的自定义颜色仍被记住，重新勾选即可恢复。设置存放在同一个 `%LOCALAPPDATA%\Fast Pack\settings.json` 中；拖动调色短暂停止后自动保存，关闭面板、开始转换或关闭程序时也会提交待保存的颜色。

1.8.9–1.14 的附魔图案通常为灰度图，颜色由游戏渲染器额外乘入；1.15 起颜色烘焙进贴图。底层兼容转换会补上或补偿这层染色，修复旧图直接复制到新版后变白的问题。桌面版未勾选自定义颜色时，最终输出统一使用原版紫色；需要白色等效果时开启自定义颜色选择相应色值。1.19.4 起同步生成物品、实体和盔甲资源名称，兼容后续版本的路径变化。

自定义颜色会统一光效色相并保留图案的相对明暗与 alpha，继续使用游戏自带的流动闪烁动画。原包没有附魔图时，自动生成可平铺图案，无需玩家补文件。面板中的像素剑为颜色示意；游戏的实际亮度还取决于渲染版本、客户端设置和包内自定义渲染逻辑，旧版染色补偿可能降低亮度。

输出包内缓存原始光效图案并核对当前贴图指纹，再次修改颜色时复用图案，避免重复染色越来越暗；选过黑色后也能换回其他颜色。自行修改过的贴图会作为新的图案处理。

### 转换结果命名

默认按 **`[目标版本]原材质包名.zip`** 命名，并保留版本号中的点。例如：`9178.zip` 转为 1.20.1 后得到 **`[1.20.1]9178.zip`**。

输入文件名已有受支持的版本前缀时，会替换该前缀，避免重复叠加。文件夹输出采用 `[目标版本]原文件夹名`，保留原文件夹名称中的点。如果生成路径会与源路径相同，结果放入同级的 `Fast Pack` 文件夹，保护源包。

GUI 会随原包和目标版本更新默认文件名，并使用选定的导出目录。CLI 可省略 `OUT` 使用相同命名规则；显式选择的输出名称仍按用户指定的路径保存。

### 发布程序命名

v1.1.7 起，GUI 和命令行发布文件均带版本号：`FastPack-v1.1.12.exe`、`fast-pack-cli-v1.1.12.exe`。便携包也包含对应的版本化文件名。后续发布由 Rust 打包入口从 Cargo 版本自动生成名称，开发构建目录中的 Cargo 目标名仍为 `FastPack.exe` 和 `fast-pack-cli.exe`。

### 转换过程中发生了什么

```mermaid
flowchart LR
    A[导入 ZIP / 文件夹] --> B[识别版本与资源结构]
    B --> C[目录、名称与语言迁移]
    C --> D[实体、模型、GUI、字体适配]
    D --> E[天空与渲染资源兼容处理]
    E --> F[写入目标元数据并导出]
```

源包保持原样。输出包内的 `fastpack/` 保存转换所需的原始资源、天空缓存与兼容信息，这些文件帮助反向转换恢复内容。

## 自动适配能力

| 资源类型 | 自动处理内容 |
| --- | --- |
| 纹理目录与名称 | `blocks/items` 与 `block/item` 的目录迁移、已知名称映射、动画元数据同步改名与后续版本路径变化 |
| 语言资源 | 旧 `.lang` 与新 JSON 语言格式互转，保留可解析内容 |
| 箱子 | 1.15 UV 重排、双箱拆分为左右纹理、高清比例与反向合并 |
| 船与其他实体 | 旧船 UV 重建、船桨纹理生成、实体目录别名、26.x 矿车路径及旧船反向重建 |
| 旗帜 / 盾牌 | 将旧图案掩码转为 alpha，结合上传的底图 RGB；降级时生成旧式灰度掩码 |
| 末影龙 | 清理 1.15 删除的龙翼底面 UV，并保留原图 |
| 模型 / 方块状态 | 旧手持姿态基准换算、降级时展开平面物品父模型继承、补齐缺省姿态、左右手显示结构、旧转换备份修复、普通状态键、模型名称与引用、1.21.4+ 默认物品定义绑定 |
| 发射器 / 投掷器 | 六方向旧版 variants、水平与垂直模型补齐、朝向纹理引用修复 |
| 游戏模式 GUI | 在实际 UV 的 `y=75` 行将两个 25px 槽位适配为 26px |
| 背包 GUI | 从原效果列表的边框生成窄版效果背景，保留材质风格 |
| GUI 图集 | 从 1.20.2 起按 88 条配方生成独立精灵，保留高清比例；支持反向合并 |
| 创造背包 | 六列 28px / 七列 26px 图集、上下 / 选中状态、误切红蓝占位区修复、完整边框、滚动条合并、1.8.9 人物窗口与装备格重定位 |
| 药水效果图标 | 将 mob_effect 独立图标合并到旧版 inventory 图集，保留包内图标并补齐缺失符号 |
| 附魔光效 | 旧版紫色染色烘焙 / 反向补偿，生成物品、实体与盔甲资源，任意 RGB 颜色、原图案和 alpha 保留、动画元数据同步、缺失图案自动生成 |
| Unicode 字体 | 将上传的 16×16 字符网格页转成 bitmap 提供器，补 uniform 定义及目标默认字形回退 |
| 天体 | 26.x 太阳路径与八张月相资源的拆分、旧式月相图合并 |
| 天空 | 路径与纹理引用重写、六面朝向、可表达的淡入淡出和条件迁移、静态天空格式转换、原始层缓存 |
| 材质包着色器 | 代际识别、目标版本变体匹配、已识别的 import 规则处理、跨代覆盖的自动备份与回退 |

## 支持的版本标签

| 系列 | 版本 |
| --- | --- |
| 1.8–1.12 | 1.8.9、1.9.4、1.10.2、1.11.2、1.12.2 |
| 1.13–1.16 | 1.13.2、1.14.4、1.15.2、1.16.1、1.16.5 |
| 1.17–1.19 | 1.17.1、1.18.2、1.19.2、1.19.3、1.19.4 |
| 1.20 | 1.20.1、1.20.2、1.20.4、1.20.6 |
| 1.21 | 1.21.1、1.21.3、1.21.4、1.21.5、1.21.6、1.21.8、1.21.10、1.21.11 |
| 26.x | 26.1.2、26.2 |

版本表内置于源码，26.2 使用资源格式 88。程序不会联网更新版本规则。相同 `pack_format` 的版本可能共用一组适配规则；表外版本标签当前不提供直接选择。

## 兼容范围

- 面向 **Minecraft Java Edition 材质包 / 资源包**；独立光影包与基岩版包不在处理范围内。
- 自定义天空依赖客户端本身具备相应渲染能力，资源包文件无法给纯原版客户端新增该能力。
- 跨代 GLSL 缺少可确认兼容变体时会回退默认渲染，转换后的包可失去原着色器特效。
- 任意动态天空关键帧、自定义模型谓词、已删除功能和新版本独有方块，无法保证在旧版本中等价表现。未识别内容会保留或注明处理结果。
- 原包未提供的下界合金、三叉戟等新增装备会沿用客户端材质；程序不会把凭空生成的贴图当成原作者作品。原包本来使用原版图标的装备会保留这些图标。
- 冲突与未知名称会进入转换详情；原包仍会保留。
- 未在每个 Minecraft 版本、客户端加载器及实际材质包中逐一验证游戏画面。有限样例不代表任意资源都完全等价。
- Windows x64 桌面版是当前发布目标。库与 CLI 可用于其他平台，当前 Release 提供 Windows 构建。

## 命令行

便携包包含 `fast-pack-cli-v<版本号>.exe`。以下以 v1.1.12 为例，命令默认执行自动适配：

```powershell
.\fast-pack-cli-v1.1.12.exe old.zip new.zip --to 26.2
.\fast-pack-cli-v1.1.12.exe 9178.zip --to 1.20.1
.\fast-pack-cli-v1.1.12.exe modern.zip legacy.zip --from 26.2 --to 1.8.9
.\fast-pack-cli-v1.1.12.exe old.zip output_folder --folder --to 1.20.1
.\fast-pack-cli-v1.1.12.exe old.zip --probe
.\fast-pack-cli-v1.1.12.exe --versions
.\fast-pack-cli-v1.1.12.exe old.zip new.zip --to 26.2 --quiet
.\fast-pack-cli-v1.1.12.exe old.zip --to 1.20.1 --glint-color '#FFFFFF'
.\fast-pack-cli-v1.1.12.exe old.zip --to 1.20.1 --vanilla-glint
.\fast-pack-cli-v1.1.12.exe old.zip new.zip --renames names.json --report-json report.json
```

`--from auto` 默认自动识别源版本。`--report-json` 仅在指定时将详情另存到提供的路径，不会把 `转换报告.txt` 塞入输出 ZIP。自定义映射采用 JSON 对象，例如 `{"custom_texture":"replacement_texture"}`。

`--glint-color` 接受带或不带 `#` 的 3 位 / 6 位 HEX 色值。省略时保留输入包的光效效果并自动适配目标版本；`--vanilla-glint` 将附魔光效恢复原版紫色，与 `--glint-color` 互斥；两个选项都省略时继续自动适配输入包颜色。命令行参数不会改写桌面版已保存的颜色。

GUI 对已有输出提供覆盖确认；CLI 会替换明确指定的已有输出。输出不得与源路径相同，也不得覆盖源目录或其上级目录。

## 从源码构建

推荐使用 Rust MSVC 稳定版工具链，以及包含 Windows SDK 的 Visual Studio C++ Build Tools。当前项目使用 Rust 2024 edition、Iced 0.14 和静态 CRT 配置；构建工具本身不随程序分发。

```powershell
git clone https://github.com/LongCold-ColdEternity-Team/Fast-Pack.git
cd Fast-Pack
cargo build --release --locked
```

构建结果：

```text
target-native/release/FastPack.exe
target-native/release/fast-pack-cli.exe
```

发布打包使用 Rust 入口。将第三方许可证整理到 `third-party-licenses` 后执行：

```powershell
cargo run --release --locked --example release_bundle -- target-native/release dist-releases third-party-licenses
```

发布文件生成到 `dist-releases/v<Cargo 版本>/`，包含带版本号的 EXE、便携 ZIP 和 `SHA256SUMS.txt`。文件名直接来自当前 Cargo 版本，后续版本无需手动改写打包命名。

源码仓库保留 Rust 实现、Cargo 文件、构建配置、内置资源配方、图标、开发者头像和文档。编译产物在 Releases 分发。测试输入由 Rust 代码生成，仓库无需携带打包的测试 ZIP。

开发者可以按需执行：

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
```

需要使用软件渲染时，可在启动前指定后端：

```powershell
$env:ICED_BACKEND = 'tiny-skia'
.\target-native\release\FastPack.exe
```

## 源码结构

| 路径 | 职责 |
| --- | --- |
| `src/gui.rs` | 桌面工作台、动画、后台任务、天空预览和“了解详细”页面 |
| `src/preferences.rs` | 用户配置目录中的目标版本、导出目录、附魔颜色与开关记忆 |
| `examples/release_bundle.rs` | 从 Cargo 版本生成发布文件名、便携包与 SHA-256 校验值 |
| `src/motion.rs` | 有界、可中断、可关闭的时间插值 |
| `src/converter.rs` | ZIP 与目录处理、转换流水线、元数据和详情 |
| `src/compat/assets.rs` | 实体 UV、alpha、模型、GUI、附魔光效、字体 |
| `src/compat/glint.rs` | 附魔染色代际转换、HEX 解析、RGB 重着色、图案生成与元数据同步 |
| `src/compat/item_display.rs`、`assets/item-display.json` | 官方物品手持姿态参考表、跨版本姿态换算与旧转换恢复 |
| `src/compat/item_bindings.rs`、`assets/item-bindings.json` | 旧包缺失的物品模型绑定与盔甲纹饰变体 |
| `src/compat/legacy_gui.rs`、`legacy_effect_icons.rs` | 创造标签页与滚动条图集重建、药水效果图标合并与默认符号 |
| `src/compat/legacy_blocks.rs` | 发射器与投掷器六方向状态、缺失模型与纹理适配 |
| `src/compat/sky.rs`、`atlas.rs` | 天空解析、路径、六面转换与原始层缓存 |
| `src/compat/shaders.rs` | 着色器代际、版本变体与兼容回退 |
| `src/compat/properties.rs` | Java properties 转义、续行和 Unicode |
| `src/tables.rs`、`assets/*.json` | 内置版本表、名称映射与像素配方 |
| `src/cli.rs`、`src/bin/cli.rs` | 独立命令行入口 |
| `tests/` | 转换、兼容、安全边界和由 Rust 生成的样例 |
| `examples/` | Rust 诊断样例、配方校核与图标处理工具 |
| `assets/developers/` | 开发者头像，用于应用与项目介绍 |

## 开发者

<table>
  <tr>
    <td align="center"><img src="assets/developers/coldeternity-team.png" width="96" height="96" alt="ColdEternity Team"><br><strong>ColdEternity Team</strong></td>
    <td align="center"><img src="assets/developers/long-cold.png" width="96" height="96" alt="Long_Cold"><br><strong>Long_Cold</strong></td>
    <td align="center"><img src="assets/developers/wind.png" width="96" height="96" alt="Wind_"><br><strong>Wind_</strong></td>
  </tr>
</table>

关注wind

## 开发交流群

**QQ 群：663420633**  
**进群答案 / 密码：快手/B站/抖音**

交流、反馈和二次开发请尽量说明源版本、目标版本、复现步骤与具体资源路径。分享材质包前，请确认自己有权公开其中的内容。

## 许可证

本项目采用 **ColdEternity Team Source-Available License 1.0**，完整条款见 [LICENSE](LICENSE)。该许可证属于自定义源码可用许可证，未获 OSI 认可。

- 二次修改、改编、重新打包与衍生发布无需事先授权。
- 二改版本必须显著标注 **ColdEternity Team**，并保留 [本原始仓库链接](https://github.com/LongCold-ColdEternity-Team/Fast-Pack)。
- 二改版本发布可执行文件时，必须同时在公开仓库提供可复现该程序的完整对应源码；不得闭源、仅发布混淆源码或缺失关键构建资源。
- 原版与二改版均不得收费、提供付费下载或会员解锁、捆绑付费产品 / 服务，或用于其他直接、间接商业获利。
- 不得删除、隐藏作者、许可证及原始仓库信息；二改版本还应说明实质性改动。

第三方依赖遵循其各自许可证，发行包保留相应许可材料。

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=LongCold-ColdEternity-Team/Fast-Pack&type=Date)](https://star-history.com/#LongCold-ColdEternity-Team/Fast-Pack&Date)
