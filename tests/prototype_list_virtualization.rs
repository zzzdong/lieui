//! 方案 B 第二个原型：动态列表 + 滚动虚拟化
//!
//! 现状（`src/widget/virtual_list.rs`）：每帧用 `item: Arc<dyn Fn(usize)->Box<dyn Widget>>`
//! 重新构造行 widget，靠 `key: "row-{i}"` 让 reconciler 复用 ElementId。
//! 问题是：**行的内部状态（Input 编辑文本、行内 hover）无法保留**——因为每帧 new 了新的
//! 行 widget，状态无从安放。
//!
//! 方案 B：**行 widget 实例按索引缓存并复用**。滚动窗口 `[first,last)` 变化时，
//! 已实例化的行保留在缓存里，滚回来直接用同一个实例（状态天然保留）。
//! 纯投影，不需要 reconciler diff。
//!
//! 验证目标：
//!   1. 滚动窗口变化时，行实例复用、内部状态保留。
//!   2. 列表项增删时，实例如何映射。
//!   3. 行工厂从"每帧构造"变为"按需实例化 + 缓存"。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

// ============================================================================
// 模拟类型（对应 ViewNode / FlexStyle）
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
enum ViewNode {
    Text(String),
    Div {
        children: Vec<ViewNode>,
        key: Option<String>,
    },
}

trait Widget {
    fn build(&self, ctx: &mut Ctx) -> ViewNode;
}

struct Ctx;
impl Ctx {
    fn new() -> Self {
        Self
    }
}

// ============================================================================
// 带内部状态的行 widget：模拟 Input（有编辑文本，跨帧要保留）
// ============================================================================

/// 一行，带内部状态（编辑文本）。方案 B：状态在实例字段里。
struct Row {
    index: usize,
    /// 内部可变状态：这一行被编辑过的文本
    edit_text: RefCell<String>,
    /// hover 态
    hovered: Cell<bool>,
}
impl Row {
    fn new(index: usize) -> Self {
        Self {
            index,
            edit_text: RefCell::new(format!("item {index}")),
            hovered: Cell::new(false),
        }
    }
    /// 用户在这一行输入（直接改实例字段）
    fn type_text(&self, s: &str) {
        *self.edit_text.borrow_mut() = s.to_string();
    }
    fn set_hovered(&self, h: bool) {
        self.hovered.set(h);
    }
}
impl Widget for Row {
    fn build(&self, _ctx: &mut Ctx) -> ViewNode {
        // 读自己的字段投影（这就是方案 B 的核心：状态在字段里，投影读它）
        let text = self.edit_text.borrow().clone();
        let mut content = text;
        if self.hovered.get() {
            content.push_str(" *"); // hover 标记
        }
        ViewNode::Div {
            children: vec![ViewNode::Text(content)],
            key: Some(format!("row-{}", self.index)),
        }
    }
}

// ============================================================================
// 行实例缓存：方案 B 的 VirtualList 核心
// ============================================================================

/// 行实例缓存。滚动窗口变化时复用实例，保留行内状态。
/// 替代现状的 `item: Fn(usize)->Box<dyn Widget>` 每帧重建。
struct RowCache {
    /// index → Rc<Row> 实例。滚动出窗口的不删除，滚回来复用。
    instances: HashMap<usize, Rc<Row>>,
    /// 新建行的工厂（首次见到某个 index 时调用一次）
    factory: Box<dyn Fn(usize) -> Rc<Row>>,
}
impl RowCache {
    fn new(factory: impl Fn(usize) -> Rc<Row> + 'static) -> Self {
        Self {
            instances: HashMap::new(),
            factory: Box::new(factory),
        }
    }
    /// 取某行的实例：命中缓存则复用（状态保留），未命中则实例化。
    fn get(&mut self, index: usize) -> Rc<Row> {
        if let Some(r) = self.instances.get(&index) {
            return Rc::clone(r);
        }
        let r = (self.factory)(index);
        self.instances.insert(index, Rc::clone(&r));
        r
    }
    /// 只读取已实例化的行（测试用）：不存在返回 None。
    fn peek(&self, index: usize) -> Option<Rc<Row>> {
        self.instances.get(&index).cloned()
    }
    /// 行数（用于观察缓存规模 / 测试）。
    fn len(&self) -> usize {
        self.instances.len()
    }
}

// ============================================================================
// VirtualList（方案 B 版）：视口窗口 [first,last)，行实例缓存复用
// ============================================================================

struct VirtualList {
    height: f32,
    item_count: usize,
    item_height: f32,
    cache: RefCell<RowCache>,
    /// 当前滚动偏移（模拟引擎 scroll_state）
    scroll_y: f32,
}
impl VirtualList {
    fn new(height: f32, item_count: usize, item_height: f32) -> Self {
        Self {
            height,
            item_count,
            item_height,
            cache: RefCell::new(RowCache::new(|i| Rc::new(Row::new(i)))),
            scroll_y: 0.0,
        }
    }
    /// 计算可见窗口
    fn visible_range(&self) -> (usize, usize) {
        let sy = self.scroll_y;
        let ih = self.item_height;
        let count = self.item_count;
        let first = ((sy / ih).floor() as usize).min(count);
        let last = (((sy + self.height) / ih).ceil() as usize).min(count);
        (first, last)
    }
    /// 投影：只投影可见窗口内的行，行实例从缓存复用。
    fn build(&self, ctx: &mut Ctx) -> ViewNode {
        let (first, last) = self.visible_range();
        let mut rows = Vec::with_capacity(last - first);
        {
            let mut cache = self.cache.borrow_mut();
            for i in first..last {
                let row = cache.get(i); // 复用 or 实例化
                rows.push(row.build(ctx));
            }
        }
        ViewNode::Div {
            children: rows,
            key: Some("list".into()),
        }
    }
    /// 滚动（模拟）：改 scroll_y，下一帧重新投影窗口。
    fn scroll_to(&mut self, y: f32) {
        self.scroll_y = y;
    }
}

// ============================================================================
// 验证
// ============================================================================

#[test]
fn scroll_reuses_row_instances_and_preserves_state() {
    // 列表：视口高 100，每行 30，共 1000 行 → 窗口约 [0,4)
    let mut list = VirtualList::new(100.0, 1000, 30.0);

    // 第 0 帧：窗口 [0,4)
    let e1 = list.build(&mut Ctx::new());
    let rows1 = match &e1 {
        ViewNode::Div { children, .. } => children,
        _ => panic!(),
    };
    assert_eq!(rows1.len(), 4);
    assert_eq!(
        rows1[0],
        ViewNode::Div {
            children: vec![ViewNode::Text("item 0".into())],
            key: Some("row-0".into()),
        }
    );
    // 缓存里应有 4 个实例
    assert_eq!(list.cache.borrow().len(), 4);

    // 用户在"第 1 行"输入（直接改实例字段 —— 方案 B 核心能力）
    let row1_rc = list.cache.borrow().peek(1).unwrap();
    row1_rc.type_text("edited row 1");

    // 滚动到 y=60 → 窗口 [2,6)，row1 滚出窗口，row0 也滚出
    list.scroll_to(60.0);
    let e2 = list.build(&mut Ctx::new());
    let rows2 = match &e2 {
        ViewNode::Div { children, .. } => children,
        _ => panic!(),
    };
    assert_eq!(rows2.len(), 4); // [2,3,4,5]

    // 缓存规模：之前 4 个 + 新增 row2/3/4/5 中未实例化的 = 现在应有 6 个
    // （row0,row1 滚出但保留在缓存，row2..5 实例化）
    assert_eq!(list.cache.borrow().len(), 6);

    // 滚动回 y=0 → 窗口 [0,4)，row1 复用实例，编辑文本保留！
    list.scroll_to(0.0);
    let e3 = list.build(&mut Ctx::new());
    let rows3 = match &e3 {
        ViewNode::Div { children, .. } => children,
        _ => panic!(),
    };
    // row1 应该显示 "edited row 1"（状态保留了）
    assert_eq!(
        rows3[1],
        ViewNode::Div {
            children: vec![ViewNode::Text("edited row 1".into())],
            key: Some("row-1".into()),
        }
    );
    // 缓存规模：滚回时不新增实例，仍是 6
    assert_eq!(list.cache.borrow().len(), 6);
}

#[test]
fn row_hover_state_preserved_through_window_reuse() {
    let mut list = VirtualList::new(50.0, 10, 20.0); // 窗口 [0,3)
    list.build(&mut Ctx::new());

    // 第 2 行 hover
    let row2 = list.cache.borrow().peek(2).unwrap();
    row2.set_hovered(true);

    // 滚动到底再滚回，row2 的 hover 状态应保留
    list.scroll_to(200.0);
    list.build(&mut Ctx::new()); // 窗口 [10,10) → 空（超出 item_count）
    list.scroll_to(0.0);
    let e = list.build(&mut Ctx::new());
    let rows = match &e {
        ViewNode::Div { children, .. } => children,
        _ => panic!(),
    };
    // row2 hover 保留：文本带 " *"
    assert_eq!(
        rows[2],
        ViewNode::Div {
            children: vec![ViewNode::Text("item 2 *".into())],
            key: Some("row-2".into()),
        }
    );
}

#[test]
fn item_count_change_rebuilds_window_but_reuses_instances() {
    let mut list = VirtualList::new(100.0, 1000, 30.0);
    list.build(&mut Ctx::new());
    assert_eq!(list.cache.borrow().len(), 4);

    // 用户在 row0 输入
    list.cache
        .borrow()
        .peek(0)
        .unwrap()
        .type_text("row0 edited");

    // item_count 从 1000 减到 2（列表项增删）
    list.item_count = 2;
    let e = list.build(&mut Ctx::new());
    let rows = match &e {
        ViewNode::Div { children, .. } => children,
        _ => panic!(),
    };
    assert_eq!(rows.len(), 2); // [0,1)
    // row0 实例复用，编辑文本保留
    assert_eq!(
        rows[0],
        ViewNode::Div {
            children: vec![ViewNode::Text("row0 edited".into())],
            key: Some("row-0".into()),
        }
    );
}

#[test]
fn instance_identity_is_stable_across_projections() {
    // 关键：同一个 index 在多次投影中必须返回**同一个 Rc 实例**（Rc 指针相等），
    // 而不是每帧新 clone。这保证状态、回调、ref 都稳定。
    let mut list = VirtualList::new(100.0, 1000, 30.0);
    list.build(&mut Ctx::new());
    list.build(&mut Ctx::new());

    let a = list.cache.borrow().peek(3).unwrap();
    let b = list.cache.borrow().peek(3).unwrap();
    assert!(Rc::ptr_eq(&a, &b), "同一 index 必须是同一实例");

    // 且跨投影稳定：滚动后滚回，仍是同一实例
    list.scroll_to(90.0);
    list.build(&mut Ctx::new());
    list.scroll_to(0.0);
    list.build(&mut Ctx::new());
    let c = list.cache.borrow().peek(3).unwrap();
    assert!(Rc::ptr_eq(&a, &c), "滚动窗口变化后实例身份必须稳定");
}
