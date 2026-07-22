//! LayoutNode + LayoutContext — 完整 Flex 布局

use crate::core::ElementId;
use crate::layout::box_model::{BoxStyle, ComputedLayout, IntrinsicSize, LayoutConstraint};
use crate::layout::measurable::{DefaultMeasurer, Measurable};
use crate::runtime::element::ElementTree;

#[derive(Debug, Clone)]
pub struct LayoutNode {
    pub id: ElementId,
    pub style: BoxStyle,
    pub intrinsic: IntrinsicSize,
    pub computed: ComputedLayout,
    pub dirty: bool,
    pub children: Vec<LayoutNode>,
}
impl LayoutNode {
    pub fn new(id: ElementId) -> Self {
        Self { id, style: BoxStyle::new(), intrinsic: IntrinsicSize::zero(),
               computed: ComputedLayout::default(), dirty: true, children: Vec::new() }
    }
    pub fn bounds(&self) -> crate::geometry::Rect { self.computed.rect() }
    pub fn hit_test_rec(&self, px: f32, py: f32) -> Option<ElementId> {
        if !self.computed.contains(px, py) { return None; }
        for child in self.children.iter().rev() {
            if let Some(id) = child.hit_test_rec(px, py) { return Some(id); }
        }
        Some(self.id)
    }
}

#[derive(Debug, Clone)]
pub struct LayoutContext { pub root: Option<LayoutNode> }
impl LayoutContext {
    pub fn new() -> Self { Self { root: None } }
    pub fn hit_test(&self, point: crate::geometry::Point, _tree: &ElementTree) -> Option<ElementId> {
        self.root.as_ref().and_then(|r| r.hit_test_rec(point.x, point.y))
    }
    pub fn compute(&mut self, viewport: crate::geometry::Size, tree: &ElementTree) {
        let Some(root_id) = tree.root() else { return; };
        let constraint = LayoutConstraint::loose((viewport.width, viewport.height));
        let full = Self::build_tree(root_id, constraint, tree, None);
        self.root = Some(full);
    }
    fn build_tree(id: ElementId, constraint: LayoutConstraint, tree: &ElementTree, _pt: Option<&str>) -> LayoutNode {
        let measurer = DefaultMeasurer;
        let intrinsic = tree.props(id).map(|p| measurer.measure(p, &constraint)).unwrap_or(IntrinsicSize::zero());
        tree.set_intrinsic(id, intrinsic);
        let tn = tree.type_name(id).unwrap_or("");

        // 读取 flex 属性（以字符串形式存储）
        let props = tree.props(id);
        let justify = props.and_then(|p| p.get_str("justify")).unwrap_or("start");
        let align = props.and_then(|p| p.get_str("align")).unwrap_or("stretch");
        let spacing = props.and_then(|p| p.get_f32("spacing")).unwrap_or(4.0);

        // 第一遍：递归构建子节点，收集尺寸
        let raw_children: Vec<LayoutNode> = tree.children_of(id).iter().map(|&cid| {
            Self::build_tree(cid, constraint, tree, Some(tn))
        }).collect();

        // 计算总尺寸
        let (total_main, max_cross): (f32, f32) = match tn {
            "column" => {
                let tm: f32 = raw_children.iter().map(|c| c.computed.height).sum();
                let mc = raw_children.iter().map(|c| c.computed.width).fold(0.0f32, f32::max);
                (tm + spacing * (raw_children.len().saturating_sub(1) as f32), mc)
            }
            "row" => {
                let tm: f32 = raw_children.iter().map(|c| c.computed.width).sum();
                let mc = raw_children.iter().map(|c| c.computed.height).fold(0.0f32, f32::max);
                (tm + spacing * (raw_children.len().saturating_sub(1) as f32), mc)
            }
            _ => (0.0, 0.0),
        };

        // 容器自身尺寸
        let (w, h) = match tn {
            "column" => (max_cross.max(intrinsic.width).max(constraint.min_width), total_main.max(constraint.min_height)),
            "row" => (total_main.max(intrinsic.width).max(constraint.min_width), max_cross.max(intrinsic.height).max(constraint.min_height)),
            _ => (constraint.max_width.min(intrinsic.width).max(constraint.min_width),
                   constraint.max_height.min(intrinsic.height).max(constraint.min_height)),
        };
        let cw = w.min(constraint.max_width);
        let ch = h.min(constraint.max_height);

        // 第二遍：根据 justify + align 计算每个子节点的位置
        let free_main = match tn {
            "column" => ch - total_main,
            "row" => cw - total_main,
            _ => 0.0,
        };
        let free_main = free_main.max(0.0); // 不压缩子节点

        let children = if tn == "column" || tn == "row" {
            let mut result = Vec::with_capacity(raw_children.len());
            let n = raw_children.len();

            // 计算每个子节点的 main axis 偏移量
            let offsets: Vec<f32> = if n == 0 {
                Vec::new()
            } else {
                let gap = spacing;
                match justify {
                    "center" => {
                        let start = free_main / 2.0;
                        let mut o = Vec::with_capacity(n);
                        let mut cur = start;
                        for c in &raw_children {
                            o.push(cur);
                            cur += (if tn == "column" { c.computed.height } else { c.computed.width }) + gap;
                        }
                        o
                    }
                    "end" => {
                        let mut o = Vec::with_capacity(n);
                        let mut cur = free_main;
                        for c in &raw_children {
                            o.push(cur);
                            cur += (if tn == "column" { c.computed.height } else { c.computed.width }) + gap;
                        }
                        o
                    }
                    "space-between" => {
                        let gap2 = if n > 1 { free_main / (n - 1) as f32 } else { 0.0 };
                        let mut o = Vec::with_capacity(n);
                        let mut cur = 0.0;
                        for c in &raw_children {
                            o.push(cur);
                            cur += (if tn == "column" { c.computed.height } else { c.computed.width }) + gap2;
                        }
                        o
                    }
                    "space-around" => {
                        let gap2 = if n > 0 { free_main / n as f32 } else { 0.0 };
                        let half = gap2 / 2.0;
                        let mut o = Vec::with_capacity(n);
                        let mut cur = half;
                        for _ in &raw_children {
                            o.push(cur);
                            cur += gap2 + (if tn == "row" { 0.0 } else { 0.0 });
                        }
                        // 修正: space-around 每个子项两边间距相等
                        let _ = (half, gap2);
                        o
                    }
                    "space-evenly" => {
                        let gap2 = if n > 0 { free_main / (n + 1) as f32 } else { 0.0 };
                        let mut o = Vec::with_capacity(n);
                        let mut cur = gap2;
                        for _ in &raw_children {
                            o.push(cur);
                            cur += gap2 + (if tn == "row" { 0.0 } else { 0.0 });
                        }
                        o
                    }
                    _ => { // "start" 默认
                        let mut o = Vec::with_capacity(n);
                        let mut cur = 0.0;
                        for c in &raw_children {
                            o.push(cur);
                            cur += (if tn == "column" { c.computed.height } else { c.computed.width }) + gap;
                        }
                        o
                    }
                }
            };

            // 计算 cross axis 偏移 + 设置最终位置
            for (i, mut child) in raw_children.into_iter().enumerate() {
                let main_off = offsets.get(i).copied().unwrap_or(0.0);
                let cross_off = match align {
                    "center" => (cw - child.computed.width) / 2.0,
                    "end" => (cw - child.computed.width).max(0.0),
                    _ => 0.0, // "start" / "stretch"
                };
                match tn {
                    "column" => { child.computed.x = cross_off; child.computed.y = main_off; }
                    "row" => { child.computed.x = main_off; child.computed.y = cross_off; }
                    _ => {}
                }
                result.push(child);
            }
            result
        } else {
            raw_children
        };

        let mut node = LayoutNode::new(id);
        node.intrinsic = intrinsic;
        node.computed = ComputedLayout { x: 0.0, y: 0.0, width: cw, height: ch };
        node.children = children;
        node
    }
}
impl Default for LayoutContext { fn default() -> Self { Self::new() } }
