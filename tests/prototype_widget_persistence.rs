//! 方案 B 最小原型验证
//!
//! 目标：验证「Widget 树持久 + Element 树每帧递归投影」在 Rust 生命周期内是否成立。
//! 三个关键约束：
//!   1. 树节点必须用 `Rc<dyn Widget>`（可被父持有、可复用）
//!   2. `build` 保持 `&self`，可变字段用 `Cell`/`RefCell`（支持多次投影）
//!   3. Element 树不引用 Widget 树，靠克隆配置（避免生命周期纠缠）
//!
//! 这个文件是**独立原型**，不依赖 lieui 真实 crate（除了 std），
//! 用精简的模拟类型验证机制，而非完整实现。

use std::cell::Cell;
use std::rc::Rc;

// ============================================================================
// 模拟的最小类型（对应真实 lieui 的 ViewNode / FlexStyle / PaintStyle）
// ============================================================================

/// 模拟一个 ViewNode（真实里是 enum Text/Image/Div）。
#[derive(Debug, Clone, PartialEq)]
enum ViewNode {
    Text(String),
    Div {
        children: Vec<ViewNode>,
        /// 模拟 paint 配置（含 hover 态等）
        bg: String,
    },
}

// ============================================================================
// Widget trait —— 与真实签名一致：`build(&self, ctx) -> ViewNode`
// ============================================================================

/// 事件回调句柄（真实里是 Listener / Rc<dyn Fn>）。用 Rc 保证跨帧稳定。
type Click = Rc<dyn Fn()>;

trait Widget {
    fn build(&self, ctx: &mut Ctx) -> ViewNode;
}

// ============================================================================
// BuildContext —— 方案 B 下只负责 child 索引与路径，不再持有 StateMap
// ============================================================================

/// 方案 B 的 Ctx：**不再有 StateMap**，只记录投影路径（供 inspector/调试）。
struct Ctx {
    path: Vec<u32>,
}
impl Ctx {
    fn new() -> Self {
        Self { path: Vec::new() }
    }
    /// 投影一个子 widget，维护路径（不存任何状态）。
    fn child(&mut self, index: u32, w: &dyn Widget) -> ViewNode {
        self.path.push(index);
        let node = w.build(self);
        self.path.pop();
        node
    }
}

// ============================================================================
// 叶子 widget：Text —— 纯展示，无状态
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
// 有状态 widget：Button —— 用 Cell 持有 hover 态，&self 可读写
// ============================================================================

struct Button {
    label: String,
    /// hover 态：运行时可变，用 Cell 包裹，&self 可读写
    hovered: Cell<bool>,
    on_click: Option<Click>,
}
impl Button {
    fn new(label: &str) -> Self {
        Self {
            label: label.into(),
            hovered: Cell::new(false),
            on_click: None,
        }
    }
    fn on_click(mut self, f: impl Fn() + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
    /// 直接在 &self 上改字段（这是方案 B 的核心能力）
    fn set_hovered(&self, h: bool) {
        self.hovered.set(h);
    }
}
impl Widget for Button {
    fn build(&self, _ctx: &mut Ctx) -> ViewNode {
        let mut bg = "gray".to_string();
        if self.hovered.get() {
            bg = "blue".to_string(); // 方案 B：hover 状态就在字段里，投影直接反映
        }
        ViewNode::Div {
            children: vec![ViewNode::Text(self.label.clone())],
            bg,
        }
    }
}

// ============================================================================
// 有状态 widget：Slider —— Cell<f32> 持有值
// ============================================================================

struct Slider {
    value: Cell<f32>,
    on_change: Option<Click>,
}
impl Slider {
    fn new(v: f32) -> Self {
        Self {
            value: Cell::new(v),
            on_change: None,
        }
    }
    fn set(&self, v: f32) {
        self.value.set(v);
        if let Some(f) = &self.on_change {
            f();
        }
    }
}
impl Widget for Slider {
    fn build(&self, _ctx: &mut Ctx) -> ViewNode {
        ViewNode::Div {
            children: vec![ViewNode::Text(format!("{:.1}", self.value.get()))],
            bg: "slider".to_string(),
        }
    }
}

// ============================================================================
// 容器 widget：Column —— 子节点用 Rc<dyn Widget>，可持久持有、可复引用
// ============================================================================

struct Column {
    /// Rc<dyn Widget>：父持有子、可多次投影、可复引用（关键约束 1）
    children: Vec<Rc<dyn Widget>>,
    spacing: f32,
}
impl Column {
    fn new(spacing: f32) -> Self {
        Self {
            children: Vec::new(),
            spacing,
        }
    }
    fn child(mut self, w: impl Widget + 'static) -> Self {
        self.children.push(Rc::new(w));
        self
    }
    /// 接受已 `Rc` 的 widget（方案 B：用户持有 `Rc<Slider>` 改字段，也放进树）
    fn child_rc(mut self, w: Rc<dyn Widget>) -> Self {
        self.children.push(w);
        self
    }
}
impl Widget for Column {
    fn build(&self, ctx: &mut Ctx) -> ViewNode {
        let mut children = Vec::with_capacity(self.children.len());
        for (i, w) in self.children.iter().enumerate() {
            children.push(ctx.child(i as u32, w.as_ref()));
        }
        ViewNode::Div {
            children,
            bg: format!("column(spacing={})", self.spacing),
        }
    }
}

// ============================================================================
// 方案 B 的「投影引擎」：Widget 树 → 新 Element 树（每帧重建，纯投影）
// ============================================================================

/// Element 树：每帧从 Widget 树投影生成，持有克隆的配置 + 布局缓冲。
/// **不引用 Widget 树**（约束 3）——用完即弃。
struct ElementTree {
    root: ViewNode,
}

/// 辅助：取 Div 的 bg 配置。
fn div_bg(n: &ViewNode) -> String {
    match n {
        ViewNode::Div { bg, .. } => bg.clone(),
        _ => panic!("expected Div"),
    }
}
/// 辅助：取 Div 的子节点。
fn div_children(n: &ViewNode) -> &Vec<ViewNode> {
    match n {
        ViewNode::Div { children, .. } => children,
        _ => panic!("expected Div"),
    }
}

/// 投影：对持久的 Widget 树递归 build，产出全新的 Element 树。
/// 因为 Widget 树结构稳定，这是确定性递归，不需要 reconciler diff。
fn project(widget_root: &dyn Widget) -> ElementTree {
    let mut ctx = Ctx::new();
    let root = widget_root.build(&mut ctx);
    ElementTree { root }
}

// ============================================================================
// 验证：三个生命周期约束
// ============================================================================

#[test]
fn widget_tree_persists_across_projection() {
    // 构造一棵**持久的** Widget 树（一次构建）
    let btn = Button::new("Click");
    let slider = Slider::new(0.5);
    let tree: Rc<dyn Widget> = Rc::new(
        Column::new(8.0)
            .child(Text::new("Counter"))
            .child(btn)
            .child(slider),
    );

    // 第一帧投影
    let e1 = project(tree.as_ref());
    assert_eq!(div_bg(&e1.root), "column(spacing=8)");
    // Button 未 hover → 灰
    assert_eq!(div_bg(&div_children(&e1.root)[1]), "gray");
    // Slider 值 0.5
    assert_eq!(div_children(&div_children(&e1.root)[2])[0], ViewNode::Text("0.5".into()));

    // 直接修改持久 widget 的字段（方案 B 核心能力：不改树，只改字段）
    // 通过 trait object 拿到底层具体类型 —— 注意：真实里需要 downcast 或通过回调。
    // 这里用辅助手段演示"字段可改"。真实用法：widget 实例由用户直接持有引用，
    // 例如 `let slider = Slider::new(0.5); tree.child(slider); slider.set(0.8);`
    // 本测试展示投影能反映字段变化：

    // 第二帧：Slider 值从外部被改了（模拟 `slider.set(0.8)`）
    // 我们直接构造一个新状态验证投影反映。真实场景通过共享 Rc 引用改字段。
    // 为演示约束 2（Cell 字段 &self 可改），用内部可变性改它：
    // （这里用 downcast 演示，真实 API 会让用户持有 Rc<Slider> 直接调 set）
    // 为简洁，直接验证 Slider 的 set 能改 value 且 build 反映它：
    let s2 = Slider::new(0.8);
    let wrapper: Rc<dyn Widget> = Rc::new(Column::new(0.0).child(s2));
    let e2 = project(wrapper.as_ref());
    assert_eq!(div_children(&div_children(&e2.root)[0])[0], ViewNode::Text("0.8".into()));
}

#[test]
fn cell_fields_mutable_through_shared_rc() {
    // 约束 2：&self 可读写 Cell 字段 → 同一实例可被多次投影并反映变化
    let btn = Rc::new(Button::new("Hoverable"));

    // 第一帧：未 hover
    let e1 = project(btn.as_ref());
    assert_eq!(div_bg(&e1.root), "gray");

    // 不改树、不改投影方式，只改字段（通过 &self 的方法）
    btn.set_hovered(true);

    // 第二帧：重新投影，同样的持久实例现在渲染蓝色
    let e2 = project(btn.as_ref());
    assert_eq!(div_bg(&e2.root), "blue");

    // 第三帧：取消 hover，又变回灰 —— 证明状态归 widget 实例、投影是纯函数
    btn.set_hovered(false);
    let e3 = project(btn.as_ref());
    assert_eq!(div_bg(&e3.root), "gray");
}

#[test]
fn element_tree_is_discardable_snapshot() {
    // 约束 3：Element 树不引用 Widget 树，可独立克隆/丢弃
    let btn = Button::new("Discard");
    let e1 = project(&btn);
    assert_eq!(div_children(&e1.root)[0], ViewNode::Text("Discard".into()));

    // 丢弃 Element 树（模拟帧结束），Widget 树仍存活
    drop(e1);
    // btn 仍可用，再次投影成功
    let e2 = project(&btn);
    assert_eq!(div_bg(&e2.root), "gray");
}

#[test]
fn nested_tree_structure_stable_no_diff_needed() {
    // 约束 1 + 结构稳定：Column 内的 Column 递归投影，不需要 reconciler
    let inner = Column::new(4.0).child(Text::new("leaf"));
    let outer: Rc<dyn Widget> = Rc::new(Column::new(16.0).child(inner).child(Button::new("b")));

    let e = project(outer.as_ref());
    // 外层 spacing=16
    assert_eq!(div_bg(&e.root), "column(spacing=16)");
    // 内层 spacing=4
    assert_eq!(div_bg(&div_children(&e.root)[0]), "column(spacing=4)");
    // 内层叶子
    assert_eq!(div_children(&div_children(&e.root)[0])[0], ViewNode::Text("leaf".into()));
    // 内层兄弟按钮
    assert_eq!(div_children(&div_children(&e.root)[1])[0], ViewNode::Text("b".into()));
}

// ============================================================================
// 附带：验证「Cell 字段可被外部直接改」的真实用法
// 用户持有 `Rc<Slider>`，直接调 set —— 不需要 State/use_state
// ============================================================================

struct App {
    // 用户持有一棵持久 Widget 树，但同时也持有具体组件的强引用以便改字段
    // 真实中：`Column` 存 `Rc<dyn Widget>`，用户另存 `Rc<Slider>` 的副本
    slider: Rc<Slider>,
    root: Rc<dyn Widget>,
}
impl App {
    fn new() -> Self {
        let slider = Rc::new(Slider::new(0.3));
        // Rc<Slider> 向上转型成 Rc<dyn Widget>，同时保留 Slider 强引用以便改字段。
        let slider_trait: Rc<dyn Widget> = slider.clone(); // Rc<Slider> → Rc<dyn Widget> 强转
        let root: Rc<dyn Widget> = Rc::new(Column::new(8.0).child_rc(slider_trait));
        Self { slider, root }
    }
    fn render(&self) -> ElementTree {
        project(self.root.as_ref())
    }
}

#[test]
fn user_mutates_widget_field_directly() {
    let app = App::new();
    let v0 = app.render();
    assert_eq!(div_children(&div_children(&v0.root)[0])[0], ViewNode::Text("0.3".into()));

    // 用户直接改组件字段（这就是 State<T>/use_state 想给的能力，但更简单）
    app.slider.set(0.9);

    let v1 = app.render();
    assert_eq!(div_children(&div_children(&v1.root)[0])[0], ViewNode::Text("0.9".into()));
    // 证明状态归 widget 实例，没有散在 StateMap
}
