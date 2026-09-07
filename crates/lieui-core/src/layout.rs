//! 布局桥接层 —— 把节点树 + 属性表适配成 `lieui-layout` 的 `LayoutTree`

use std::collections::HashMap;

use lieui_layout::{
    CSSDirection, ComputedLayout, Dimension as EngineDim, DisplayType, FlexAlign, FlexDirection,
    FlexStyle, FlexWrap, LayoutTree, PositionType, VALUE_UNDEFINED,
};
use lieui_text::{FnvBuildHasher, FontWeight, TextAlign, TextService, TextSpec};

use crate::id::{ElementTypeId, NodeId};
use crate::props::keys as K;
use crate::props::{Dimension, PropValue, SharedString};
use crate::tree::Tree;

// ── 枚举映射 ──

fn to_flex_direction(v: u32) -> FlexDirection {
    match v {
        K::flex_direction::ROW => FlexDirection::Row,
        K::flex_direction::ROW_REVERSE => FlexDirection::RowReverse,
        K::flex_direction::COLUMN_REVERSE => FlexDirection::ColumnReverse,
        _ => FlexDirection::Column,
    }
}

fn to_flex_wrap(v: u32) -> FlexWrap {
    match v {
        K::flex_wrap::WRAP => FlexWrap::Wrap,
        K::flex_wrap::WRAP_REVERSE => FlexWrap::WrapReverse,
        _ => FlexWrap::NoWrap,
    }
}

fn to_align(v: u32) -> FlexAlign {
    match v {
        K::align::AUTO => FlexAlign::Auto,
        K::align::CENTER => FlexAlign::Center,
        K::align::END => FlexAlign::End,
        K::align::STRETCH => FlexAlign::Stretch,
        K::align::BASELINE => FlexAlign::Baseline,
        K::align::SPACE_BETWEEN => FlexAlign::SpaceBetween,
        K::align::SPACE_AROUND => FlexAlign::SpaceAround,
        K::align::SPACE_EVENLY => FlexAlign::SpaceEvenly,
        _ => FlexAlign::Start,
    }
}

fn to_text_align(v: u32) -> TextAlign {
    match v {
        K::text_align::CENTER => TextAlign::Center,
        K::text_align::END => TextAlign::End,
        K::text_align::JUSTIFY => TextAlign::Justify,
        _ => TextAlign::Start,
    }
}

// ── 便捷取值 ──

struct Props<'a> {
    tree: &'a Tree,
    store: &'a mut crate::props::PropertyStore,
}

impl Props<'_> {
    fn f32(&mut self, node: NodeId, slot: u16, default: f32) -> f32 {
        let key = crate::props::PropKeyId {
            slot,
            tag: crate::props::TypeTag::F32,
        };
        match self.store.resolve(self.tree, node, key) {
            PropValue::F32(v) => v,
            _ => default,
        }
    }
    fn opt_f32(&mut self, node: NodeId, slot: u16) -> Option<f32> {
        let key = crate::props::PropKeyId {
            slot,
            tag: crate::props::TypeTag::F32,
        };
        match self.store.resolve(self.tree, node, key) {
            PropValue::F32(v) => Some(v),
            _ => None,
        }
    }
    fn u32(&mut self, node: NodeId, slot: u16, default: u32) -> u32 {
        let key = crate::props::PropKeyId {
            slot,
            tag: crate::props::TypeTag::U32,
        };
        match self.store.resolve(self.tree, node, key) {
            PropValue::U32(v) => v,
            _ => default,
        }
    }
    fn bool(&mut self, node: NodeId, slot: u16, default: bool) -> bool {
        let key = crate::props::PropKeyId {
            slot,
            tag: crate::props::TypeTag::Bool,
        };
        match self.store.resolve(self.tree, node, key) {
            PropValue::Bool(v) => v,
            _ => default,
        }
    }
    fn dim(&mut self, node: NodeId, slot: u16) -> Dimension {
        let key = crate::props::PropKeyId {
            slot,
            tag: crate::props::TypeTag::Dim,
        };
        match self.store.resolve(self.tree, node, key) {
            PropValue::Dim(v) => v,
            _ => Dimension::Auto,
        }
    }
    fn string(&mut self, node: NodeId, slot: u16) -> SharedString {
        let key = crate::props::PropKeyId {
            slot,
            tag: crate::props::TypeTag::Str,
        };
        match self.store.resolve(self.tree, node, key) {
            PropValue::Str(v) => v,
            _ => SharedString::default(),
        }
    }
}

// ── 布局结果存储 ──

/// 布局结果表（按 `NodeId::to_u64()` 索引）
#[derive(Default)]
pub struct LayoutStore {
    map: HashMap<u64, ComputedLayout, FnvBuildHasher>,
}

impl LayoutStore {
    pub fn set(&mut self, node: u64, layout: ComputedLayout) {
        self.map.insert(node, layout);
    }
    pub fn get(&self, node: NodeId) -> Option<ComputedLayout> {
        self.map.get(&node.to_u64()).copied()
    }
    pub fn get_raw(&self, node: u64) -> Option<ComputedLayout> {
        self.map.get(&node).copied()
    }
    pub fn remove(&mut self, node: NodeId) {
        self.map.remove(&node.to_u64());
    }
    pub fn clear(&mut self) {
        self.map.clear();
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

// ── LayoutTree 实现 ──

/// 布局 pass 的输入视图。只在一次 pass 期间存在。
pub struct LayoutHost<'a> {
    pub tree: &'a Tree,
    pub props: &'a mut crate::props::PropertyStore,
    pub text: &'a mut TextService,
    pub viewport: (f32, f32),
    /// 布局结果表：用于解析百分比尺寸（取上一帧父级给的可用尺寸）
    pub last: &'a LayoutStore,
    /// 本次 pass 是否遇到了「无历史可用尺寸」的百分比 —— 需要再跑一 pass
    pub unresolved_percent: bool,
}

impl<'a> LayoutHost<'a> {
    pub fn new(
        tree: &'a Tree,
        props: &'a mut crate::props::PropertyStore,
        text: &'a mut TextService,
        last: &'a LayoutStore,
        viewport: (f32, f32),
    ) -> Self {
        Self {
            tree,
            props,
            text,
            viewport,
            last,
            unresolved_percent: false,
        }
    }

    /// 读取文本节点的排版输入（拥有字符串，不借 self）。
    /// 只有 `ElementTypeId::TEXT` 节点返回 `Some`。
    fn text_query(&mut self, node: NodeId) -> Option<TextQuery> {
        if self.tree.get(node)?.type_id != ElementTypeId::TEXT {
            return None;
        }
        let mut p = Props {
            tree: self.tree,
            store: self.props,
        };
        Some(TextQuery {
            content: p.string(node, K::TEXT.slot()),
            family: p.string(node, K::FONT_FAMILY.slot()),
            font_size: p.f32(node, K::FONT_SIZE.slot(), 16.0),
            line_height: p.f32(node, K::LINE_HEIGHT.slot(), 0.0),
            weight: p.u32(node, K::FONT_WEIGHT.slot(), 400),
            italic: p.bool(node, K::ITALIC.slot(), false),
            wrap: p.bool(node, K::TEXT_WRAP.slot(), true),
            align: p.u32(node, K::TEXT_ALIGN.slot(), 0),
        })
    }
}

/// 文本排版输入（拥有字符串，便于构造借用的 `TextSpec`）
struct TextQuery {
    content: SharedString,
    family: SharedString,
    font_size: f32,
    line_height: f32,
    weight: u32,
    italic: bool,
    wrap: bool,
    align: u32,
}

/// 解析 `Dimension` 为引擎的 px 值。
///
/// ★ `Percent` 需要父级可用尺寸：取自上一帧记录的 `avail`。
/// 首帧没有历史值时退化为 Auto，并通过 `unresolved` 触发二次 pass 修正。
///
/// 独立成自由函数（而非方法）：`style_of` 里 `PropertyStore` 的可变借用还活着，
/// `&mut self` 方法会与之冲突。
fn dim_to_px(
    tree: &Tree,
    last: &LayoutStore,
    viewport: (f32, f32),
    unresolved: &mut bool,
    node: NodeId,
    d: Dimension,
    axis: usize,
) -> f32 {
    match d {
        Dimension::Px(v) => v,
        Dimension::Percent(p) => {
            let base = match last.get(node) {
                Some(l) => {
                    if axis == EngineDim::Width as usize {
                        l.avail_w
                    } else {
                        l.avail_h
                    }
                }
                None => {
                    if tree.get(node).is_some_and(|n| n.parent.is_none()) {
                        if axis == EngineDim::Width as usize {
                            viewport.0
                        } else {
                            viewport.1
                        }
                    } else {
                        *unresolved = true;
                        return VALUE_UNDEFINED;
                    }
                }
            };
            if base.is_finite() {
                base * p / 100.0
            } else {
                *unresolved = true;
                VALUE_UNDEFINED
            }
        }
        Dimension::Auto => VALUE_UNDEFINED,
    }
}

impl LayoutTree for LayoutHost<'_> {
    fn style_of(&mut self, node_u: u64) -> FlexStyle {
        let node = NodeId::from_u64(node_u);
        let mut p = Props {
            tree: self.tree,
            store: self.props,
        };

        // 尺寸先取（Percent 依赖 last 的历史 avail）
        let w = p.dim(node, K::WIDTH.slot());
        let h = p.dim(node, K::HEIGHT.slot());

        let mut s = FlexStyle {
            flex_direction: to_flex_direction(p.u32(
                node,
                K::FLEX_DIRECTION.slot(),
                K::flex_direction::COLUMN,
            )),
            flex_wrap: to_flex_wrap(p.u32(node, K::FLEX_WRAP.slot(), 0)),
            justify_content: to_align(p.u32(node, K::JUSTIFY_CONTENT.slot(), K::align::START)),
            align_items: to_align(p.u32(node, K::ALIGN_ITEMS.slot(), K::align::STRETCH)),
            align_self: to_align(p.u32(node, K::ALIGN_SELF.slot(), K::align::AUTO)),
            align_content: to_align(p.u32(node, K::ALIGN_CONTENT.slot(), K::align::START)),
            position_type: match p.u32(node, K::POSITION_TYPE.slot(), 0) {
                K::position_type::ABSOLUTE => PositionType::Absolute,
                _ => PositionType::Relative,
            },
            display_type: match p.u32(node, K::DISPLAY.slot(), 0) {
                K::display::NONE => DisplayType::None,
                _ => DisplayType::Flex,
            },
            flex_grow: p.f32(node, K::FLEX_GROW.slot(), 0.0),
            flex_shrink: p.f32(node, K::FLEX_SHRINK.slot(), 1.0),
            item_space: p.f32(node, K::GAP.slot(), 0.0),
            line_space: p.f32(node, K::LINE_GAP.slot(), 0.0),
            min_dim: [
                p.opt_f32(node, K::MIN_WIDTH.slot())
                    .unwrap_or(VALUE_UNDEFINED),
                p.opt_f32(node, K::MIN_HEIGHT.slot())
                    .unwrap_or(VALUE_UNDEFINED),
            ],
            max_dim: [
                p.opt_f32(node, K::MAX_WIDTH.slot())
                    .unwrap_or(VALUE_UNDEFINED),
                p.opt_f32(node, K::MAX_HEIGHT.slot())
                    .unwrap_or(VALUE_UNDEFINED),
            ],
            dim: [
                dim_to_px(
                    self.tree,
                    self.last,
                    self.viewport,
                    &mut self.unresolved_percent,
                    node,
                    w,
                    EngineDim::Width as usize,
                ),
                dim_to_px(
                    self.tree,
                    self.last,
                    self.viewport,
                    &mut self.unresolved_percent,
                    node,
                    h,
                    EngineDim::Height as usize,
                ),
            ],
            ..Default::default()
        };

        s.set_padding(CSSDirection::Left, p.f32(node, K::PADDING_L.slot(), 0.0));
        s.set_padding(CSSDirection::Top, p.f32(node, K::PADDING_T.slot(), 0.0));
        s.set_padding(CSSDirection::Right, p.f32(node, K::PADDING_R.slot(), 0.0));
        s.set_padding(CSSDirection::Bottom, p.f32(node, K::PADDING_B.slot(), 0.0));
        s.set_margin(CSSDirection::Left, p.f32(node, K::MARGIN_L.slot(), 0.0));
        s.set_margin(CSSDirection::Top, p.f32(node, K::MARGIN_T.slot(), 0.0));
        s.set_margin(CSSDirection::Right, p.f32(node, K::MARGIN_R.slot(), 0.0));
        s.set_margin(CSSDirection::Bottom, p.f32(node, K::MARGIN_B.slot(), 0.0));
        s.set_border(CSSDirection::Left, p.f32(node, K::BORDER_L.slot(), 0.0));
        s.set_border(CSSDirection::Top, p.f32(node, K::BORDER_T.slot(), 0.0));
        s.set_border(CSSDirection::Right, p.f32(node, K::BORDER_R.slot(), 0.0));
        s.set_border(CSSDirection::Bottom, p.f32(node, K::BORDER_B.slot(), 0.0));

        // 绝对定位偏移：未设置时保持 VALUE_AUTO（不要写 0）
        if let Some(v) = p.opt_f32(node, K::LEFT.slot()) {
            s.set_position(CSSDirection::Left, v);
        }
        if let Some(v) = p.opt_f32(node, K::TOP.slot()) {
            s.set_position(CSSDirection::Top, v);
        }
        if let Some(v) = p.opt_f32(node, K::RIGHT.slot()) {
            s.set_position(CSSDirection::Right, v);
        }
        if let Some(v) = p.opt_f32(node, K::BOTTOM.slot()) {
            s.set_position(CSSDirection::Bottom, v);
        }

        s.overflow_scroll = p.bool(node, K::OVERFLOW_SCROLL.slot(), false);
        s.content_width = p.opt_f32(node, K::CONTENT_WIDTH.slot());
        s.content_height = p.opt_f32(node, K::CONTENT_HEIGHT.slot());
        s
    }

    fn collect_children(&mut self, node_u: u64, out: &mut Vec<u64>) {
        let node = NodeId::from_u64(node_u);
        for c in self.tree.children(node) {
            out.push(c.to_u64());
        }
    }

    fn measure(&mut self, node_u: u64, max_width: Option<f32>) -> Option<(f32, f32)> {
        let node = NodeId::from_u64(node_u);
        let q = self.text_query(node)?;
        let spec = TextSpec {
            text: q.content.as_str(),
            family: q.family.as_str(),
            font_size: q.font_size,
            line_height: q.line_height,
            weight: FontWeight(q.weight),
            italic: q.italic,
            wrap: q.wrap,
            max_width,
            align: to_text_align(q.align),
        };
        Some(self.text.measure(&spec))
    }

    fn is_text(&mut self, node_u: u64) -> bool {
        let node = NodeId::from_u64(node_u);
        self.tree
            .get(node)
            .is_some_and(|n| n.type_id == ElementTypeId::TEXT)
    }

    fn scroll_offset(&mut self, node_u: u64) -> (f32, f32) {
        let node = NodeId::from_u64(node_u);
        let mut p = Props {
            tree: self.tree,
            store: self.props,
        };
        (
            p.f32(node, K::SCROLL_X.slot(), 0.0),
            p.f32(node, K::SCROLL_Y.slot(), 0.0),
        )
    }
}
