//! Image — 映射到 `ViewNode::Image` 的基础组件

use crate::event::EventContext;
use crate::layout::style::FlexStyle;
use crate::layout::types::FlexAlign;
use crate::view::node::{Listener, ViewNode};
use crate::view::paint::{ImageFit, ImageStyle};
use crate::widget::layout::LayoutAttr;
use crate::widget::{BuildContext, Widget};
use std::rc::Rc;

#[derive(Clone)]
pub struct Image {
    data: std::sync::Arc<Vec<u8>>,
    style: ImageStyle,
    layout: LayoutAttr,
    /// 是否撑满可用空间（等价于 expand(true)）。因涉及 flex_basis/min 等特殊处理，独立存放。
    expand: bool,
    listeners: Vec<Listener>,
}
impl Image {
    pub fn from_rgba(data: Vec<u8>, w: u32, h: u32) -> Self {
        Self {
            data: std::sync::Arc::new(data),
            style: ImageStyle {
                width: w,
                height: h,
                ..ImageStyle::default()
            },
            layout: LayoutAttr::new(),
            expand: false,
            listeners: Vec::new(),
        }
    }
    pub fn from_arc(data: std::sync::Arc<Vec<u8>>, w: u32, h: u32) -> Self {
        Self {
            data,
            style: ImageStyle {
                width: w,
                height: h,
                ..ImageStyle::default()
            },
            layout: LayoutAttr::new(),
            expand: false,
            listeners: Vec::new(),
        }
    }

    /// 显示尺寸（布局宽高）。不设置时图片占位为 0，通常应指定以便 `fit` 生效。
    pub fn width(mut self, w: f32) -> Self {
        self.layout = self.layout.width(w);
        self
    }
    pub fn height(mut self, h: f32) -> Self {
        self.layout = self.layout.height(h);
        self
    }

    /// 用完整布局属性（builder 式）设置本图片的布局。
    pub fn layout(mut self, l: LayoutAttr) -> Self {
        self.layout = l;
        self
    }

    /// 从常见图片文件（png/jpeg/gif/webp/bmp/…）解码为 RGBA8。
    /// 失败（格式不支持/文件不存在）时返回 `None`。
    pub fn from_file<P: AsRef<std::path::Path>>(path: P) -> Option<Self> {
        let img = image::open(path).ok()?;
        Self::from_dynamic(img)
    }

    /// 从内存中的图片字节解码（自动按 magic 探测格式）。
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let img = image::load_from_memory(bytes).ok()?;
        Self::from_dynamic(img)
    }

    fn from_dynamic(img: image::DynamicImage) -> Option<Self> {
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        if w == 0 || h == 0 {
            return None;
        }
        Some(Self::from_rgba(rgba.into_raw(), w, h))
    }
    pub fn opacity(mut self, o: f32) -> Self {
        self.style.opacity = o.clamp(0.0, 1.0);
        self
    }
    pub fn fit(mut self, f: ImageFit) -> Self {
        self.style.fit = f;
        self
    }
    /// 让图片在 flex 容器中撑满可用空间（等价于 expand(true)），
    /// 配合 `fit(Contain)` 即可实现等比自适应缩放。
    ///
    /// 同时把 flex-basis 置为 0、min-width/min-height 置为 0：图片的尺寸完全由
    /// flex-grow 按可用空间分配，而不受其固有像素尺寸影响，避免图片宽高变化
    /// （如旋转 PDF 页面）时撑大父容器导致布局抖动。
    pub fn expand(mut self, v: bool) -> Self {
        self.expand = v;
        self
    }
    pub fn radius(mut self, r: f32) -> Self {
        self.style.border_radius = r;
        self
    }
    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_click(Rc::new(f)));
        self
    }
    pub fn on_click_with_ctx<F: Fn(&mut EventContext) + 'static>(mut self, f: F) -> Self {
        self.listeners.push(Listener::on_click_with_ctx(Rc::new(f)));
        self
    }
}
impl Widget for Image {
    fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
        let mut layout = self.layout.apply(FlexStyle::default());
        if self.expand {
            // flex-grow 撑满可用空间；flex_basis 置 0、min 置 0，使图片尺寸完全由
            // flex-grow 按可用空间分配，不受其固有像素尺寸影响（避免布局抖动）。
            layout = layout
                .flex_grow(1.0)
                .flex_basis(0.0)
                .align_self(FlexAlign::Stretch)
                .min_width(0.0)
                .min_height(0.0);
        }
        ViewNode::Image {
            data: std::sync::Arc::clone(&self.data),
            style: self.style,
            layout,
            key: None,
            listeners: self.listeners.clone(),
        }
    }
}
