# M1 布局里程碑 —— 变更记录与偏离说明

> 配套 `docs/design.md` §3.3 / §7.2。本文件专门记录**临时调整、与原计划不一致的点、踩过的坑**，
> 便于后续里程碑（M2 信号、M4 主题热重载、M5 平台……）回溯。

## 1. 与原计划（design.md）不一致的点

### 1.1 布局引擎：taffy → 移植 Taitank Flex
- **原计划**：`crates/lieui-layout` = taffy 集成（`TaffyTree<MeasureContext>` + `LayoutPartialTree`）。
- **实际**：用户临时决策改为使用本地 `main` 分支里已有的 Taitank 风格 Flex 引擎，只做 Flex 布局。
- **做法**：`git archive main src/layout | tar -xf` 字节安全取出 → 拆成 `types/style/flex_line/flex_node/box_model/measurable`，**删去** `text`/`state`/`view`/`geometry`/`paint` 等强依赖，改为通过 `LayoutTree` trait 回调宿主。
- **影响**：
  - `lieui-layout` 变为**零依赖、树无关** crate（不认识节点树 / 属性系统 / 文本服务）。
  - `Window.taffy` 字段已移除；布局改为函数式（`compute_layout(host, root, avail, origin)`）。
  - design.md §3.3 / §7.3 依赖表已同步修订。

### 1.2 parley 版本 0.7 → 0.11
- `Brush` trait 在 0.11 中是**空结构体 `()`**（无字段），颜色由绘制层取（布局/文本层不持有颜色）。
- 部分 API 名称/路径与原 `0.7` 计划不同（`ranged_builder` / `break_all_lines` / `Alignment::Start` 等）。
- `FontContext::default()` 在 Windows 上可用（系统字体集合），无需额外配置。

### 1.3 重排边界实现方式不同
- **原计划**：依赖 taffy 的 partial-tree 增量。
- **实际**：`ComputedLayout` 回显 `local_x/local_y`（父内容盒内偏移）与 `avail_w/avail_h`（父约束原值）。
  重排边界子树重算时，用上一帧的 `parent_origin()` 还原原点、用 `avail` 原样回传父约束，从而**不必重算祖先**即可复现完全相同输入。

### 1.4 百分比尺寸用「至多 2 轮 pass」修正
- Flex 引擎解析 `Percent` 需要父级可用尺寸；首帧无历史 `avail` 时退化成 Auto，并置 `unresolved_percent`，
  P2 在首轮后若仍有未解析百分比则跑第 2 轮（此时 `last` 已记录了第 1 轮的 `avail`）。
- 这与设计「P2 单遍」描述略有出入，但仅发生在首帧 / 视口变化后的首帧。

### 1.5 Windows 链接器未启用 rust-lld
- stable 工具链下 `-Clinker-features=+lld` 仍是 unstable；本机未安装 `lld-link`。
- 因此 `.cargo/config.toml` 只配置了 Linux mold + macOS lld，Windows 段**注释保留**并写明启用条件（安装 LLVM 后取消注释）。
- 当前规模（4 个 crate）下 link.exe 链接时间占比很小，暂不阻塞。

## 2. 实现过程中的临时调整 / 坑（已修复，留作警示）

### 2.1 不可继承属性沿树向上「污染」子节点（真实 bug）
- 现象：文本节点被父容器 `WIDTH/HEIGHT` 命中，全变成 400×600，且被误判为布局边界。
- 根因：`resolve_inherited` 原本对所有槽位都沿树向上取值。
- 修复：**只有 `INHERITABLE_SLOTS`（`font_size/family/weight/line_height/italic/fg`）才向上走**，且祖先的 `LOCAL` 也参与继承（容器上写的 `font_size` 应被子节点继承）。
- 回归测试：`props::store` 中 `non_inheritable_props_do_not_walk_up`。

### 2.2 PowerShell 5.1 下 cargo stderr 中断测量脚本
- `cargo build 2>&1 | Out-Null` 在 PS 5.1 下会把 native stderr 包成 `ErrorRecord`，配合 `EAP=Stop` 直接终止脚本。
- 解决：脚本统一用 `cmd /c "cargo build ... >nul 2>&1"`（见 `scripts/measure_build.ps1`）。

### 2.3 `edition = "2024"` 的保留字 `gen`
- `NodeId` 用 `index | (generation << 32)`，变量名不能叫 `gen`（edition 2024 保留字），改用 `generation` / `gen_val`。

### 2.4 `git archive` 经 PowerShell 管道会破坏 UTF-8
- 直接 `git archive main src/layout | tar -xf` 在 PowerShell 中会损坏非 ASCII 字节。
- 解决：`git archive -o layout.tar main src/layout; tar -xf layout.tar`（走文件，不经管道）。

### 2.5 `LayoutTree` 用 `&mut self`
- 宿主（LayoutHost）需要持有属性解析缓存与文本测度缓存，方法必须是 `&mut self`。
  `collect_children` 用「填充 `out: &mut Vec`」而非 `FnMut` 回调，规避回调内重入借用的冲突。

## 3. 验收数据（详见 `docs/m1-build-times.md`）
- 重排边界：改 1 个文本 → 只重排所在边界子树（7/15 节点），不整窗。
- 测度缓存：二次重排命中率 >90%，零重新整形。
- 编译时间：noop 168ms / 结构 1.2s / 叶子 1.5s / cold 56s（达标）。
- 27 个单测 + clippy 零警告 + rustfmt 通过。
