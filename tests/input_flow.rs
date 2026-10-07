//! **输入链路的黑盒验收**（集成测试）—— 外部用户视角。
//!
//! ## 为什么要有这个文件（测试分层的实际价值）
//!
//! - **单元**（`src/app_tests.rs`，91 个）：能看到模块私有项，验证**内部机制**。
//! - **集成**（本文件）：只有 `lieui::` **公开 API**，验证**用户链路**。
//!
//! 单元测试能断言 `FrameStats` 字段，而用户关心"我点了按钮，数字变了"。
//! **若一个行为只被单元测试覆盖，"它对用户是否可用"就从未被验证过** ——
//! 本轮 `app.rs` / `window.rs` 拆模块时 91 个单元测试全部原样通过，
//! 但它们**一个都没走公开 API 路径**；只有本文件能证明"用户视角没坏"。

use std::rc::Rc;

use lieui::app::{App, ViewModel, erased};
use lieui::event::{Event, EventKind, KeyCode, NamedKey, PointerButton, PointerId};
use lieui::prelude::{Placement, Point, Signal, ViewBuf, WindowConfig};
use lieui::track::{Key, Kind, NodeId};
use lieui::{Runtime, WindowId};

struct Ui {
    count: Signal<i32>,
    popup: Signal<bool>,
    taps: Signal<usize>,
}

impl ViewModel for Ui {
    fn view(self: &Rc<Self>, v: &mut ViewBuf) {
        let me = self.clone();
        v.column(|c| {
            c.text(me.count.get().to_string()).key("count");
            let a = self.clone();
            c.button("+1").key("inc").on_tap(move || {
                a.taps.set(a.taps.get() + 1);
                a.count.set(a.count.get() + 1);
            });
            let b = self.clone();
            c.button("弹层").key("pop").on_tap(move || b.popup.set(!b.popup.get()));
        });
        // 弹层声明在 `column` **之外**：写在闭包内会二次独占借用 `v`
        // （`v.column(|c| … v.popup_at(…) …)` ⇒ E0500/E0501）。
        if self.popup.get() {
            let me2 = self.clone();
            v.popup_at("pop", Placement::Below, |p| {
                p.on(EventKind::Dismissed, move |_| me2.popup.set(false));
                p.text("弹层内容");
            });
        }
    }
}

fn setup() -> (Runtime, App, Rc<Ui>, WindowId) {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());
    let ui = Rc::new(Ui {
        count: Signal::new(&rt, 0),
        popup: Signal::new(&rt, false),
        taps: Signal::new(&rt, 0),
    });
    let id = app.window_erased(WindowConfig::new().title("t"), erased(ui.clone()));
    app.frame_all();
    (rt, app, ui, id)
}

fn node_by_key(app: &App, id: WindowId, key: &str) -> NodeId {
    let t = app.window_ctx(id).expect("窗口存在").track();
    let root = t.content_root().expect("内容根").node;
    t.descendants(root)
        .into_iter()
        .find(|n| t.get(*n).and_then(|x| x.key.as_ref()) == Some(&Key::from(key)))
        .unwrap_or_else(|| panic!("找不到 key={key}"))
}

/// 全树搜索文本。
///
/// ★ 必须遍历**所有层根**，不能只查 `content_root()` ——
///   弹层 / 菜单在 `Popup` 层，`content_root()` 只覆盖 `Content` 层。
///   （第一版就是这么写的，于是"弹层内容已渲染"这条断言恒假。）
fn text_present(app: &App, id: WindowId, needle: &str) -> bool {
    let t = app.window_ctx(id).unwrap().track();
    t.roots()
        .iter()
        .flat_map(|r| t.descendants(r.node))
        .any(|n| matches!(t.get(n).map(|x| &x.kind), Some(Kind::Text(s)) if s == needle))
}

/// 一次**真实的**点击：命中测试 → 按下 → 抬起（不绕过命中测试）。
fn click(app: &mut App, rt: &Runtime, id: WindowId, node: NodeId) {
    let r = app.window_ctx(id).unwrap().track().get(node).unwrap().rect();
    let pos = Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5);
    let p = PointerId(0);
    let w = app.window_ctx_mut(id).unwrap();
    for kind in [EventKind::PointerPressed, EventKind::Tapped] {
        let ev = Event::pointer(kind, p, pos, PointerButton::Left);
        let path = w.hit(pos);
        w.dispatch(rt, &path, &ev);
    }
}
/// ★ 点按钮 → 计数加一 → **下一帧**渲染出 "1"。
///
/// 这是"用户点了没反应"这类报障的最小复现路径。
#[test]
fn click_updates_state_and_next_frame_shows_it() {
    let (rt, mut app, ui, id) = setup();
    let inc = node_by_key(&app, id, "inc");
    click(&mut app, &rt, id, inc);
    assert_eq!(ui.taps.get(), 1, "★ 处理器被调用一次");
    assert_eq!(ui.count.get(), 1, "★ Signal 必须改变");
    app.frame_all();
    assert!(
        text_present(&app, id, "1"),
        "★ 下一帧必须渲染出 '1'（Signal 变了但 UI 没更新 = 用户点了没反应）"
    );
}

/// ★ 反方向：点空白处**不**误触发。
///
/// 只测"点对了能中"的话，把命中测试改成"什么都返回按钮"也能通过；
/// 真正的回归是**误触发**。
#[test]
fn clicking_outside_does_nothing() {
    let (rt, mut app, ui, id) = setup();
    let outside = Point::new(2.0, 2.0);
    let p = PointerId(0);
    {
        let w = app.window_ctx_mut(id).unwrap();
        for kind in [EventKind::PointerPressed, EventKind::Tapped] {
            let ev = Event::pointer(kind, p, outside, PointerButton::Left);
            let path = w.hit(outside);
            w.dispatch(&rt, &path, &ev);
        }
    }
    assert_eq!(ui.taps.get(), 0, "★ 点空白不得触发处理器");
}

/// Tab 在**没有 `tab_stop` 节点**时返回 `None`（不移动焦点）。
///
/// 这条测的是既有设计而非缺陷：`tab_order` 只收 `n.tab_stop` 的节点，默认关闭
/// ⇒ 空 Tab 链。（第一版我以为"按钮天然可聚焦"，写了 `assert!(first.is_some())`
/// —— 失败后发现是**我的期望错了**，不是代码错了。）
#[test]
fn tab_without_tab_stop_nodes_is_inert() {
    let (rt, mut app, _ui, id) = setup();
    assert!(app.window_ctx(id).unwrap().track().focused.is_none(), "初始无焦点");
    let target = app.window_ctx_mut(id).unwrap().tab(&rt, true);
    assert!(target.is_none(), "没有 tab_stop 节点时 Tab 不应移动焦点");
    assert!(app.window_ctx(id).unwrap().track().focused.is_none(), "焦点应保持为空");
}

/// 打开弹层 → Escape 关闭（含内容与层根检查）。
///
/// D15 修的"Escape 关不了弹层"是用户级缺陷，此前**只有单元测试**覆盖。
#[test]
fn escape_closes_a_popup_opened_from_the_ui() {
    let (rt, mut app, ui, id) = setup();
    let pop = node_by_key(&app, id, "pop");
    click(&mut app, &rt, id, pop);
    app.frame_all();
    assert!(ui.popup.get(), "★ 弹层应已打开");
    assert!(text_present(&app, id, "弹层内容"), "弹层内容应已渲染");

    let out = app
        .window_ctx_mut(id)
        .unwrap()
        .key(&rt, Event::key(EventKind::KeyDown, KeyCode::Named(NamedKey::Escape)));
    assert!(out.handled, "★ Escape 应被当作已处理");
    app.frame_all();
    assert!(!ui.popup.get(), "★ Escape 后弹层应关闭");
    assert!(!text_present(&app, id, "弹层内容"), "弹层内容应已从树上消失");
}

/// ★ 反向：**其它按键不得关闭弹层**。
///
/// 陷阱：Escape 是 `KeyCode` 变体而非 `EventKind` 变体，
/// 只判 `kind == KeyDown` 会让**任意键**都关弹层 —— 而正向测试照样通过。
#[test]
fn other_keys_leave_the_popup_open() {
    let (rt, mut app, ui, id) = setup();
    let pop = node_by_key(&app, id, "pop");
    click(&mut app, &rt, id, pop);
    app.frame_all();
    for code in [KeyCode::Named(NamedKey::Enter), KeyCode::Char('x')] {
        app.window_ctx_mut(id)
            .unwrap()
            .key(&rt, Event::key(EventKind::KeyDown, code));
        app.frame_all();
        assert!(ui.popup.get(), "★ 按键 {code:?} 不得关闭弹层");
    }
}
