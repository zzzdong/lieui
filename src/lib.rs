#![forbid(unsafe_code)]
//! lieui v3 —— retained 视图树 + R1 响应式 + 纯 CPU 渲染
//!
//! **状态：M1 进行中**（M0 已完成：基础设施 crate 抽出）。
//! 设计基线见 `docs/architecture-v3.md`，施工记录见 `docs/operation-log.md`。
//!
//! ## 已完成的模块
//!
//! | 模块 | 内容 |
//! |---|---|
//! | [`reactive`] | `Runtime`（每窗口脏标志表）+ `Signal<T>`（`Rc` 句柄，无 thread_local）+ `act` |
//! | [`window`] | `WindowId`（M4 与 winit 做映射） |
//! | [`style`] | `TextStyle`（= `TextSpec` + 颜色）/ `PaintStyle` / `ShadowSpec` / `ImageStyle` |
//! | [`event`] | `EventKind` / `Routing` / `Event` / `HandlerSlot` / `Ctx` / 两段式路由分发 |
//! | [`cmd`] | `Cmd` 命令缓冲（唯一延迟写入通道）+ `apply_cmds` |
//! | [`track`] | 保留树（arena + `Node` + `Kind`/`KindDesc` + 层 + 视图态 + 脏区） |
//! | [`view`] | 描述 arena + 构造 DSL（`column`/`row`/`text`/`button`/`keyed_list`/层入口） |
//! | [`align`] | 位置 + 一层 key 对齐 + 内部 patch + 脏区登记 |
//! | [`layout`] | 布局适配器：保留树 ⇄ Taitank Flex 引擎，**边界重排** + 脏区登记 |
//! | [`hit`] | 命中测试（层序 / 可见性 / 裁剪 / 变换 / 捕获），纯函数 |
//! | [`input`] | 指针输入状态机（hover 传播 / `Tapped` 合成 / 捕获路由 / 默认滚动） |
//! | [`focus`] | 焦点管理（`set_focus` / Tab 链 / `FocusState`） |
//! | [`transform`] | 2D 仿射矩阵（绘制 / 命中 / 裁剪共用） |
//! | [`app`] | `ViewModel` / `WindowView`（对象安全擦除）/ `WindowCtx` 帧驱动 / 多窗口 `App` |
//! | [`theme`] | `Theme`（design token）+ `ThemeMode`（浅/深/跟随系统/自定义）|
//! | [`task`] | 后台任务与跨线程通信：`Waker` / `spawn_task(_busy)` / 取消 / loading 遮罩状态 |
//! | [`timer`] | 统一时钟：`set_timeout` / `set_interval` / `request_animation`（帧唤醒汇总）|
//!
//! ## 事件与时钟（速览）
//!
//! **两条入径，各自单一职责**：
//!
//! | 通道 | 承载 | API |
//! |---|---|---|
//! | 输入事件 | 指针 / 键盘 / IME / 滚动 | winit → `InputEvent`/`Event` → `dispatch`（用户 handler）|
//! | **消息** | 自定义事件 / 后台任务 / 跨线程 | `Runtime::emit` / `Ctx::emit` / [`Emitter`] / `Runtime::emit_global` → `on_external` |
//!
//! 框架控制消息（开窗/关窗）单独走 `Ctx::request`（`RequestQueue`），别拿它当事件总线。
//!
//! **一个时钟**：定时器与动画帧统一在 [`timer`]，平台层只问
//! `WindowCtx::next_wakeup()`（= 光标闪烁 / tooltip / 忙碌 spinner / 定时器 / 动画帧 的最早值）；
//! 没有唤醒源就是 `ControlFlow::Wait`（空闲零功耗）。
//!
//! ## 后台任务与 loading 遮罩（速览）
//!
//! ```ignore
//! // 事件处理器里：起任务 + 遮罩（进度 / 取消都自动接好）
//! cx.spawn_task_busy("正在导出…", |ctx| {
//!     for i in 0..n {
//!         if ctx.is_cancelled() { return Err("已取消"); }
//!         ctx.progress(i, n);
//!         work(i);
//!     }
//!     Ok(summary)
//! });
//! // 结果在 on_external 里取：data.downcast::<TaskEvent>() -> ev.payload.downcast::<Result<..>>()
//! ```
//!
//! ## 待补（里程碑）
//!
//! - **M1/M2 剩余**：`stack` 容器、`*_bind` 双向绑定（依赖 Input/Slider 的真实行为）
//! - **M3**：`render/` draw list + 持久 Pixmap + 脏区 + `present_with_damage`
//! - **M4**：winit 事件循环 / IME / 拖拽 / 关闭守卫 / `RepaintHandle` / 动态开关窗 / 层锚点
//! - **M5**：`widgets/` + `custom.rs`（`Kind::Custom` 逃生舱）

pub mod align;
pub mod app;
pub mod cmd;
pub mod custom;
pub mod event;
pub mod focus;
pub mod hit;
pub mod icon;
pub mod input;
pub mod layout;
pub(crate) mod overlay;
#[cfg(feature = "winit")]
pub mod platform;
pub mod reactive;
pub mod render;
pub mod style;
pub mod task;
pub mod theme;
pub mod timer;
pub mod track;
pub mod transform;
pub mod view;
pub mod widgets;
pub mod window;

pub use align::{AlignStats, align};
pub use app::{
    App, CloseAction, ExternalData, FrameStats, PointerOutcome, ViewModel, WindowConfig, WindowCtx,
    WindowView, erased,
};
pub use cmd::{Cmd, CmdBuf, apply_cmds};
pub use event::{
    Ctx, DispatchOutcome, Event, EventKind, EventView, Handler, HandlerSlot, KeyCode, NamedKey,
    PointerButton, PointerId, RoutePlan, Routing, collect_route, dispatch,
};
pub use focus::FocusChange;
pub use input::{InputEvent, InputStep};
pub use layout::LayoutStats;
pub use reactive::{Dirty, Runtime, Signal, act, act1};
pub use render::{
    Op, RasterStats, Rasterizer, RenderStats, Renderer, Scene, SceneBuilder, SceneOptions,
    SceneStats, TextCache, damage_batches,
};
pub use transform::Affine;
pub use style::{ImageFit, ImageStyle, PaintStyle, ShadowSpec, TextStyle};
pub use event::Emitter;
pub use task::{BusyItem, BusyToken, CancelToken, TaskCtx, TaskEvent, TaskFailed, TaskHandle, Waker};
pub use timer::{FRAME_PERIOD, TimerHandle};
pub use track::{
    Anchor, Axis, Flags, FocusPolicy, FocusState, ImageData, InteractionState, Key, Kind, KindDesc,
    KindTag, Layer, LayerOpts, Node, NodeId, Placement, Root, RootId, Track, Transform, Visibility,
};
pub use view::{DescRef, ViewBuf, VirtualListState};
pub use window::WindowId;

// 基础设施 crate 的便捷再导出。
// 注意：本 crate 自己有一个 `layout` 模块（布局适配器），所以引擎 crate 别名用 `flex` 避免撞名。
pub use lieui_geom as geom;
pub use lieui_layout as flex;
pub use lieui_text as text;

/// 用户态常用导入（`use lieui::prelude::*;`）
pub mod prelude {
    pub use crate::app::{App, CloseAction, ExternalData, ViewModel, WindowConfig, WindowCtx};
    pub use crate::cmd::CmdBuf;
    pub use crate::custom::{self, CustomCell, CustomNode};
    pub use crate::event::{Ctx, Emitter, Event, EventKind, EventView, PointerButton, PointerId};
    pub use crate::icon::{icon_char, icon_font_family};
    pub use crate::input::InputEvent;
    pub use crate::reactive::{Runtime, Signal, act, act1};
    pub use crate::style::{PaintStyle, ShadowSpec, TextStyle};
    pub use crate::task::{
        BusyItem, BusyToken, CancelToken, TaskCtx, TaskEvent, TaskFailed, TaskHandle, Waker,
    };
    pub use crate::theme::{Theme, ThemeMode};
    pub use crate::timer::{FRAME_PERIOD, TimerHandle};
    pub use crate::track::{
        AnchorTarget, FocusState, ImageData, Key, Layer, NodeId, Placement, Transform, Visibility,
    };
    pub use crate::view::{DescRef, ViewBuf, VirtualListState};
    pub use lieui_geom::{Color, Point, Rect, Size};
    pub use crate::render::scene::Scene;
    pub use crate::transform::Affine;
}
