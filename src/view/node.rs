//! ViewNode — 类型安全的 UI 描述枚举
//!
//! 每个变体有各自的类型字段，不再用字符串键值对。

use crate::geometry::Color;
use crate::layout::flex::{AlignItems, JustifyContent};
use crate::view::callback::CallbackId;

// ------ PropMap 保留用于 Custom 变体 ------

#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    Str(String), F32(f32), F64(f64), I32(i32), U32(u32),
    Bool(bool), Color(Color), Bytes(Vec<u8>), Callback(CallbackId),
}

#[derive(Debug, Clone)]
pub struct PropMap { pub(crate) entries: Vec<(&'static str, PropValue)> }
impl PropMap {
    pub fn new() -> Self { Self { entries: Vec::new() } }
    pub fn from_array<const N: usize>(arr: [(&'static str, PropValue); N]) -> Self { Self { entries: arr.to_vec() } }
    pub fn set(&mut self, k: &'static str, v: PropValue) { if let Some(e) = self.entries.iter_mut().find(|(k2,_)| *k2==k) { e.1=v; } else { self.entries.push((k,v)); } }
    pub fn get(&self, k: &str) -> Option<&PropValue> { self.entries.iter().find(|(k2,_)| *k2==k).map(|(_,v)| v) }
    pub fn get_str(&self, k: &str) -> Option<&str> { match self.get(k) { Some(PropValue::Str(s))=>Some(s), _=>None } }
    pub fn get_f32(&self, k: &str) -> Option<f32> { match self.get(k) { Some(PropValue::F32(v))=>Some(*v),Some(PropValue::F64(v))=>Some(*v as f32), _=>None } }
    pub fn get_f64(&self, k: &str) -> Option<f64> { match self.get(k) { Some(PropValue::F64(v))=>Some(*v),Some(PropValue::F32(v))=>Some(*v as f64), _=>None } }
    pub fn get_bool(&self, k: &str) -> Option<bool> { match self.get(k) { Some(PropValue::Bool(v))=>Some(*v), _=>None } }
    pub fn get_u32(&self, k: &str) -> Option<u32> { match self.get(k) { Some(PropValue::U32(v))=>Some(*v), _=>None } }
    pub fn get_color(&self, k: &str) -> Option<Color> { match self.get(k) { Some(PropValue::Color(c))=>Some(*c), _=>None } }
    pub fn get_bytes(&self, k: &str) -> Option<&[u8]> { match self.get(k) { Some(PropValue::Bytes(v))=>Some(v), _=>None } }
    pub fn iter(&self) -> impl Iterator<Item=&(&'static str, PropValue)> { self.entries.iter() }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}
impl Default for PropMap { fn default() -> Self { Self::new() } }

// ===== Typed ViewNode =====

#[derive(Debug, Clone)]
pub enum ViewNode {
    Text { content: String, font_size: f64, color: Color, key: Option<String> },
    Button { label: String, on_click: Option<u64>, key: Option<String> },
    Image { data: Vec<u8>, w: u32, h: u32, key: Option<String> },
    Checkbox { checked: bool, label: String, on_click: Option<u64>, key: Option<String> },
    Divider { key: Option<String> },
    // Container nodes (children stored inline for build(), extracted into ElementTree at reconcile)
    Column { justify: JustifyContent, align: AlignItems, spacing: f32, expand: bool, key: Option<String>, children: Vec<ViewNode> },
    Row { justify: JustifyContent, align: AlignItems, spacing: f32, expand: bool, key: Option<String>, children: Vec<ViewNode> },
    Container { expand: bool, key: Option<String>, children: Vec<ViewNode> },
    Custom { type_name: &'static str, props: PropMap, key: Option<String>, children: Vec<ViewNode> },
}

impl ViewNode {
    /// 提取变体名称（与旧系统兼容）
    pub fn type_name(&self) -> &'static str {
        match self {
            ViewNode::Text { .. } => "text",
            ViewNode::Button { .. } => "button",
            ViewNode::Image { .. } => "image",
            ViewNode::Checkbox { .. } => "checkbox",
            ViewNode::Divider { .. } => "divider",
            ViewNode::Column { .. } => "column",
            ViewNode::Row { .. } => "row",
            ViewNode::Container { .. } => "container",
            ViewNode::Custom { type_name, .. } => type_name,
        }
    }

    pub fn key(&self) -> Option<&str> {
        match self {
            ViewNode::Text { key, .. } | ViewNode::Button { key, .. } | ViewNode::Image { key, .. }
                | ViewNode::Checkbox { key, .. } | ViewNode::Divider { key, .. }
                | ViewNode::Column { key, .. } | ViewNode::Row { key, .. }
                | ViewNode::Container { key, .. } | ViewNode::Custom { key, .. } => key.as_deref(),
        }
    }

    pub fn set_key(&mut self, k: String) {
        let set = |key: &mut Option<String>| *key = Some(k.clone());
        match self {
            ViewNode::Text { key, .. } | ViewNode::Button { key, .. } | ViewNode::Image { key, .. }
                | ViewNode::Checkbox { key, .. } | ViewNode::Divider { key, .. }
                | ViewNode::Column { key, .. } | ViewNode::Row { key, .. }
                | ViewNode::Container { key, .. } | ViewNode::Custom { key, .. } => set(key),
        }
    }

    /// 获取子节点（容器变体）
    pub fn children(&self) -> &[ViewNode] {
        match self {
            ViewNode::Column { children, .. } | ViewNode::Row { children, .. }
                | ViewNode::Container { children, .. } | ViewNode::Custom { children, .. } => children,
            _ => &[],
        }
    }

    /// 比较"配置"部分是否相等（排除 children，因为孩子由树结构管理）
    pub fn config_eq(&self, other: &Self) -> bool {
        use ViewNode::*;
        match (self, other) {
            (Text { content: a, font_size: b, color: c, .. }, Text { content: x, font_size: y, color: z, .. })
                => a==x && (b-y).abs()<0.001 && c==z,
            (Button { label: a, on_click: b, .. }, Button { label: x, on_click: y, .. }) => a==x && b==y,
            (Image { data: a, w: b, h: c, .. }, Image { data: x, w: y, h: z, .. }) => a==x && b==y && c==z,
            (Checkbox { checked: a, label: b, on_click: c, .. }, Checkbox { checked: x, label: y, on_click: z, .. }) => a==x && b==y && c==z,
            (Divider { .. }, Divider { .. }) => true,
            (Column { justify: a, align: b, spacing: c, expand: d, .. }, Column { justify: x, align: y, spacing: z, expand: w, .. })
                => a==x && b==y && (c-z).abs()<0.001 && d==w,
            (Row { justify: a, align: b, spacing: c, expand: d, .. }, Row { justify: x, align: y, spacing: z, expand: w, .. })
                => a==x && b==y && (c-z).abs()<0.001 && d==w,
            (Container { expand: a, .. }, Container { expand: x, .. }) => a==x,
            (Custom { type_name: a, props: b, .. }, Custom { type_name: x, props: y, .. }) => a==x && b.entries==y.entries,
            _ => false,
        }
    }

    /// 容器类型：是否有子节点
    pub fn is_container(&self) -> bool {
        matches!(self, ViewNode::Column{..}|ViewNode::Row{..}|ViewNode::Container{..}|ViewNode::Custom{..})
    }
}
