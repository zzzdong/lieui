//! 菜单组件：自绘弹出菜单、菜单项、分隔线、子菜单、右键菜单容器。
//!
//! 设计采用「方案 A：纯自绘 + 防溢出翻转」。菜单落在本窗口的 `LayerKind::Popup`
//! 层（`LayerStack` 已是每窗口独立字段），自动设置 `FocusPolicy::Dismissable`
//! （点击浮层外部自动关闭），并由 `apply_anchor` 在定位超出视口时自动翻转到对侧，
//! 保证菜单始终完整落在窗口内，不依赖系统原生菜单控件。
//!
//! 典型用法：
//! ```ignore
//! // 右键菜单：ContextMenu 包裹任意触发 widget，内部监听右键弹出。
//! ContextMenu::new(trigger_widget)
//!     .menu(|_handle, ctx| {
//!         Menu::new()
//!             .item(MenuItem::new("复制").on_click(|_| copy()))
//!             .item(MenuItem::new("粘贴").on_click(|_| paste()))
//!             .separator()
//!             .item(MenuItem::new("删除").on_click(|_| delete()))
//!     });
//!
//! // 下拉/弹出菜单：在按钮的点击回调里手动弹出。
//! let btn = Button::new("菜单").on_click_with_ctx(|ctx| {
//!     let r = ctx.current_rect().unwrap_or_default();
//!     show_popup_with(r, PopupPlacement::Below, |_h, _ctx| {
//!         Box::new(Menu::new().item(MenuItem::new("项")))
//!     });
//! });
//! ```

use crate::event::{Event, EventContext, MouseButton};
use crate::geometry::Color;
use crate::prelude::FlexAlign;
use crate::state::{PopupHandle, PopupPlacement, hide_popup, show_popup_with};
use crate::view::node::ViewNode;
use crate::view::paint::ShadowSpec;
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
    background: Color,
    close_handle: Option<PopupHandle>,
    /// 鼠标离开菜单区域时是否自动关闭（如右键菜单场景）。
    close_on_leave: bool,
    /// 记录本菜单当前已展开的二级菜单句柄集合。
    ///
    /// 由本菜单构建时共享给其直接子 `Submenu`：子菜单展开/收起时写入/移除句柄，
    /// 主菜单 `close_on_leave` 判定时据此在「仍有子菜单展开」时挂起关闭——
    /// 这样鼠标从主菜单移向右侧子菜单时不会误关主菜单（解决 close_on_leave 与子菜单冲突）。
    submenu_tracker: Rc<RefCell<HashSet<PopupHandle>>>,
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
            background: Color::WHITE,
            close_handle: None,
            close_on_leave: false,
            submenu_tracker: Rc::new(RefCell::new(HashSet::new())),
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

    /// 设置本菜单所属 Popup 句柄：构建时注入给所有菜单项，点击后自动关闭。
    pub fn auto_close(mut self, handle: PopupHandle) -> Self {
        self.close_handle = Some(handle);
        self
    }

    /// 配置鼠标离开菜单区域时自动关闭。适合右键菜单等「移开即收」的场景。
    /// 默认关闭；若鼠标正移向已展开的子菜单，则主菜单挂起关闭，待子菜单收起后再生效。
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
            close_handle: self.close_handle,
            close_on_leave: self.close_on_leave,
            submenu_tracker: Rc::clone(&self.submenu_tracker),
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
                    col = col.child(it);
                }
                MenuChild::Separator => col = col.child(MenuSeparator::new()),
                MenuChild::Submenu(sub) => {
                    // 把本菜单的子菜单跟踪器共享给直接子 Submenu，用于解决
                    // close_on_leave 与子菜单的关闭冲突（P3）。
                    let s = sub.clone().track_with(Rc::clone(&self.submenu_tracker));
                    col = col.child(s);
                }
            }
        }
        let mut c = Container::new().child(col);
        c = c.padding(4.0);
        c = c.background(self.background);
        c = c.border_radius(8.0);
        c = c.border(1.0, Color::rgba(210, 214, 220, 255));
        c = c.shadow(ShadowSpec {
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 16.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 40),
        });
        // 集成「离开即关闭」特性：鼠标移出菜单整体时自动收起（点击项/点击外部关闭
        // 仍由 auto_close 与 LayerStack 的 Dismissable 机制负责，此处为额外开关）。
        if self.close_on_leave
            && let Some(h) = self.close_handle
        {
            let tracker = Rc::clone(&self.submenu_tracker);
            c = c.on_mouse_leave(move |_ctx| {
                // 若仍有子菜单展开（鼠标正移向子菜单），挂起关闭，避免主菜单被误关。
                if tracker.borrow().is_empty() {
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
    hover_background: Color,
}

impl MenuItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            hint: None,
            on_click: None,
            close_handle: None,
            hover_background: Color::rgba(232, 240, 254, 255),
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

    /// 绑定所属 Popup 句柄，点击后自动关闭该菜单。
    pub fn close_with(mut self, handle: PopupHandle) -> Self {
        self.close_handle = Some(handle);
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
        let row = Row::new()
            .spacing(8.0)
            .align_items(FlexAlign::Center)
            .width(160.0)
            .child(Text::new(self.label.clone()).font_size(14.0));

        let row = if let Some(hint) = &self.hint {
            row.child(
                Text::new(hint.clone())
                    .font_size(12.0)
                    .color(Color::rgba(140, 140, 140, 255)),
            )
        } else {
            row
        };

        let on_click = self.on_click.clone();
        let close_handle = self.close_handle;
        let mut c = Container::new().child(row);
        c = c.padding(6.0);
        c = c.hover_background(self.hover_background);
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
            if let Some(h) = close_handle {
                hide_popup(h);
            }
        });
        c.build(ctx)
    }
}

/// 菜单分隔线。
pub struct MenuSeparator {
    color: Color,
}

impl MenuSeparator {
    pub fn new() -> Self {
        Self {
            color: Color::rgba(225, 228, 232, 255),
        }
    }
}

impl Default for MenuSeparator {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for MenuSeparator {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let mut c = Container::new();
        c = c.height(1.0);
        c = c.background(self.color);
        c = c.margin(4.0);
        c.build(ctx)
    }
}

/// 子菜单：菜单项 + 右侧箭头，hover 时在右侧展开二级 `Menu`。
///
/// 二级菜单通过 `show_popup_with` 以 `PopupPlacement::RightOf` 锚定到本项矩形，
/// 复用同一套 `Popup` + `Dismissable` 机制，天然支持嵌套。
pub struct Submenu {
    label: String,
    items: Menu,
    hover_background: Color,
    /// 当前已展开的二级菜单句柄（结构体级 Rc 成员，重建后仍能追踪，避免泄漏）。
    open_handle: Rc<RefCell<Option<PopupHandle>>>,
    /// 父级 `Menu` 共享过来的子菜单跟踪器（用于 close_on_leave 挂起判定）。无则 None。
    tracker: Option<Rc<RefCell<HashSet<PopupHandle>>>>,
}

impl Submenu {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            items: Menu::new(),
            hover_background: Color::rgba(232, 240, 254, 255),
            open_handle: Rc::new(RefCell::new(None)),
            tracker: None,
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

    /// 绑定父级共享的子菜单跟踪器（由 `Menu::build` 注入）。
    pub(crate) fn track_with(mut self, tracker: Rc<RefCell<HashSet<PopupHandle>>>) -> Self {
        self.tracker = Some(tracker);
        self
    }
}

impl Clone for Submenu {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            items: self.items.clone(),
            hover_background: self.hover_background,
            // Rc 克隆共享同一底层句柄/跟踪器，保证重建后仍能追踪打开的二级菜单。
            open_handle: Rc::clone(&self.open_handle),
            tracker: self.tracker.as_ref().map(Rc::clone),
        }
    }
}

impl Widget for Submenu {
    fn build(&self, ctx: &mut BuildContext) -> ViewNode {
        let row = Row::new()
            .spacing(8.0)
            .align_items(FlexAlign::Center)
            .width(160.0)
            .child(Text::new(self.label.clone()).font_size(14.0))
            .child(
                Text::new("▶".to_string())
                    .font_size(10.0)
                    .color(Color::rgba(140, 140, 140, 255)),
            );

        let sub = self.items.clone();
        let open = Rc::clone(&self.open_handle);
        let tracker = self.tracker.clone();
        let mut c = Container::new().child(row);
        c = c.padding(6.0);
        c = c.hover_background(self.hover_background);
        c = c.on_mouse_enter(move |ctx| {
            // 切换父项时关闭上次打开的二级菜单（hover 到别的项时替换）。
            if let Some(h) = open.borrow_mut().take() {
                hide_popup(h);
                if let Some(t) = &tracker {
                    t.borrow_mut().remove(&h);
                }
            }
            let r = ctx
                .current_rect()
                .unwrap_or_else(|| crate::geometry::Rect::new(0.0, 0.0, 0.0, 0.0));
            let s = sub.clone();
            let tracker_for_builder = tracker.clone();
            let h = show_popup_with(r, PopupPlacement::RightOf, move |h, _ctx| {
                // 把二级菜单的关闭句柄注入，点击项后关闭二级（父级菜单由
                // 用户点击父项或点击外部关闭）。
                let inner = s.clone().auto_close(h);
                // 鼠标离开二级菜单区域后自动关闭它，并同步更新父级跟踪器。
                let t = tracker_for_builder.clone();
                let mut cc = Container::new().child(inner);
                cc = cc.on_mouse_leave(move |_ctx| {
                    hide_popup(h);
                    if let Some(tt) = &t {
                        tt.borrow_mut().remove(&h);
                    }
                });
                Box::new(cc)
            });
            *open.borrow_mut() = Some(h);
            if let Some(t) = &tracker {
                t.borrow_mut().insert(h);
            }
        });
        c.build(ctx)
    }
}

/// 右键菜单容器：包裹任意触发 widget，监听右键 `MouseDown` 弹出菜单。
///
/// 右键按下时以鼠标坐标为锚点（`PopupPlacement::Below`）弹出，并自动给菜单
/// 设置 `auto_close(handle)`，点击菜单项后自动关闭。
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
                    let anchor = crate::geometry::Rect::new(x, y, 1.0, 1.0);
                    let m = menu.clone();
                    show_popup_with(anchor, PopupPlacement::Below, move |handle, ctx| {
                        Box::new(
                            m(handle, ctx)
                                .auto_close(handle)
                                .close_on_leave(close_on_leave),
                        )
                    });
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
