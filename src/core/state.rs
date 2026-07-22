//! ElementState — 运行时元素交互状态

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ElementState {
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
}
