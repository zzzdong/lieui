//! 方案 B 第三个原型：事件回调 + 字段修改 + ViewNode 保持纯 DOM
//!
//! 这是方案 B 用户 API 可用性的最终确认。核心链路：
//!
//!   用户回调改 widget 字段（Cell/RefCell）
//!     → 请求重建
//!       → build(&self) 重新投影
//!         → 产出新的 ViewNode（纯数据 DOM）
//!           → 下游布局/渲染消费
//!
//! 关键约束（用户要求）：**ViewNode 始终保持"类 DOM"的纯数据表示**，
//! 不持有 widget 引用、不持有回调、不持有任何可变状态。它只是某一时刻
//! widget 状态的视图快照。
//!
//! 验证目标：
//!   1. 回调如何在 &self 下改 widget 字段（Rc<dyn Widget> 捕获方案）。
//!   2. 改字段 → 重新投影 → ViewNode 反映新值。
//!   3. ViewNode 是纯数据：可 Clone、可 drop，不引用 widget。
//!   4. 状态归 widget 实例，回调通过捕获的 Rc 稳定引用同一实例。

#![allow(dead_code)] // 原型：保留未在此文件使用的 API（Ctx::new / child / count 等）

use std::cell::{Cell, RefCell};
use std::rc::Rc;

// ============================================================================
// ViewNode —— 纯数据 DOM（用户要求保留此类 DOM 定位）
// ============================================================================

/// 纯数据中间表示。**不持有任何 widget 引用/回调/状态**。
/// 可以在任意时刻被 Clone / 传输 / drop，是自包含的视图快照。
#[derive(Debug, Clone, PartialEq)]
enum ViewNode {
    Text(String),
    Div {
        children: Vec<ViewNode>,
        /// 模拟样式（hover 态等已解析成最终值）
        bg: String,
        /// 纯数据的回调标签（如 "on_click"）。只作为"这个节点挂了哪个回调"的描述，
        /// 真正的回调由 widget 持有，ViewNode 只记录意图，不持闭包。
        on_click: bool,
    },
}

// ============================================================================
// BuildContext —— 投影路径（纯数据辅助，无状态）
// ============================================================================

struct Ctx;
impl Ctx {
    fn new() -> Self {
        Self
    }
}

// ============================================================================
// Widget trait —— build(&self) 声明式投影
// ============================================================================

trait Widget {
    fn build(&self, ctx: &mut Ctx) -> ViewNode;
}

// ============================================================================
// 纯展示 widget：Text —— 无状态，只投影
// ============================================================================

struct Text {
    content: String,
}
impl Text {
    fn new(s: impl Into<String>) -> Self {
        Self { content: s.into() }
    }
}
impl Widget for Text {
    fn build(&self, _ctx: &mut Ctx) -> ViewNode {
        ViewNode::Text(self.content.clone())
    }
}

// ============================================================================
// 有状态组件：Counter —— 回调通过捕获的 Rc<Self> 改字段
// ============================================================================

/// Counter：内部持有 count 字段 + 一个"点击"回调。
///
/// 方案 B 的关键设计：**回调在构造时捕获 `Rc<Counter>`，点击时直接改字段**。
/// `build(&self)` 是 &self，但回调持有 `Rc<Self>`，可以在外部 `&self` 上调用
/// `self.count.set(...)`（Cell 允许 &self 改 Copy 字段）。
struct Counter {
    count: Cell<i32>,
    /// 点击回调：RefCell 允许 &self 注入（方案 B：widget 被 Rc 包裹，不能 mut self 消费）。
    on_click: RefCell<Option<Rc<dyn Fn()>>>,
}
impl Counter {
    fn new() -> Self {
        Self {
            count: Cell::new(0),
            on_click: RefCell::new(None),
        }
    }
    /// 注入点击回调（&self 即可，因回调字段是 RefCell）。
    /// 方案 B 的关键：**回调捕获 Rc<Self>，点击时通过 &self 改 Cell 字段**。
    fn set_on_click(&self, f: impl Fn() + 'static) {
        *self.on_click.borrow_mut() = Some(Rc::new(f));
    }
    /// 用户点击 → 触发回调 → 回调内部改 count 字段。
    fn click(&self) {
        if let Some(f) = self.on_click.borrow().as_ref() {
            f();
        }
    }
    fn count(&self) -> i32 {
        self.count.get()
    }
}
impl Widget for Counter {
    fn build(&self, _ctx: &mut Ctx) -> ViewNode {
        ViewNode::Div {
            children: vec![ViewNode::Text(format!("count={}", self.count.get()))],
            bg: "counter".to_string(),
            on_click: self.on_click.borrow().is_some(),
        }
    }
}

// ============================================================================
// 有状态组件：Slider —— 值 + on_change 回调（外部观察者模式）
// ============================================================================

/// Slider：value 字段 + on_change 回调（改值时通知外部）。
/// 用"事件回调持有 Rc<Slider>" + "on_change 持有外部监听"两种机制演示。
struct Slider {
    value: Cell<f32>,
    /// 值变化时回调外部（通知用户 value 变了）。RefCell 允许 &self 注入。
    on_change: RefCell<Option<Rc<dyn Fn(f32)>>>,
}
impl Slider {
    fn new(v: f32) -> Self {
        Self {
            value: Cell::new(v),
            on_change: RefCell::new(None),
        }
    }
    fn set_on_change(&self, f: impl Fn(f32) + 'static) {
        *self.on_change.borrow_mut() = Some(Rc::new(f));
    }
    /// 用户拖动 → 设置新值 → 触发 on_change 通知外部。
    fn set_value(&self, v: f32) {
        self.value.set(v);
        if let Some(f) = self.on_change.borrow().as_ref() {
            f(v);
        }
    }
    fn value(&self) -> f32 {
        self.value.get()
    }
}
impl Widget for Slider {
    fn build(&self, _ctx: &mut Ctx) -> ViewNode {
        ViewNode::Div {
            children: vec![ViewNode::Text(format!("{:.1}", self.value.get()))],
            bg: "slider".to_string(),
            on_click: false,
        }
    }
}

// ============================================================================
// 容器：Column —— Rc<dyn Widget> 子节点
// ============================================================================

struct Column {
    children: Vec<Rc<dyn Widget>>,
}
impl Column {
    fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }
    fn child(mut self, w: impl Widget + 'static) -> Self {
        self.children.push(Rc::new(w));
        self
    }
    fn child_rc(mut self, w: Rc<dyn Widget>) -> Self {
        self.children.push(w);
        self
    }
}
impl Widget for Column {
    fn build(&self, ctx: &mut Ctx) -> ViewNode {
        let mut children = Vec::new();
        for w in &self.children {
            children.push(w.build(ctx));
        }
        ViewNode::Div {
            children,
            bg: "column".to_string(),
            on_click: false,
        }
    }
}

// ============================================================================
// 投影引擎 + 事件分发（模拟框架）
// ============================================================================

struct ElementTree {
    root: ViewNode,
}

/// 投影：widget 树 → 新的纯数据 ViewNode 树。
fn project(root: &dyn Widget) -> ElementTree {
    let mut ctx = Ctx::new();
    ElementTree {
        root: root.build(&mut ctx),
    }
}

// 辅助访问
fn div_children(n: &ViewNode) -> &Vec<ViewNode> {
    match n {
        ViewNode::Div { children, .. } => children,
        _ => panic!("expected Div"),
    }
}

// ============================================================================
// 验证
// ============================================================================

#[test]
fn click_callback_mutates_widget_field_and_reprojection_reflects() {
    // 构造：Counter，点击回调捕获 Rc<Counter> 自增 count
    let counter = Rc::new(Counter::new());
    let c = Rc::clone(&counter);
    counter.set_on_click(move || {
        c.count.set(c.count.get() + 1); // 直接改字段（Cell &self 可改）
    });

    // 第一帧：count=0
    let e1 = project(&*counter);
    assert_eq!(div_children(&e1.root)[0], ViewNode::Text("count=0".into()));

    // 模拟用户点击（框架命中 ViewNode.on_click 为 true 的节点 → 调 widget 的回调）
    // 关键：回调是通过 Rc<Counter> 捕获的，点击改的是**同一实例**的字段
    counter.click();

    // 第二帧：重新投影，count=1
    let e2 = project(&*counter);
    assert_eq!(div_children(&e2.root)[0], ViewNode::Text("count=1".into()));

    // 多点击几次
    counter.click();
    counter.click();
    let e3 = project(&*counter);
    assert_eq!(div_children(&e3.root)[0], ViewNode::Text("count=3".into()));
}

#[test]
fn viewnode_is_pure_data_not_referencing_widget() {
    // 约束：ViewNode 必须是无 widget 引用的纯数据。
    // 验证方法：从 widget 投影出 ViewNode，drop 掉 widget，ViewNode 仍独立可用。
    let counter = Rc::new(Counter::new());
    counter.set_on_click(|| {});
    let counter: Rc<dyn Widget> = counter;
    let e = project(counter.as_ref());

    // 拿到 ViewNode 的克隆（纯数据）
    let snapshot = e.root.clone();
    let snapshot_children = div_children(&snapshot).clone();

    // drop widget 树和 ElementTree
    drop(counter);
    drop(e);

    // snapshot 仍可用 —— 证明 ViewNode 不引用 widget，是自包含的纯 DOM
    assert_eq!(snapshot_children[0], ViewNode::Text("count=0".into()));
}

#[test]
fn on_change_callback_notifies_external_observer() {
    // Slider 的 on_change 回调（外部观察者模式）：
    // 改值 → 触发 on_change → 外部收到新值
    let observed = Rc::new(Cell::new(0.0f32));
    let obs = Rc::clone(&observed);
    let slider = Rc::new(Slider::new(0.3));
    slider.set_on_change(move |v| obs.set(v));

    // 第一帧：value=0.3
    let e1 = project(&*slider);
    assert_eq!(div_children(&e1.root)[0], ViewNode::Text("0.3".into()));

    // 用户拖动到 0.8
    slider.set_value(0.8);

    // 外部观察者收到 0.8
    assert_eq!(observed.get(), 0.8);
    // 重新投影反映 0.8
    let e2 = project(&*slider);
    assert_eq!(div_children(&e2.root)[0], ViewNode::Text("0.8".into()));
}

#[test]
fn shared_rc_gives_stable_instance_identity_for_callbacks() {
    // 关键：同一 widget 的多个回调（on_click、on_change）通过 Rc::clone 共享同一实例，
    // 保证"改字段 → 投影"用的永远是同一份状态，不会错乱。
    let slider = Rc::new(Slider::new(0.0));
    let s1 = Rc::clone(&slider);
    let s2 = Rc::clone(&slider);
    assert!(Rc::ptr_eq(&s1, &s2));

    // 通过一个句柄改值，另一个句柄读同一份
    s1.set_value(0.5);
    assert_eq!(s2.value(), 0.5);

    // 放进树 + 保留 Rc 改字段（原型 2 验证过的模式，这里和回调结合）
    let tree_slider: Rc<dyn Widget> = slider.clone();
    let root: Rc<dyn Widget> = Rc::new(Column::new().child_rc(tree_slider));
    let e1 = project(root.as_ref());
    assert_eq!(
        div_children(&div_children(&e1.root)[0])[0],
        ViewNode::Text("0.5".into())
    );

    // 用户通过保留的 Rc<Slider> 改值 → 重新投影
    slider.set_value(0.9);
    let e2 = project(root.as_ref());
    assert_eq!(
        div_children(&div_children(&e2.root)[0])[0],
        ViewNode::Text("0.9".into())
    );
}

#[test]
fn full_interaction_loop_counter_and_slider() {
    // 完整交互链路：Counter 自增 + Slider 拖拽，两组件共享一棵树，各自独立状态
    let counter = Rc::new(Counter::new());
    let c = Rc::clone(&counter);
    counter.set_on_click(move || c.count.set(c.count.get() + 1));
    let counter_widget: Rc<dyn Widget> = counter.clone();

    let slider = Rc::new(Slider::new(0.0));
    let last_val = Rc::new(Cell::new(0.0f32));
    let lv = Rc::clone(&last_val);
    slider.set_on_change(move |v| lv.set(v));
    let slider_widget: Rc<dyn Widget> = slider.clone();

    let root: Rc<dyn Widget> = Rc::new(
        Column::new()
            .child_rc(counter_widget)
            .child_rc(slider_widget),
    );

    // 初始
    let e = project(root.as_ref());
    let kids = div_children(&e.root);
    assert_eq!(div_children(&kids[0])[0], ViewNode::Text("count=0".into()));
    assert_eq!(div_children(&kids[1])[0], ViewNode::Text("0.0".into()));

    // 用户点 counter 2 次 + 拖 slider 到 0.6
    counter.click();
    counter.click();
    slider.set_value(0.6);

    // 重新投影，两个组件都反映各自状态，互不干扰
    let e = project(root.as_ref());
    let kids = div_children(&e.root);
    assert_eq!(div_children(&kids[0])[0], ViewNode::Text("count=2".into()));
    assert_eq!(div_children(&kids[1])[0], ViewNode::Text("0.6".into()));
    // slider 的 on_change 也通知了外部
    assert_eq!(last_val.get(), 0.6);
}
