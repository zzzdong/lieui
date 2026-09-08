//! 菜单组件：自绘弹出菜单、菜单项、分隔线、子菜单、右键菜单容器。
//!
//! 设计采用「方案 A：纯自绘 + 防溢出翻转」。菜单落在本窗口的 `LayerKind::Popup`
//! 层，自动设置 `FocusPolicy::Dismissable`（点击浮层外部自动关闭），并由 `apply_anchor`
//! 在定位超出视口时自动翻转到对侧，保证菜单始终完整落在窗口内。
//!
//! # Popup 生命周期体系
//!
//! 所有菜单浮层共用一套由 `LayerStack` 维护的**父子归属 + 级联关闭**规则：
//!
//! - 每个 popup（父菜单 / 子菜单 / 右键菜单）都是独立 `LayerEntry`，但通过
//!   `show_popup_with_parent(.., parent)` 登记**父子归属**：子菜单的 parent 是父菜单，
//!   右键菜单套菜单同理。
//! - 关闭入口统一为 `hide_popup(handle)` → `LayerStack::close_popup`，它会**级联关闭
//!   整棵子树**：父菜单被关闭（点项 / 点外 / hover 离开 / 右键重复打开）时，其所有
//!   展开的子菜单一并收起，**不会残留孤儿 popup**。
//! - 右键菜单重复点击时，`ContextMenu` 会先 `hide_popup` 旧的再开新的，不再依赖
//!   `hit_test_top` 「点击旧 popup 外部」的巧合（嵌套右键菜单也能正确关闭旧菜单）。
//!
//! # 关闭策略
//!
//! | 触发源          | 机制                                     |
//! |-----------------|------------------------------------------|
//! | 菜单项点击      | `MenuItem::on_mouse_up` → `hide_popup(根)`（级联关子树） |
//! | 点击浮层外部    | `LayerStack` Dismissable → `close_popup`（级联）        |
//! | hover 离开      | `close_on_leave` → `hide_popup`（级联）   |
//! | 右键重复打开    | `ContextMenu` 先 `hide_popup(旧)`（级联） |

use crate::event::{Event, EventContext, MouseButton};
use crate::geometry::Color;
use crate::prelude::FlexAlign;
use crate::state::{
    PopupHandle, PopupPlacement, hide_popup, show_popup_with, show_popup_with_parent,
};
use crate::theme::current;
use crate::view::node::ViewNode;
use crate::widget::{BuildContext, Button, ButtonVariant, Column, Container, Row, Text, Widget};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

/// 菜单项点击回调类型。
type MenuClickFn = Rc<dyn Fn(&mut EventContext)>;
/// 菜单内容构建器类型：给定 Popup 句柄与构建上下文，产出菜单内容。
/// 由 `ContextMenu` 与 `MenuButton` 共用。
type MenuBuilder = Rc<dyn Fn(PopupHandle, &mut BuildContext) -> Menu>;

/// 菜单容器：纵向排列的菜单项/分隔线/子菜单，自绘背景、圆角、边框、阴影。
///
/// 子项用枚举存储，便于在 `build` 阶段把所属 Popup 的 `close_handle` 注入给
/// 每个 `MenuItem`，使点击后自动关闭菜单。
pub struct Menu {
    children: Vec<MenuChild>,
    min_width: f32,
    /// 菜单外层卡片背景色。为 `None` 时使用当前主题背景。
    background: Option<Color>,
    close_handle: Option<PopupHandle>,
    /// 鼠标离开菜单区域时是否自动关闭（如右键菜单场景）。
    close_on_leave: bool,
    /// 本菜单树当前已展开的**所有子孙** popup 句柄集合（含直接子与更深层）。
    ///
    /// 在整棵 `Menu` 树构建时共享给所有 `Submenu`；子菜单展开/收起时写入/移除自己
    /// 及其后代的句柄。父菜单 `close_on_leave` 判定时据此在「鼠标正移向某子孙菜单」
    /// 时挂起关闭，待该子孙收起后再生效。集合记录整条展开链，因此天然支持多级嵌套
    /// 子菜单（旧实现的 `submenu_tracker` 只记录直接子，多级嵌套会误关）。
    expanded: Rc<RefCell<HashSet<PopupHandle>>>,
    /// 跨所有 `Submenu` 共享的「待关闭二级菜单」信号。
    ///
    /// 当鼠标从某子菜单主行移开时，不直接关闭其二级菜单，而是把句柄写入 `pending`；
    /// 待鼠标 `on_mouse_enter` 进入**下一个**目标（另一个子菜单主行 / 普通项 / 分隔线）
    /// 时再执行关闭。这样「主行 → 二级菜单」的进入路径不会误关（二级的 enter 会取消
    /// `pending`），而「主行 → 其它项 / 菜单外」则必然关闭，彻底覆盖「移离子菜单后
    /// 应自动关闭」的所有场景，且无 timer 依赖、无闪烁。
    pending: Rc<RefCell<Option<PopupHandle>>>,
}

enum MenuChild {
    Item(MenuItem),
    Separator,
    Submenu(Submenu),
}

impl Menu {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            min_width: 180.0,
            background: None,
            close_handle: None,
            close_on_leave: false,
            expanded: Rc::new(RefCell::new(HashSet::new())),
            pending: Rc::new(RefCell::new(None)),
        }
    }

    /// 追加一个菜单项。
    pub fn item(mut self, item: MenuItem) -> Self {
        self.children.push(MenuChild::Item(item));
        self
    }

    /// 追加一个子菜单（带二级展开）。
    pub fn submenu(mut self, sub: Submenu) -> Self {
        self.children.push(MenuChild::Submenu(sub));
        self
    }

    /// 追加一条分隔线。
    pub fn separator(mut self) -> Self {
        self.children.push(MenuChild::Separator);
        self
    }

    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = w;
        self
    }

    /// 设置菜单背景色（覆盖主题默认值）。
    pub fn background(mut self, background: Color) -> Self {
        self.background = Some(background);
        self
    }

    /// 设置本菜单所属 Popup 句柄：构建时注入给所有菜单项，点击后自动关闭。
    /// 同时也作为本菜单内所有 `Submenu` 的父 popup 句柄（建立父子归属，级联关闭）。
    pub fn auto_close(mut self, handle: PopupHandle) -> Self {
        self.close_handle = Some(handle);
        self
    }

    /// 配置鼠标离开菜单区域时自动关闭。适合右键菜单等「移开即收」的场景。
    /// 默认关闭；若鼠标正移向已展开的子孙菜单，则主菜单挂起关闭，待其收起后再生效。
    pub fn close_on_leave(mut self, on: bool) -> Self {
        self.close_on_leave = on;
        self
    }
}

impl Default for Menu {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Menu {
    fn clone(&self) -> Self {
        Self {
            children: self.children.clone(),
            min_width: self.min_width,
            background: self.background,
            // 注意：`close_handle` 不应随 clone 传播给子菜单（避免子菜单误关父 popup）。
            // 每个 popup 在 `build` 时由 `show_popup_*` 注入各自的 handle，此处 clone
            // 仅用于 build 阶段复制构造体，最终 handle 以注入为准，因此置 None 是安全的。
            close_handle: None,
            close_on_leave: self.close_on_leave,
            expanded: Rc::clone(&self.expanded),
            pending: Rc::clone(&self.pending),
        }
    }
}

impl Clone for MenuChild {
    fn clone(&self) -> Self {
        match self {
            MenuChild::Item(i) => MenuChild::Item(i.clone()),
            MenuChild::Separator => MenuChild::Separator,
            MenuChild::Submenu(s) => MenuChild::Submenu(s.clone()),
        }
    }
}

impl Widget for Menu {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let mut col = Column::new().spacing(2.0).width(self.min_width);
        for child in &self.children {
            match child {
                MenuChild::Item(item) => {
                    let mut it = item.clone();
                    if let Some(h) = self.close_handle {
                        it = it.close_with(h);
                    }
                    // 进入普通菜单项时执行 pending：关闭上一个子菜单遗留的二级菜单，
                    // 覆盖「从子菜单主行移向普通项」这一原本不会触发关闭的路径。
                    let pending = Rc::clone(&self.pending);
                    let wrapped = Container::new().child(it).on_mouse_enter(move |_ctx| {
                        if let Some(p) = pending.borrow_mut().take() {
                            hide_popup(p);
                        }
                    });
                    col = col.child(wrapped);
                }
                MenuChild::Separator => {
                    // 进入分隔线同样执行 pending（分隔线虽不可交互，但鼠标可能途经）。
                    let pending = Rc::clone(&self.pending);
                    let wrapped =
                        Container::new()
                            .child(MenuSeparator::new())
                            .on_mouse_enter(move |_ctx| {
                                if let Some(p) = pending.borrow_mut().take() {
                                    hide_popup(p);
                                }
                            });
                    col = col.child(wrapped);
                }
                MenuChild::Submenu(sub) => {
                    // 把本菜单的「关闭句柄（作为父 popup）」「展开集合」与「pending 信号」
                    // 共享给直接子 Submenu：子菜单展开时登记父子归属，并由 expanded / pending
                    // 协同管理二级菜单的打开与关闭（移离自动关闭）。
                    let s = sub.clone().track_with(
                        self.close_handle,
                        Rc::clone(&self.expanded),
                        Rc::clone(&self.pending),
                    );
                    col = col.child(s);
                }
            }
        }
        let t = current();
        let bg = self.background.unwrap_or(t.menu.background);
        let mut c = Container::new().child(col);
        c = c.padding(4.0);
        c = c.background(bg);
        c = c.border_radius(t.radius.medium);
        c = c.border(1.0, t.menu.border);
        c = c.shadow(t.menu.shadow);
        // 集成「离开即关闭」特性（如右键菜单）。鼠标移出菜单整体时自动收起；
        // 但若仍有子孙菜单展开（鼠标正移向子菜单），挂起关闭，避免父菜单被误关
        // （支持多级嵌套：expanded 记录整条展开链）。
        if self.close_on_leave
            && let Some(h) = self.close_handle
        {
            let expanded = Rc::clone(&self.expanded);
            c = c.on_mouse_leave(move |_ctx| {
                if expanded.borrow().is_empty() {
                    hide_popup(h);
                }
            });
        }
        c.build(ctx)
    }
}

/// 单个菜单项：文案 + 可选右侧快捷键 + hover 高亮 + 点击回调。
pub struct MenuItem {
    label: String,
    hint: Option<String>,
    on_click: Option<MenuClickFn>,
    close_handle: Option<PopupHandle>,
    /// hover 高亮背景色。为 `None` 时使用当前主题的次级背景。
    hover_background: Option<Color>,
}

impl MenuItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            hint: None,
            on_click: None,
            close_handle: None,
            hover_background: None,
        }
    }

    /// 右侧快捷键提示文本（如 "Ctrl+C"）。
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// 设置点击回调（左键释放在项上时触发）。
    pub fn on_click(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }

    /// 绑定所属 Popup 句柄，点击后自动关闭该菜单（级联关闭其子树）。
    pub fn close_with(mut self, handle: PopupHandle) -> Self {
        self.close_handle = Some(handle);
        self
    }

    /// 设置 hover 高亮背景色（覆盖主题默认值）。
    pub fn hover_background(mut self, color: Color) -> Self {
        self.hover_background = Some(color);
        self
    }
}

impl Clone for MenuItem {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            hint: self.hint.clone(),
            on_click: self.on_click.clone(),
            close_handle: self.close_handle,
            hover_background: self.hover_background,
        }
    }
}

impl Widget for MenuItem {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let t = current();
        let row = Row::new()
            .spacing(8.0)
            .align_items(FlexAlign::Center)
            .width(160.0)
            .child(
                Text::new(self.label.clone())
                    .font_size(14.0)
                    .color(t.menu.item_text),
            );

        let row = if let Some(hint) = &self.hint {
            row.child(
                Text::new(hint.clone())
                    .font_size(12.0)
                    .color(t.menu.item_hint),
            )
        } else {
            row
        };

        let on_click = self.on_click.clone();
        let close_handle = self.close_handle;
        let hover = self
            .hover_background
            .unwrap_or(t.menu.item_hover_background);
        let mut c = Container::new().child(row);
        c = c.padding(6.0);
        c = c.hover_background(hover);
        c = c.on_mouse_up(move |ctx| {
            let is_left = matches!(
                ctx.event(),
                Some(&Event::MouseUp {
                    button: MouseButton::Left,
                    ..
                })
            );
            if !is_left {
                return;
            }
            if let Some(f) = &on_click {
                f(ctx);
            }
            // 点击项关闭本 popup，级联关闭其所有子孙（子菜单等）。
            if let Some(h) = close_handle {
                hide_popup(h);
            }
        });
        c.build(ctx)
    }
}

/// 菜单分隔线。
pub struct MenuSeparator {}

impl MenuSeparator {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for MenuSeparator {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for MenuSeparator {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let t = current();
        let mut c = Container::new();
        c = c.height(1.0);
        c = c.background(t.menu.divider);
        c = c.margin(4.0);
        c.build(ctx)
    }
}

/// 子菜单：菜单项 + 右侧箭头，hover 时在右侧展开二级 `Menu`。
///
/// 二级菜单通过 `show_popup_with_parent(.., parent = 父菜单句柄)` 以
/// `PopupPlacement::RightOf` 锚定到本项矩形，并登记为父菜单的**子 popup**。
/// 因此父菜单被关闭（点项 / 点外 / 离开 / 右键重复）时会**级联关闭本子菜单及其
/// 所有后代**（见 `LayerStack::close_popup`），彻底消除孤儿 popup。
pub struct Submenu {
    label: String,
    items: Menu,
    /// hover 高亮背景色。为 `None` 时使用当前主题的次级背景。
    hover_background: Option<Color>,
    /// 当前已展开的二级菜单句柄（结构体级 Rc 成员，重建后仍能追踪，避免泄漏）。
    open_handle: Rc<RefCell<Option<PopupHandle>>>,
    /// 父级 `Menu` 的 popup 句柄：二级菜单展开时作为 parent 登记父子归属。
    parent_handle: Option<PopupHandle>,
    /// 父级 `Menu` 共享过来的展开集合（记录整条展开链，用于 close_on_leave 挂起判定）。
    expanded: Option<Rc<RefCell<HashSet<PopupHandle>>>>,
    /// 父级 `Menu` 共享过来的「待关闭二级菜单」信号（见 `Menu::pending` 说明）。
    pending: Option<Rc<RefCell<Option<PopupHandle>>>>,
}

impl Submenu {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            items: Menu::new(),
            hover_background: None,
            open_handle: Rc::new(RefCell::new(None)),
            parent_handle: None,
            expanded: None,
            pending: None,
        }
    }

    pub fn item(mut self, item: MenuItem) -> Self {
        self.items = self.items.item(item);
        self
    }

    pub fn separator(mut self) -> Self {
        self.items = self.items.separator();
        self
    }

    /// 设置 hover 高亮背景色（覆盖主题默认值）。
    pub fn hover_background(mut self, color: Color) -> Self {
        self.hover_background = Some(color);
        self
    }

    /// 绑定父级共享信息：父菜单的 popup 句柄（建立归属）、展开集合（close_on_leave 挂起）
    /// 与 pending 信号（移离自动关闭）。由 `Menu::build` 注入。
    pub(crate) fn track_with(
        mut self,
        parent: Option<PopupHandle>,
        expanded: Rc<RefCell<HashSet<PopupHandle>>>,
        pending: Rc<RefCell<Option<PopupHandle>>>,
    ) -> Self {
        self.parent_handle = parent;
        self.expanded = Some(expanded);
        self.pending = Some(pending);
        self
    }
}

impl Clone for Submenu {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            items: self.items.clone(),
            hover_background: self.hover_background,
            // Rc 克隆共享同一底层句柄/展开集合，保证重建后仍能追踪打开的二级菜单。
            open_handle: Rc::clone(&self.open_handle),
            parent_handle: self.parent_handle,
            expanded: self.expanded.as_ref().map(Rc::clone),
            pending: self.pending.as_ref().map(Rc::clone),
        }
    }
}

impl Widget for Submenu {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let t = current();
        let row = Row::new()
            .spacing(8.0)
            .align_items(FlexAlign::Center)
            .width(160.0)
            .child(
                Text::new(self.label.clone())
                    .font_size(14.0)
                    .color(t.menu.item_text),
            )
            .child(
                Text::new("▶".to_string())
                    .font_size(10.0)
                    .color(t.menu.item_hint),
            );

        let sub = self.items.clone();
        let open = Rc::clone(&self.open_handle);
        let parent = self.parent_handle;
        let expanded = self.expanded.clone();
        let pending = self.pending.clone();
        let mut c = Container::new().child(row);
        c = c.padding(6.0);
        c = c.hover_background(
            self.hover_background
                .unwrap_or(t.menu.item_hover_background),
        );
        // 鼠标进入子菜单主行：
        // 1) 先执行 pending —— 关闭上一个子菜单遗留的二级菜单（延迟到此刻才真正关，
        //    因此「主行 → 二级菜单」的进入路径不会误关，二级的 on_mouse_enter 会取消它）。
        // 2) 再关闭自己可能残留的旧二级（切换子菜单时替换）。
        // 3) 展开新的二级菜单。
        c = c.on_mouse_enter({
            let pending = pending.clone();
            let open = open.clone();
            let expanded = expanded.clone();
            let sub = sub.clone();
            move |ctx| {
                if let Some(p) = pending.as_ref().and_then(|p| p.borrow_mut().take()) {
                    hide_popup(p);
                }
                if let Some(h) = open.borrow_mut().take() {
                    hide_popup(h);
                    if let Some(e) = &expanded {
                        e.borrow_mut().remove(&h);
                    }
                }
                let r = ctx
                    .current_rect()
                    .unwrap_or_else(|| crate::geometry::Rect::new(0.0, 0.0, 0.0, 0.0));
                let s = sub.clone();
                let expanded_for_builder = expanded.clone();
                let pending_for_cc = pending.clone();
                let open_for_cc = open.clone();
                // 二级菜单登记 parent 为父菜单句柄 → 父子归属，级联关闭。
                let h =
                    show_popup_with_parent(r, PopupPlacement::RightOf, parent, move |h, _ctx| {
                        // 把二级菜单的关闭句柄注入，点击项后关闭二级（连同其后代级联）。
                        let inner = s.clone().auto_close(h);
                        // 记录本子菜单展开（连同其后代）到展开集合，使父菜单 close_on_leave 挂起。
                        let e = expanded_for_builder.clone();
                        let pending_inner = pending_for_cc.clone();
                        let open_inner = open_for_cc.clone();
                        let mut cc = Container::new().child(inner);
                        // 进入二级菜单：取消 pending，避免「主行 leave 标记的待关」把
                        // 刚进入的二级菜单误关掉。
                        cc = cc.on_mouse_enter(move |_ctx| {
                            if let Some(p) = &pending_inner {
                                p.borrow_mut().take();
                            }
                        });
                        cc = cc.on_mouse_leave(move |_ctx| {
                            // 鼠标离开二级菜单：关闭它（级联关其后代），并同步展开集合
                            // 与 open_handle，避免句柄残留。
                            hide_popup(h);
                            if let Some(ee) = &e {
                                ee.borrow_mut().remove(&h);
                            }
                            open_inner.borrow_mut().take();
                        });
                        Box::new(cc)
                    });
                *open.borrow_mut() = Some(h);
                if let Some(e) = &expanded {
                    e.borrow_mut().insert(h);
                }
            }
        });
        // 鼠标离开子菜单主行：把当前展开的二级菜单句柄标记为「待关闭」，延迟到进入
        // 下一个目标时执行。若下一个目标是自己的二级菜单（on_mouse_enter 取消 pending）
        // 则保持打开；若是其它项或菜单外，则必然关闭 —— 实现「移离后自动关闭」。
        c = c.on_mouse_leave({
            let pending_for_leave = pending.clone();
            let open_for_leave = open.clone();
            move |_ctx| {
                if let Some(h) = *open_for_leave.borrow()
                    && let Some(p) = &pending_for_leave
                {
                    *p.borrow_mut() = Some(h);
                }
            }
        });
        c.build(ctx)
    }
}

/// 右键菜单容器：包裹任意触发 widget，监听右键 `MouseDown` 弹出菜单。
///
/// 右键按下时以鼠标坐标为锚点（`PopupPlacement::Below`）弹出，并自动给菜单
/// 设置 `auto_close(handle)`，点击菜单项后自动关闭。重复右键打开时，会先关闭
/// 上一次打开的菜单（级联），不再依赖点击外部关闭的巧合，嵌套右键菜单也能正确关闭。
///
/// 最近一次打开的菜单句柄通过 `use_state` 持久化，**跨每次重建复用**，因此即使
/// 触发源 widget 所在页面频繁 rebuild，重复右键仍能命中并关闭旧菜单（彻底消除
/// 「实例字段每次重建都被重置为空」导致的旧菜单残留）。
pub struct ContextMenu {
    trigger: Rc<dyn Widget>,
    menu: Option<MenuBuilder>,
    close_on_leave: bool,
}

impl ContextMenu {
    pub fn new(trigger: impl Widget + 'static) -> Self {
        Self {
            trigger: Rc::new(trigger),
            menu: None,
            close_on_leave: false,
        }
    }

    /// 设置右键弹出的菜单内容构建器（可拿到 PopupHandle 用于精确关闭）。
    /// 返回的 `Menu` 会被自动注入 `auto_close(handle)`，点击项后自动关闭。
    pub fn menu(mut self, f: impl Fn(PopupHandle, &mut BuildContext) -> Menu + 'static) -> Self {
        self.menu = Some(Rc::new(f));
        self
    }

    /// 右键菜单是否在鼠标移开后自动关闭（默认关闭）。
    pub fn close_on_leave(mut self, on: bool) -> Self {
        self.close_on_leave = on;
        self
    }
}

impl Widget for ContextMenu {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let close_on_leave = self.close_on_leave;
        // 跨重建持久化：最近一次打开的右键菜单 popup 句柄。
        let last: crate::widget::Stateful<Option<PopupHandle>> = ctx.use_state(|| None);
        let last_for_open = last.clone();
        let mut c = Container::new().child(self.trigger.clone());
        if let Some(menu) = self.menu.clone() {
            c = c.on_mouse_down(move |ctx| {
                let is_right = matches!(
                    ctx.event(),
                    Some(&Event::MouseDown {
                        button: MouseButton::Right,
                        ..
                    })
                );
                if !is_right {
                    return;
                }
                if let Some(&Event::MouseDown { x, y, .. }) = ctx.event() {
                    // 重复右键：先关闭上一次打开的菜单（级联关其子树），避免残留。
                    if let Some(old) = *last_for_open.get() {
                        hide_popup(old);
                    }
                    let anchor = crate::geometry::Rect::new(x, y, 1.0, 1.0);
                    let m = menu.clone();
                    let h = show_popup_with(anchor, PopupPlacement::Below, move |handle, ctx| {
                        Box::new(
                            m(handle, ctx)
                                .auto_close(handle)
                                .close_on_leave(close_on_leave),
                        )
                    });
                    last_for_open.set(Some(h));
                }
            });
        }
        c.build(ctx)
    }
}

/// 菜单按钮：点击后弹出下拉菜单的一站式组件。
///
/// 封装了 `Button` + `show_popup_with` + `auto_close` 的全部样板，使用者只需提供
/// 菜单内容构建器，无需手动处理弹出锚点、`PopupHandle` 或 `Box::new` 等细节。
/// 弹出的 `Menu` 已被自动注入 `auto_close(handle)`，点击项或点击外部均会自动关闭。
pub struct MenuButton {
    label: String,
    menu: Option<MenuBuilder>,
    placement: PopupPlacement,
    variant: ButtonVariant,
}

impl MenuButton {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            menu: None,
            placement: PopupPlacement::Below,
            variant: ButtonVariant::Secondary,
        }
    }

    /// 设置下拉菜单内容构建器（可拿到 `PopupHandle`，但框架会自动注入 `auto_close`）。
    pub fn menu(mut self, f: impl Fn(PopupHandle, &mut BuildContext) -> Menu + 'static) -> Self {
        self.menu = Some(Rc::new(f));
        self
    }

    /// 菜单相对按钮的弹出方位（默认 `Below`）。
    pub fn placement(mut self, p: PopupPlacement) -> Self {
        self.placement = p;
        self
    }

    /// 按钮外观变体（默认 `Secondary`）。
    pub fn variant(mut self, v: ButtonVariant) -> Self {
        self.variant = v;
        self
    }
}

impl Widget for MenuButton {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let m = self.menu.clone();
        let placement = self.placement;
        let variant = self.variant;
        Button::new(self.label.clone())
            .variant(variant)
            .on_click_with_ctx(move |ctx| {
                let r = ctx
                    .current_rect()
                    .unwrap_or_else(|| crate::geometry::Rect::new(0.0, 0.0, 0.0, 0.0));
                if let Some(m) = m.clone() {
                    show_popup_with(r, placement, move |handle, ctx| {
                        Box::new(m(handle, ctx).auto_close(handle))
                    });
                }
            })
            .build(ctx)
    }
}
