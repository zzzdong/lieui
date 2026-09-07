//! 布局输入视图 —— 让 Flex 引擎保持「树无关」
//!
//! `lieui-layout` 不依赖 `lieui-core` 的节点树表示，只通过本 trait 取数据与测量叶子，
//! 由 `lieui-core` 为 `Tree + PropertyStore + TextService` 实现它。
//!
//! 方法一律 `&mut self`：实现方内部持有属性解析缓存与文本测度缓存，需要可变访问。

use crate::style::FlexStyle;

/// 布局引擎需要的一切外部信息。
///
/// 节点句柄统一为 `u64`（`NodeId::to_u64()`），避免布局层引入 core 的 ID 类型。
pub trait LayoutTree {
    /// 节点的 flex 样式。每次 pass 都重新取，可直接从属性表现算。
    fn style_of(&mut self, node: u64) -> FlexStyle;

    /// 把子节点收集进 `out`（清空后按树序追加）。
    ///
    /// 用「填充 Vec」而非回调：布局构建是递归的，回调形式会与 `&mut self` 重入冲突。
    fn collect_children(&mut self, node: u64, out: &mut Vec<u64>);

    /// 叶子内容测量。
    ///
    /// - `max_width = None`：无宽度约束（intrinsic 尺寸，用于 flex-basis 计算）。
    /// - `max_width = Some(w)`：按可用宽度重新测量（文本换行）。
    /// - 返回 `None`：该叶子没有可测量内容，引擎回落到 `FlexNode::intrinsic_size`。
    ///
    /// ★ 是否换行由实现方决定（文本属性的 `wrap`），引擎不感知文本样式。
    fn measure(&mut self, node: u64, max_width: Option<f32>) -> Option<(f32, f32)>;

    /// 是否为文本叶子（标记 `FlexStyle::node_type`，供调试与诊断使用）。
    fn is_text(&mut self, _node: u64) -> bool {
        false
    }

    /// 滚动容器当前的滚动偏移。非滚动容器返回 `(0, 0)`。
    fn scroll_offset(&mut self, _node: u64) -> (f32, f32) {
        (0.0, 0.0)
    }
}
