//! ViewNode — View 树的扁平节点

use crate::geometry::Color;
use crate::view::callback::CallbackId;

#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    Str(String),
    F32(f32), F64(f64), I32(i32), U32(u32),
    Bool(bool),
    Color(Color),
    Bytes(Vec<u8>),
    Callback(CallbackId),
}

#[derive(Debug, Clone)]
pub struct PropMap { entries: Vec<(&'static str, PropValue)> }

impl PropMap {
    pub fn new() -> Self { Self { entries: Vec::new() } }
    pub fn from_array<const N: usize>(arr: [(&'static str, PropValue); N]) -> Self { Self { entries: arr.to_vec() } }
    pub fn set(&mut self, key: &'static str, value: PropValue) {
        if let Some(existing) = self.entries.iter_mut().find(|(k, _)| *k == key) { existing.1 = value; }
        else { self.entries.push((key, value)); }
    }
    pub fn get(&self, key: &str) -> Option<&PropValue> { self.entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v) }
    pub fn get_str(&self, key: &str) -> Option<&str> { match self.get(key) { Some(PropValue::Str(s)) => Some(s.as_str()), _ => None } }
    pub fn get_f32(&self, key: &str) -> Option<f32> { match self.get(key) { Some(PropValue::F32(v)) => Some(*v), Some(PropValue::F64(v)) => Some(*v as f32), _ => None } }
    pub fn get_f64(&self, key: &str) -> Option<f64> { match self.get(key) { Some(PropValue::F64(v)) => Some(*v), Some(PropValue::F32(v)) => Some(*v as f64), _ => None } }
    pub fn get_color(&self, key: &str) -> Option<Color> { match self.get(key) { Some(PropValue::Color(c)) => Some(*c), _ => None } }
    pub fn get_bool(&self, key: &str) -> Option<bool> { match self.get(key) { Some(PropValue::Bool(v)) => Some(*v), _ => None } }
    pub fn get_u32(&self, key: &str) -> Option<u32> { match self.get(key) { Some(PropValue::U32(v)) => Some(*v), _ => None } }
    pub fn get_bytes(&self, key: &str) -> Option<&[u8]> { match self.get(key) { Some(PropValue::Bytes(v)) => Some(v.as_slice()), _ => None } }
    pub fn get_callback(&self, key: &str) -> Option<CallbackId> { match self.get(key) { Some(PropValue::Callback(id)) => Some(*id), _ => None } }
    pub fn iter(&self) -> impl Iterator<Item = &(&'static str, PropValue)> { self.entries.iter() }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

impl Default for PropMap { fn default() -> Self { Self::new() } }

#[derive(Debug, Clone)]
pub struct ViewNode {
    pub type_name: &'static str,
    pub key: Option<String>,
    pub props: PropMap,
    pub children: Vec<ViewNode>,
}
impl ViewNode {
    pub fn new(type_name: &'static str) -> Self { Self { type_name, key: None, props: PropMap::new(), children: Vec::new() } }
}
