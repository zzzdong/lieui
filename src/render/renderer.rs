//! 渲染器 trait - 定义渲染后端接口

use crate::geometry::Color;
use crate::render::visual::{
    BoxShadowDef, FillStrokeStyle, GradientDef, LayeredElement, Stroke, Transform, VisualElement,
};
use crate::text::TextLayout;
use kurbo::{BezPath, Point, Rect};

/// 渲染器 trait - 所有渲染后端需要实现此 trait
///
/// 设计原则：
/// - 纯数据驱动：接收 VisualElement 纯数据描述
/// - 后端无关：可以用 vello_cpu、wgpu、skia 等任何后端实现
/// - 可扩展：通过 VisualElement::Custom 扩展自定义元素
pub trait Renderer {
    /// 渲染单个视觉元素
    fn draw(&mut self, element: &VisualElement);

    /// 渲染单个带层级的视觉元素
    fn draw_layered(&mut self, layered: &LayeredElement) {
        self.draw(&layered.element);
    }

    /// 渲染视觉元素列表（无层级信息）
    fn draw_all(&mut self, elements: &[VisualElement]) {
        for element in elements {
            self.draw(element);
        }
    }

    /// 渲染带层级的视觉元素列表（会按 z_index 排序后渲染）
    fn draw_all_layered(&mut self, elements: &[LayeredElement]) {
        // 按 z_index 排序，数值小的先渲染（在底层）
        let mut sorted = elements.to_vec();
        sorted.sort_by_key(|e| e.z_index);
        for layered in &sorted {
            self.draw_layered(layered);
        }
    }

    // ---- 基础图形绘制 ----

    /// 绘制矩形
    fn draw_rect(&mut self, rect: Rect, style: &FillStrokeStyle);

    /// 绘制圆角矩形
    fn draw_rounded_rect(&mut self, rect: Rect, radius: f64, style: &FillStrokeStyle);

    /// 绘制圆形
    fn draw_circle(&mut self, center: Point, radius: f64, style: &FillStrokeStyle);

    /// 绘制线条
    fn draw_line(&mut self, start: Point, end: Point, style: &Stroke);

    /// 绘制折线
    fn draw_polyline(&mut self, points: &[Point], style: &Stroke);

    /// 绘制路径
    fn draw_path(&mut self, path: &BezPath, style: &FillStrokeStyle);

    // ---- 渐变 ----

    /// 绘制渐变路径
    fn draw_gradient_path(
        &mut self,
        path: &BezPath,
        gradient: &GradientDef,
        stroke: Option<&Stroke>,
    );

    // ---- 文本 ----

    /// 绘制文本（简化签名）
    ///
    /// # 参数
    /// - `position`: 文本起始位置
    /// - `color`: 文本颜色
    /// - `rotation`: 旋转角度（弧度）
    /// - `layout`: 预计算的文本布局（必需）
    fn draw_text(
        &mut self,
        position: Point,
        color: Color,
        rotation: f64,
        layout: Option<&TextLayout>,
    );

    // ---- 图片 ----

    /// 绘制图片
    fn draw_image(
        &mut self,
        bounds: Rect,
        data: &[u8],
        width: u32,
        height: u32,
        opacity: Option<f32>,
    );

    // ---- 阴影 ----

    /// 绘制盒阴影
    fn draw_box_shadow(&mut self, rect: Rect, radius: f64, shadow: &BoxShadowDef);

    // ---- 变换组合 ----

    /// 开始变换组
    fn push_transform(&mut self, transform: &Transform);

    /// 结束变换组
    fn pop_transform(&mut self);

    /// 开始透明度和裁剪组
    fn push_layer(&mut self, opacity: Option<f32>, clip_path: Option<&BezPath>);

    /// 结束层
    fn pop_layer(&mut self);
}
