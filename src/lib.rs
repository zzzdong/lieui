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
#[cfg(feature = "winit")]
pub mod platform;
pub mod reactive;
pub mod render;
pub mod style;
pub mod theme;
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
pub use track::{
    Anchor, Axis, Flags, FocusPolicy, FocusState, ImageData, InteractionState, Key, Kind, KindDesc,
    KindTag, Layer, LayerOpts, Node, NodeId, Placement, Root, RootId, Track, Transform, Visibility,
};
pub use view::{DescRef, ViewBuf};
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
    pub use crate::event::{Ctx, Event, EventKind, EventView, PointerButton, PointerId};
    pub use crate::icon::{icon_char, icon_font_family};
    pub use crate::input::InputEvent;
    pub use crate::reactive::{Runtime, Signal, act, act1};
    pub use crate::style::{PaintStyle, ShadowSpec, TextStyle};
    pub use crate::theme::Theme;
    pub use crate::track::{
        AnchorTarget, FocusState, ImageData, Key, Layer, NodeId, Placement, Transform, Visibility,
    };
    pub use crate::view::{DescRef, ViewBuf};
    pub use lieui_geom::{Color, Point, Rect, Size};
    pub use crate::render::scene::Scene;
    pub use crate::transform::Affine;
}
