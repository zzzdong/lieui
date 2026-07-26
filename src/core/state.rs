//! ElementState — 运行时元素交互状态

use crate::geometry::Rect;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ElementState {
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
    /// 当前元素作为 IME 焦点时，建议的候选窗/光标区域（屏幕坐标）。
    /// 由运行时根据布局结果或子节点 caret 计算后写入。
    pub ime_cursor_area: Option<Rect>,
}
