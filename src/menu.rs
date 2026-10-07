//! 菜单构件：`p.menu(|m| …)` —— 对齐 WinUI `MenuFlyout` / `MenuFlyoutItem` /
//! `MenuFlyoutSeparator`。
//!
//! ```ignore
//! v.popup_at_point(pos, Placement::Below, |p| {
//!     p.menu(|m| {
//!         m.item("在此页后插入 PDF…").on_tap(..);
//!         m.item("复制此页").accelerator("Ctrl+C").on_tap(..);
//!         m.separator();
//!         m.item("剪切此页").enabled(false);   // 禁用项不进路由（`collect_from` 查 enabled）
//!         m.item("删除此页").icon("delete").on_tap(..);
//!     });
//! });
//! ```
//!
//! ## 为什么"先攒规格、后统一渲染"
//!
//! `m.item("复制").checked(true).accelerator("Ctrl+C")` 里的 `checked` / `accelerator`
//! 都要**回头改这一行**（前面插勾、右侧加快捷键文本）。如果 `item()` 当场就把行建出来，
//! 后加的属性就只能"往当前容器追加"—— 那会把勾选标记加到标签**后面**。
//!
//! 所以 [`MenuRef`] 只往 `Vec<ItemSpec>` 里攒，`ViewBuf::menu` 在闭包**结束后**一次性渲染：
//! 哪些槽位出现（勾选列 / 图标列）、行高、内边距、配色全部集中在一处，
//! 以后增删属性**不动树形**。
//!
//! 顺带白拿两个 WinUI 行为：
//! - **勾选列 / 图标列按需出现** —— 整个菜单里没有任何一项被勾选，就不为所有项留空白列
//!   （否则每个标签都凭空缩进 18px）；
//! - **标签左对齐稳定** —— 有勾选/图标的项与没有的项，文字起始 x 相同。
//!
//! ## 明确不做（v1）
//!
//! - **级联子菜单**（`submenu`）：悬停展开 + 左右键切换 + 子菜单锚点跟随是**另一套会话状态**。
//!   半残的 `submenu`（能声明、不能展开）比没有更糟。真需要时先用
//!   `m.item(..).key(..)` + `v.popup_at(key, Placement::RightOf, ..)` 手动接。
//! - **键盘导航**（打开后方向键移动高亮 / Esc 关闭 / 首字母跳转）：弹层已有轻关闭与
//!   `Dismissed`，但键盘高亮要连同焦点链一起设计，不能靠 Tab 链硬凑。
//! - **多选（`IsChecked` 三态）**：`checked` 只是画勾，不是可勾选控件。

use std::rc::Rc;

use lieui_layout::{CSSDirection, Dimension, FlexAlign, FlexDirection};

use crate::event::{Ctx, EventKind, Handler};
use crate::icon::icon_char;
use crate::track::Key;
use crate::view::ViewBuf;

// ───────────────────────── 上下文菜单（挂到元素上）─────────────────────────

/// 挂在元素上的右键菜单构造器（[`DescRef::context_menu`] 存的就是它）。
///
/// 闭包在**菜单弹层被渲染时**才跑，所以它捕获的 `Rc` 状态总是当下的值；
/// 元素被回收（列表滚走、页面关掉）⇒ 框架发现目标节点没了，会自动收起菜单。
pub type ContextMenu = Rc<dyn Fn(&mut MenuRef<'_>)>;

/// 框架注入的上下文菜单弹层的层根标签。
///
/// 为什么要标签：菜单弹层是**框架**往 app 的描述树里追加的（见
/// `WindowCtx` 的会话逻辑），app 自己声明的 `popup_at` 也在同一个 `Layer::Popup` 组里。
/// 靠"最后一个根"这种顺序去认领会随 app 的弹层开关而漂 ⇒ 用固定标签认领
/// （与 loading 遮罩用 `modal_tagged` 找回层根同一手法）。
pub(crate) const CTX_MENU_TAG: u64 = 0x6374_785f_6d65_6e75;

// ───────────────────────── 视觉度量 ─────────────────────────

/// 菜单宽度下限（WinUI 的 flyout 同样有下限，否则"复制"这种短项会挤出个窄条）
pub const MENU_MIN_WIDTH: f32 = 168.0;
/// 菜单上下内边距（首末项与边框之间）
const MENU_PAD_Y: f32 = 4.0;
/// 菜单项行高（≈ 文本行盒 + 上下内边距；供调用方估算菜单尺寸）
pub const ITEM_HEIGHT: f32 = 26.0;
const ITEM_PAD_Y: f32 = 4.0;
const ITEM_PAD_X: f32 = 10.0;
const ITEM_GAP: f32 = 8.0;
const ITEM_FONT: f64 = 13.0;
/// 勾选 / 图标槽宽：定宽 ⇒ 有无该项时标签左对齐一致
const SLOT_W: f32 = 18.0;
const SLOT_FONT: f64 = 15.0;
const ACCEL_FONT: f64 = 12.0;
const SEPARATOR_H: f32 = 1.0;
const SEPARATOR_PAD_Y: f32 = 4.0;

// ───────────────────────── 规格 ─────────────────────────

#[derive(Default)]
struct ItemSpec {
    label: String,
    /// Material 图标名（`crate::icon`）
    icon: Option<String>,
    /// 右侧快捷键提示文本（只是**显示**，不注册真快捷键）
    accelerator: Option<String>,
    checked: bool,
    enabled: bool,
    /// 供调用方 `popup_at` 找回这一项（手动级联 / 自定义浮层）
    key: Option<Key>,
    /// 点击（对齐 WinUI：菜单项的点击 = `Tapped`）
    tap: Option<Handler>,
    /// 其它事件（右键、悬停……）
    other: Vec<(EventKind, Handler)>,
}

enum Kind {
    Item(ItemSpec),
    Separator,
}

// ───────────────────────── 声明入口 ─────────────────────────

impl ViewBuf {
    /// 菜单（对齐 WinUI `MenuFlyout`）：**声明在弹层闭包内**。
    ///
    /// 菜单本身只是个纵向容器 + 一串行；打开、定位、轻关闭都是弹层的事
    /// （见 [`ViewBuf::popup_at`] / [`ViewBuf::popup_at_point`]）。所以这里只管
    /// "菜单长什么样"，不掺任何状态。
    ///
    /// ```ignore
    /// v.popup_at("btn", Placement::Below, |p| {
    ///     p.menu(|m| {
    ///         m.item("打开").on_tap(..);
    ///         m.separator();
    ///         m.item("退出").accelerator("Alt+F4").on_tap(..);
    ///     });
    /// });
    /// ```
    pub fn menu(&mut self, f: impl FnOnce(&mut MenuRef<'_>)) {
        let mut m = MenuRef {
            v: self,
            items: Vec::new(),
            min_width: MENU_MIN_WIDTH,
        };
        f(&mut m);
        m.render();
    }
}

/// 菜单声明句柄（只在 [`ViewBuf::menu`] 的闭包里活着）
pub struct MenuRef<'a> {
    v: &'a mut ViewBuf,
    items: Vec<Kind>,
    min_width: f32,
}

impl<'a> MenuRef<'a> {
    /// 追加一个菜单项。
    ///
    /// 返回的是**规格句柄**：链上的每个方法只是往规格里写字段，
    /// 真正建节点在 [`ViewBuf::menu`] 闭包结束时统一做（见模块文档）。
    pub fn item(&mut self, label: impl Into<String>) -> MenuItemRef<'_> {
        self.items.push(Kind::Item(ItemSpec {
            label: label.into(),
            enabled: true,
            ..ItemSpec::default()
        }));
        let idx = self.items.len() - 1;
        let items = &mut self.items;
        MenuItemRef { items, idx }
    }

    /// 分隔线（对齐 `MenuFlyoutSeparator`）
    pub fn separator(&mut self) {
        self.items.push(Kind::Separator);
    }

    /// 菜单宽度下限（默认 [`MENU_MIN_WIDTH`]）。
    ///
    /// 菜单是"内容自适应"的（弹层层根用未定义可用空间布局），所以给的是**下限**：
    /// 标签很长时菜单会自己变宽，不会被这个值截断。
    pub fn min_width(&mut self, w: f32) -> &mut Self {
        self.min_width = w;
        self
    }

    // ── 渲染 ──

    fn render(&mut self) {
        let theme = *self.v.theme();
        let min_w = self.min_width;
        let items = std::mem::take(&mut self.items);

        // 勾选列 / 图标列按需出现：**整个菜单里没人勾选就不留空白列**
        let any_checked = items.iter().any(|k| match k {
            Kind::Item(i) => i.checked,
            Kind::Separator => false,
        });
        let any_icon = items.iter().any(|k| match k {
            Kind::Item(i) => i.icon.is_some(),
            Kind::Separator => false,
        });

        // ⚠ 整个渲染都在**菜单容器的闭包内**：闭包一返回，`container_ref` 就把栈弹回
        // 弹层根，此后 `push_desc` 造出来的节点会挂到**弹层根**上（菜单容器变空壳）。
        let _root = self.v.container_ref(|menu| {
            // 菜单容器：纵向、项间无额外间距（间距由项自己的内边距给）、最小宽度
            menu.gap(0.0);
            menu.padding_y(MENU_PAD_Y);
            menu.layout(|s| s.min_dim[Dimension::Width as usize] = min_w);

            for kind in items {
                match kind {
                    Kind::Separator => {
                        // 上下留白 + 1px 实线：用外边距而不是"两个空行"，
                        // 这样分隔线永远贯穿菜单整宽（含 hover 高亮区的宽度）
                        menu.container(|line| {
                            line.height(SEPARATOR_H);
                            line.background(theme.control_border);
                            line.layout(|s| {
                                s.set_margin(CSSDirection::Top, SEPARATOR_PAD_Y);
                                s.set_margin(CSSDirection::Bottom, SEPARATOR_PAD_Y);
                            });
                        });
                    }
                    Kind::Item(spec) => {
                        let row = menu.container_ref(|row| {
                            // `container_ref` 固定纵向（与 `container` 同义），菜单项要**横向**：
                            // 标签 … 撑开 … 快捷键 排在一条线上
                            row.layout(|s| s.flex_direction = FlexDirection::Row);
                            row.gap(ITEM_GAP);
                            row.align_items(FlexAlign::Center);
                            row.padding_y(ITEM_PAD_Y);
                            // 水平内边距用 `set_padding`：它同时维护 `padding_from`（继承关系），
                            // 直接写 `padding[..]` 数组会绕过这套 bookkeeping
                            row.layout(|s| {
                                s.set_padding(CSSDirection::Left, ITEM_PAD_X);
                                s.set_padding(CSSDirection::Right, ITEM_PAD_X);
                            });

                            // 勾选槽：定宽空文本，勾上的那项画 ✓
                            if any_checked {
                                let mark = if spec.checked {
                                    icon_char("check").to_string()
                                } else {
                                    String::new()
                                };
                                let _ = row.text(mark).font_size(SLOT_FONT).width(SLOT_W).color(theme.accent);
                            }
                            // 图标槽：同理
                            if any_icon {
                                let glyph = match &spec.icon {
                                    Some(name) => icon_char(name).to_string(),
                                    None => String::new(),
                                };
                                let _ = row
                                    .text(glyph)
                                    .font_size(SLOT_FONT)
                                    .width(SLOT_W)
                                    .color(theme.text_secondary);
                            }

                            // 禁用项的**颜色**由主题的次要色表达，而不是给子节点打
                            // `enabled(false)`：那会让渲染层再乘一次半透明（`dim_if_disabled`）
                            // ⇒ 灰两次。行为上的"点不动"由事件路由阶段的 `enabled` 检查负责。
                            let fg = if spec.enabled { theme.text } else { theme.text_secondary };
                            let _ = row.text(spec.label.clone()).font_size(ITEM_FONT).color(fg);

                            // 撑开：把快捷键推到行尾（弹层收缩到内容时这条 spacer
                            // 与 min_width 一起给出"标签左、快捷键右"的稳定版式）
                            row.spacer();

                            if let Some(accel) = &spec.accelerator {
                                let _ = row
                                    .text(accel.clone())
                                    .font_size(ACCEL_FONT)
                                    .color(theme.text_secondary);
                            }
                        });

                        // 行自身：圆角 + hover / 按下底色 + 禁用标记
                        let mut rr = row
                            .radius(theme.control_radius)
                            .paint(|p| {
                                p.hover_background = Some(theme.control_hover);
                                p.pressed_background = Some(theme.control_pressed);
                            })
                            .enabled(spec.enabled);

                        if let Some(k) = spec.key {
                            rr = rr.key(k);
                        }
                        for (kind, h) in spec.other {
                            rr = rr.handler(kind, h);
                        }
                        if let Some(h) = spec.tap {
                            rr = rr.handler(EventKind::Tapped, h);
                        }
                    }
                }
            }
        });
    }
}

/// 菜单项规格句柄（[`MenuRef::item`] 的返回值）
pub struct MenuItemRef<'b> {
    items: &'b mut Vec<Kind>,
    idx: usize,
}

impl<'b> MenuItemRef<'b> {
    fn spec(&mut self) -> &mut ItemSpec {
        match &mut self.items[self.idx] {
            Kind::Item(s) => s,
            Kind::Separator => unreachable!("separator() 不返回 MenuItemRef"),
        }
    }

    /// 项前图标（Material 图标名，见 [`crate::icon`]）
    pub fn icon(mut self, name: impl AsRef<str>) -> Self {
        self.spec().icon = Some(name.as_ref().into());
        self
    }

    /// 右侧快捷键提示（`"Ctrl+C"`）。**只是显示**，不注册真快捷键。
    pub fn accelerator(mut self, s: impl Into<String>) -> Self {
        self.spec().accelerator = Some(s.into());
        self
    }

    /// 勾选标记（在标签前画 ✓）。只控制显示，不是可勾选控件。
    pub fn checked(mut self, v: bool) -> Self {
        self.spec().checked = v;
        self
    }

    /// 禁用（灰字 + **不响应点击**：框架在事件路由阶段就跳过禁用节点）
    pub fn enabled(mut self, v: bool) -> Self {
        self.spec().enabled = v;
        self
    }

    /// 给这一项一个 key，便于外部 `v.popup_at(key, Placement::RightOf, ..)` 挂子浮层。
    pub fn key(mut self, k: impl Into<Key>) -> Self {
        self.spec().key = Some(k.into());
        self
    }

    /// 无参回调 → 包一层（最常见形态）
    pub fn on_tap(self, f: impl Fn() + 'static) -> Self {
        self.on_tap_with(move |_| f())
    }

    /// 需要 `cx`（请求重绘 / 开窗 / 起任务）时用这个
    pub fn on_tap_with(mut self, f: impl Fn(&mut Ctx) + 'static) -> Self {
        self.spec().tap = Some(Rc::new(f));
        self
    }

    /// 注册任意事件的处理器（`on_tap` 之外的，如 `RightTapped`）
    pub fn on(mut self, kind: EventKind, f: impl Fn(&mut Ctx) + 'static) -> Self {
        self.spec().other.push((kind, Rc::new(f)));
        self
    }
}

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;
