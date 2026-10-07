//! **API 契约测试（黑盒）** —— 以外部用户视角编译并使用 `lieui`。
//!
//! ## 为什么需要它
//!
//! 本仓库其余 ~390 个测试全部内嵌在实现文件里（`#[cfg(test)] mod tests`），
//! 能访问 `pub(crate)` 私有字段 —— 它们保护的是**实现**，不是**公开 API**。
//! 于是有一条覆盖盲区：
//!
//! > 某个 `pub` 方法被改名 / 改签名 / 被删，而所有内嵌测试仍然通过
//! > （因为它们要么用旧名字、要么根本不调它）⇒ **破坏只会在用户编译时暴露**。
//!
//! 本文件没有任何私有访问：**编译能过就说明 prelude / 公开 API 至少自洽可用**。
//!
//! ## 与其它测试的分工
//!
//! | 位置 | 可见性 | 保护对象 | 放什么 |
//! |---|---|---|---|
//! | `#[cfg(test)] mod tests`（现有 ~390 个） | 含私有字段 | **实现** | 算法不变量、状态机、脏区计算 |
//! | `tests/*.rs`（本文件） | **仅 `pub` API** | **公开契约** | prelude 可用性、端到端场景、API 形状 |
//! | `///` 代码块（doc test） | 仅 `pub` API | **文档正确性** | 对外示例（当前全部 `ignore`） |
//!
//! **写法约定**：优先"能编译"而非"堆断言"。最大的价值是让 `cargo test` 阶段
//! 就暴露 API 破损；行为断言只取最稳的几条。
//!
//! **已知 API 缺陷在此登记**（不是测试写错，是真的该修）：
//! - `padding` / `gap` 返回 `()` 而非 `Self` ⇒ 无法链式（D45）。

use std::cell::Cell;
use std::rc::Rc;

use lieui::prelude::*;
use lieui::{CloseAction, CmdBuf, ExternalData, ImageStyle, Signal, ViewBuf, ViewModel};

// ─────────────────────── 端到端 ───────────────────────

/// 最小可用 ViewModel：文本反映状态，按钮改状态。
struct Counter {
    count: Signal<i32>,
}

impl ViewModel for Counter {
    fn view(self: &Rc<Self>, v: &mut ViewBuf) {
        let n = self.count.get();
        v.column(|c| {
            c.text(format!("count = {n}"));
            c.button("+1").on_tap({
                let this = Rc::clone(self);
                move || this.count.update(|x| *x += 1)
            });
        });
    }
}

#[test]
fn minimal_view_model_runs_end_to_end() {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());
    let vm = Counter {
        count: Signal::new(&rt, 0),
    };
    let _id = app.window(WindowConfig::new().size(200.0, 120.0), vm);
    let first = app.frame_all();
    assert!(first[0].1.view_ran, "首帧必须跑 view()");
    assert!(first[0].1.layout.ran, "首帧必须重排");

    let second = app.frame_all();
    assert!(second[0].1.is_idle(), "无变化时第二帧应空闲：{:?}", second[0].1);
}

#[test]
fn signal_change_marks_dirty_and_reruns_view() {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());
    let sig = Signal::new(&rt, 1i32);
    struct S(Signal<i32>);
    impl ViewModel for S {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            v.column(|c| {
                c.text(format!("v = {}", self.0.get()));
            });
        }
    }
    let _ = app.window(WindowConfig::new(), S(sig.clone()));
    app.frame_all();
    assert!(app.frame_all()[0].1.is_idle(), "基线应当空闲");

    sig.set(42);
    assert!(app.frame_all()[0].1.view_ran, "Signal 变更后必须重跑 view()");
}

#[test]
fn close_request_can_be_cancelled_and_is_repeatable() {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());
    struct Guard;
    impl ViewModel for Guard {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            v.column(|c| {
                c.text("g");
            });
        }
        fn on_close_request(self: &Rc<Self>, _cx: &mut Ctx) -> CloseAction {
            CloseAction::Cancel
        }
    }
    let id = app.window(WindowConfig::new(), Guard);
    app.frame_all();
    let w = app.window_ctx_mut(id).expect("窗口应存在");
    assert_eq!(w.close_requested(&rt), CloseAction::Cancel);
    assert_eq!(w.close_requested(&rt), CloseAction::Cancel, "可重复调用");
}
// ─────────────────────── view DSL 覆盖 ───────────────────────

/// 遍历主要容器与控件构造器。
/// 价值在**编译期**：任何一个被改名或改签名都会在这里炸。
#[test]
fn view_dsl_covers_containers_and_controls() {
    let rt = Runtime::new();
    let follow = Signal::new(&rt, true);

    struct W {
        follow: Signal<bool>,
    }
    impl ViewModel for W {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            // ⚠️ **DSL 的两条真实约束**（契约测试的价值之一就是把它们钉住）：
            //
            // ① `padding` / `gap` 返回 `()` 而非 `Self` ⇒ 不能写成
            //    `c.padding(8.0).gap(4.0)`。这是已知缺陷 **D45**。
            // ② `view()` 顶层**只能声明一个内容根**；写第二个顶层容器会
            //    `panic!("内容根只能声明一次")`（`view.rs:219`）。
            //    row / scroll 必须**嵌套**在内容根闭包内。
            v.column(|c| {
                c.padding(8.0);
                c.gap(4.0);

                c.text("文本").font_size(14.0);
                c.button("按钮").on_tap(|| {}).width(80.0).height(24.0);
                c.checkbox_bound(&self.follow);
                c.progress(0.5);
                c.spacer();

                // 嵌套容器：row / scroll 只能出现在这里
                c.row(|r| {
                    r.text("A");
                    r.text("B").font_size(12.0);
                    r.spacer();
                });
                c.scroll(|s| {
                    s.text("滚动内容");
                });
            });
        }
    }

    let mut app = App::new(rt);
    let _ = app.window(WindowConfig::new(), W { follow });
    app.frame_all(); // 跑完即说明 DSL 调用链自洽
}

/// 绑定型控件：`*_bound` 是"未绑定就不改模型"那条反直觉规则的入口，
/// 值得在契约层钉住它们存在且可链式。
#[test]
fn bound_controls_are_constructible() {
    let rt = Runtime::new();
    let agree = Signal::new(&rt, false);
    let vol = Signal::new(&rt, 0.0f32);
    let pick = Signal::new(&rt, "a".to_string());
    let txt = Signal::new(&rt, String::new());

    struct B {
        agree: Signal<bool>,
        vol: Signal<f32>,
        pick: Signal<String>,
        txt: Signal<String>,
    }
    impl ViewModel for B {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            v.column(|c| {
                c.checkbox_bound(&self.agree);
                c.switch_bound(&self.agree);
                c.radio_bound(&self.pick, "a");
                c.slider_bound(&self.vol, 0.0, 10.0);
                c.input_bound(&self.txt);
            });
        }
    }

    let mut app = App::new(rt);
    let _ = app.window(WindowConfig::new(), B { agree, vol, pick, txt });
    app.frame_all();
}

// ─────────────────────── 列表 ───────────────────────

/// `keyed_list` / `virtual_list` 的真实签名（`key_fn: fn(&T) -> K` 是**函数指针**）。
#[test]
fn lists_are_constructible() {
    let rt = Runtime::new();
    let items: Vec<u64> = (0..5).collect();

    struct L {
        items: Vec<u64>,
        vstate: lieui::view::VirtualListState,
    }
    impl ViewModel for L {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            // ⚠️ DSL 的三条真实约束（都是 `panic!`，不是编译错误）：
            //① `keyed_list` **必须在容器闭包内**（`view.rs:560`）；
            //  ② 它会**接管**该容器的子节点 —— 接管后不能再往同一容器直接加子节点
            //     （`view.rs:208`），所以两个列表必须各自独占一个容器；
            //  ③ 顶层只能有一个内容根（`view.rs:219`）。
            v.column(|outer| {
                outer.row(|c| {
                    c.keyed_list(
                        self.items.clone(),
                        |i: &u64| *i,
                        |c: &mut ViewBuf, i: &u64| {
                            c.text(format!("item {i}"));
                        },
                    );
                });
                outer.scroll(|c| {
                    c.virtual_list(
                        &self.vstate,
                        &self.items,
                        |i: &u64| *i,
                        20.0,  // item_height
                        100.0, // viewport_height
                        |c: &mut ViewBuf, i: &u64| {
                            c.text(format!("row {i}"));
                        },
                    );
                });
            });
        }
    }

    let mut app = App::new(rt.clone());
    let _ = app.window(
        WindowConfig::new().size(200.0, 200.0),
        L {
            items,
            vstate: lieui::view::VirtualListState::new(&rt),
        },
    );
    app.frame_all();
}
// ─────────────────────── 事件与命令 ───────────────────────

/// 用户 handler 与 `CmdBuf` 的公开用法。
#[test]
fn handlers_and_cmdbuf_are_usable() {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());
    let taps = Rc::new(Cell::new(0u32));

    struct H(Rc<Cell<u32>>);
    impl ViewModel for H {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            let f = self.0.clone();
            v.column(|c| {
                c.button("go").on_tap(move || f.set(f.get() + 1));
            });
        }
        fn on_external(self: &Rc<Self>, cx: &mut Ctx, _data: ExternalData) {
            // `Ctx` 是 handler 拿到的事件上下文：能排队命令、能发消息。
            cx.damage_all();
            let _ = cx.emit("hello");
        }
    }

    let _ = app.window(WindowConfig::new(), H(taps.clone()));
    app.frame_all();
    assert_eq!(taps.get(), 0, "还没有人点");

    // CmdBuf 独立于 Ctx 也能攒命令（纯命令缓冲的公开面）
    let mut cmds = CmdBuf::new();
    assert!(cmds.is_empty());
    cmds.damage_all();
    assert!(!cmds.is_empty(), "CmdBuf 应可累积命令");
}

// ─────────────────────── 主题 / 窗口选项 ───────────────────────

#[test]
fn theme_and_window_options_are_settable() {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());
    struct Empty;
    impl ViewModel for Empty {
        fn view(self: &Rc<Self>, v: &mut ViewBuf) {
            v.column(|c| {
                c.text("t");
            });
        }
    }
    let _ = app.window(WindowConfig::new().size(120.0, 80.0), Empty);

    rt.set_theme_mode(ThemeMode::Dark);
    assert_eq!(rt.theme_mode(), ThemeMode::Dark);
    rt.set_theme(Theme::light());
    assert_eq!(
        rt.theme_mode(),
        ThemeMode::Custom,
        "set_theme 会切到 Custom（不再跟随系统）"
    );

    // `full_repaint` 是 `WindowConfig` 上的 builder 方法
    let _cfg = WindowConfig::new().full_repaint(true);
    app.frame_all();
}

/// `ImageStyle` 是**死 API**（无 builder 写入、绘制不读，`raster.rs` 直接拉伸 blit，
/// 且 `gallery.rs:460` 的注释宣称"contain 缩放"与实现矛盾）。
/// 这条测试的作用：一旦有人把它接上（或彻底删掉），这里会提醒同步更新文档与断言。
#[test]
fn image_style_is_present_but_unwired() {
    let _style = ImageStyle::default();
    // 当前没有任何 ViewBuf 方法能把它写进节点 —— 见 refactor-plan D43。
}
