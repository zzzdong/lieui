# M1 编译时间基线

对应设计 §7.2「M1 布局」任务 4（量编译时间）与任务 5（mold / lld 接入）。

**目标**：样式改动 ≤ 200ms / 结构改动 ≤ 5s（有数据）。

## 测量方法

```powershell
.\scripts\measure_build.ps1               # 只打印
.\scripts\measure_build.ps1 -UpdateDoc    # 同时刷新本文表格
```

- 命令：`cargo build --workspace --examples`（含链接，因此包含链接时间）
- 每个增量场景跑 2 次取较小值，避开首次磁盘缓存抖动
- `structure` = 改 `lieui-core/src/tree.rs`（内核，影响面最大）
- `leaf` = 改 `lieui-text/src/spec.rs`（最下游叶子 crate，影响面最小）

## 实测数据

<!-- MEASURED:BEGIN -->
| 场景 | 耗时 |
|---|---|
| noop（无改动） | 168 ms |
| structure（lieui-core/src/tree.rs） | 1211 ms |
| props（lieui-core/src/props/keys.rs） | 1155 ms |
| leaf（lieui-text/src/spec.rs） | 1454 ms |
<!-- MEASURED:END -->

补充（2026-09-08，同机手动测量）：`cargo clean` 后全量构建（`--workspace --examples`）约 **56 s**（含 parley 等全部依赖）。

**验收判定（M1 任务 4）**

| 目标 | 阈值 | 实测 | 结论 |
|---|---|---|---|
| 样式改动（不触发重编译，M4 走热重载；此处以 noop 为代理） | ≤ 200 ms | 168 ms | ✅ |
| 结构改动 | ≤ 5 s | 1.2 s | ✅ |

> 数据受机器、磁盘与是否首次构建影响，仅作同机回归基线用。
> CI 上应固定机型并把回归阈值设为「基线 × 1.5」。

## 链接器

| 平台 | 配置 | 状态 |
|---|---|---|
| Linux | `.cargo/config.toml` → `clang + -fuse-ld=mold` | 已配置（需本机安装 mold） |
| macOS | `-fuse-ld=ld64.lld` | 已配置（需 Homebrew LLVM） |
| Windows / MSVC | 默认 `link.exe` | **未启用 lld**：stable 工具链的 `-Clinker-features=+lld` 仍为 unstable，`rust-lld.exe` 无法在 stable 上选用；如需启用须先安装 LLVM，再按 `.cargo/config.toml` 里的注释打开 `linker = "lld-link"` |

- 现状（Windows）：增量构建瓶颈主要在 rustc 的代码生成与 `link.exe`，本项目当前规模（4 个 crate、无重型依赖树）的链接时间占比很小。
- 后续：M6 性能阶段会补一次「链接耗时」单独采样（`cargo build --timings`），再决定是否强制切换 lld。

## 降低编译时间的既有措施

- 依赖图单向、内核零重型依赖：`lieui-layout` 无外部依赖，`lieui-text` 只依赖 parley
- `[profile.dev.package."*"] opt-level=2`：依赖全量优化，自己代码 `opt-level=1`
- `debug = "line-tables-only"`：显著减小调试信息与链接输入
