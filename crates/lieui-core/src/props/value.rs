//! 属性值与类型标签

use std::sync::Arc;

/// 共享字符串：内部 `Arc<str>`。
///
/// 设计里提到「小字符串内联」，M1 先用裸 `Arc<str>`；
/// 内联优化留到属性系统性能剖析之后（避免过早优化 + 增加不变量）。
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SharedString(Arc<str>);

impl SharedString {
    pub fn new(s: impl AsRef<str>) -> Self {
        Self(Arc::from(s.as_ref()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SharedString {
    fn default() -> Self {
        Self(Arc::from(""))
    }
}

impl std::fmt::Debug for SharedString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}
impl std::fmt::Display for SharedString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::ops::Deref for SharedString {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}
impl From<&str> for SharedString {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}
impl From<String> for SharedString {
    fn from(s: String) -> Self {
        Self(Arc::from(s))
    }
}
impl AsRef<str> for SharedString {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// RGBA 颜色（8bit / 通道）
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

/// 布局尺寸
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Dimension {
    /// 由内容/父约束决定
    #[default]
    Auto,
    Px(f32),
    Percent(f32),
}

impl Dimension {
    pub const fn px(v: f32) -> Self {
        Self::Px(v)
    }
    pub const fn percent(v: f32) -> Self {
        Self::Percent(v)
    }
    /// 是否已确定（Px / Percent）——决定该节点能否成为重排边界
    pub const fn is_definite(&self) -> bool {
        !matches!(self, Self::Auto)
    }
}

/// 属性值
///
/// ★ 无 `Object` / `Array` 变体——这是刻意的边界收窄：复合数据必须拆成
/// 多个细粒度信号或属性，避免把「半个 ViewModel」塞进属性表。
///
/// 设计里的 `Any(Box<dyn AnyPropValue>)` 变体推迟到有控件真正需要时再引入。
#[derive(Clone, Debug, PartialEq, Default)]
pub enum PropValue {
    F32(f32),
    I32(i32),
    U32(u32),
    Bool(bool),
    Color(Color),
    Str(SharedString),
    Dim(Dimension),
    /// 该属性槽未设置任何值
    #[default]
    None,
}

impl PropValue {
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Self::F32(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Self::U32(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(v) => Some(v.as_str()),
            _ => None,
        }
    }
    pub fn as_dim(&self) -> Option<Dimension> {
        match self {
            Self::Dim(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_color(&self) -> Option<Color> {
        match self {
            Self::Color(v) => Some(*v),
            _ => None,
        }
    }
    /// 未设置 → 用 `f` 兜底
    pub fn f32_or(&self, f: impl FnOnce() -> f32) -> f32 {
        self.as_f32().unwrap_or_else(f)
    }
    pub fn bool_or(&self, f: impl FnOnce() -> bool) -> bool {
        self.as_bool().unwrap_or_else(f)
    }
    pub fn u32_or(&self, f: impl FnOnce() -> u32) -> u32 {
        self.as_u32().unwrap_or_else(f)
    }
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

/// 类型标签
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum TypeTag {
    F32,
    I32,
    U32,
    Bool,
    Color,
    Str,
    Dim,
    None,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_definite() {
        assert!(Dimension::px(10.0).is_definite());
        assert!(Dimension::percent(50.0).is_definite());
        assert!(!Dimension::Auto.is_definite());
    }

    #[test]
    fn shared_string_is_cheap_to_clone() {
        let a = SharedString::new("hello");
        let b = a.clone();
        assert_eq!(a, b);
        assert_eq!(a.as_str(), "hello");
    }
}
