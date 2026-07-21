//! Vello CPU 渲染器 - 实现 Renderer trait
//!
//! 改进：
//! - 提取 apply_fill_and_stroke 消除重复模式
//! - 实现 push_transform/pop_transform 变换支持
//! - 简化 draw_text 签名

use kurbo::{Affine, BezPath, Circle, Point, Rect, Shape, Stroke as KurboStroke};
use vello_cpu::peniko::color::AlphaColor;
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::geometry::Color;
use crate::render::renderer::Renderer;
use crate::render::visual::{
    BoxShadowDef, FillStrokeStyle, GradientDef, LayeredElement, Stroke, Transform, VisualElement,
};
use crate::text::TextLayout;

/// Vello CPU 渲染器
///
/// 实现 Renderer trait，将 VisualElement 渲染为位图
pub struct VelloRenderer {
    ctx: RenderContext,
    resources: Resources,
    width: u16,
    height: u16,
    /// 变换栈
    transform_stack: Vec<Affine>,
}

impl VelloRenderer {
    /// 创建新的 VelloRenderer
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            ctx: RenderContext::new(width, height),
            resources: Resources::new(),
            width,
            height,
            transform_stack: Vec::new(),
        }
    }

    /// 渲染视觉元素序列并输出 Pixmap
    pub fn render(&mut self, elements: &[LayeredElement]) -> Pixmap {
        self.draw_all_layered(elements);

        let mut pixmap = Pixmap::new(self.width, self.height);
        self.ctx.render_to_pixmap(&mut self.resources, &mut pixmap);
        pixmap
    }

    /// 渲染到已有的 Pixmap
    pub fn render_to_pixmap(&mut self, elements: &[LayeredElement], pixmap: &mut Pixmap) {
        self.draw_all_layered(elements);
        self.ctx.render_to_pixmap(&mut self.resources, pixmap);
    }

    /// 颜色转换辅助函数
    fn color_to_vello(color: &Color) -> AlphaColor<vello_cpu::peniko::color::Srgb> {
        color.to_vello()
    }

    /// 设置描边样式
    fn set_stroke_style(&mut self, stroke: &Stroke) {
        let kurbo_stroke = KurboStroke::new(stroke.width);
        self.ctx.set_stroke(kurbo_stroke);
    }

    /// 应用填充和描边（消除重复模式）
    ///
    /// # 参数
    /// - `style`: 填充/描边样式
    /// - `fill_fn`: 填充操作（如 `|| self.ctx.fill_path(&path)`）
    /// - `stroke_fn`: 描边操作（如 `|| self.ctx.stroke_path(&path)`）
    fn apply_fill_and_stroke(
        &mut self,
        style: &FillStrokeStyle,
        fill_fn: impl FnOnce(&mut Self),
        stroke_fn: impl FnOnce(&mut Self),
    ) {
        if let Some(fill) = &style.fill {
            let color = Self::color_to_vello(fill);
            self.ctx.set_paint(color);
            fill_fn(self);
        }

        if let Some(stroke) = &style.stroke {
            let color = Self::color_to_vello(&stroke.color);
            self.ctx.set_paint(color);
            self.set_stroke_style(stroke);
            stroke_fn(self);
        }
    }

    /// 获取当前累积变换
    fn current_transform(&self) -> Affine {
        self.transform_stack
            .iter()
            .fold(Affine::IDENTITY, |acc, t| acc * *t)
    }

    /// 应用当前变换到路径
    fn apply_transform_to_path(&self, path: &BezPath) -> BezPath {
        let transform = self.current_transform();
        let mut new_path = path.clone();
        new_path.apply_affine(transform);
        new_path
    }

    /// 应用当前变换到矩形（返回变换后的路径）
    fn apply_transform_to_rect(&self, rect: Rect) -> BezPath {
        let transform = self.current_transform();
        let mut path = BezPath::new();
        path.move_to((rect.x0, rect.y0));
        path.line_to((rect.x1, rect.y0));
        path.line_to((rect.x1, rect.y1));
        path.line_to((rect.x0, rect.y1));
        path.close_path();
        path.apply_affine(transform);
        path
    }
}

impl Renderer for VelloRenderer {
    fn draw(&mut self, element: &VisualElement) {
        match element {
            VisualElement::Rect { rect, style } => {
                self.draw_rect(*rect, style);
            }
            VisualElement::RoundedRect {
                rect,
                radius,
                style,
            } => {
                self.draw_rounded_rect(*rect, *radius, style);
            }
            VisualElement::Circle {
                center,
                radius,
                style,
            } => {
                self.draw_circle(*center, *radius, style);
            }
            VisualElement::Line { start, end, style } => {
                self.draw_line(*start, *end, style);
            }
            VisualElement::Polyline { points, style } => {
                self.draw_polyline(points, style);
            }
            VisualElement::Path { path, style } => {
                self.draw_path(path, style);
            }
            VisualElement::GradientPath {
                path,
                gradient,
                stroke,
            } => {
                self.draw_gradient_path(path, gradient, stroke.as_ref());
            }
            VisualElement::TextRun {
                position,
                color,
                rotation,
                layout,
                ..
            } => {
                self.draw_text(*position, *color, *rotation, layout.as_deref());
            }
            VisualElement::Image {
                bounds,
                data,
                width,
                height,
                opacity,
            } => {
                self.draw_image(*bounds, data.as_slice(), *width, *height, *opacity);
            }
            VisualElement::BoxShadow {
                rect,
                radius,
                shadow,
            } => {
                self.draw_box_shadow(*rect, *radius, shadow);
            }
            VisualElement::Group {
                children,
                transform,
            } => {
                if let Some(t) = transform {
                    self.push_transform(t);
                }
                for layered in children {
                    self.draw(&layered.element);
                }
                if transform.is_some() {
                    self.pop_transform();
                }
            }
        }
    }

    fn draw_rect(&mut self, rect: Rect, style: &FillStrokeStyle) {
        // 应用变换（矩形变换后可能不是矩形，用路径表示）
        let transformed_path = self.apply_transform_to_rect(rect);

        self.apply_fill_and_stroke(
            style,
            |renderer| renderer.ctx.fill_path(&transformed_path),
            |renderer| renderer.ctx.stroke_path(&transformed_path),
        );
    }

    fn draw_rounded_rect(&mut self, rect: Rect, radius: f64, style: &FillStrokeStyle) {
        let path = kurbo::RoundedRect::from_rect(rect, radius).to_path(0.1);
        let transformed_path = self.apply_transform_to_path(&path);

        self.apply_fill_and_stroke(
            style,
            |renderer| renderer.ctx.fill_path(&transformed_path),
            |renderer| renderer.ctx.stroke_path(&transformed_path),
        );
    }

    fn draw_circle(&mut self, center: Point, radius: f64, style: &FillStrokeStyle) {
        let circle = Circle::new(center, radius);
        let path = circle.to_path(0.1);
        let transformed_path = self.apply_transform_to_path(&path);

        self.apply_fill_and_stroke(
            style,
            |renderer| renderer.ctx.fill_path(&transformed_path),
            |renderer| renderer.ctx.stroke_path(&transformed_path),
        );
    }

    fn draw_line(&mut self, start: Point, end: Point, style: &Stroke) {
        let transform = self.current_transform();
        let transformed_start = transform * start;
        let transformed_end = transform * end;

        let color = Self::color_to_vello(&style.color);
        self.ctx.set_paint(color);
        self.ctx.set_stroke(KurboStroke::new(style.width));

        let mut path = BezPath::new();
        path.move_to(transformed_start);
        path.line_to(transformed_end);
        self.ctx.stroke_path(&path);
    }

    fn draw_polyline(&mut self, points: &[Point], style: &Stroke) {
        if points.len() < 2 {
            return;
        }

        let transform = self.current_transform();
        let mut path = BezPath::new();
        path.move_to(transform * points[0]);
        for point in &points[1..] {
            path.line_to(transform * *point);
        }

        let color = Self::color_to_vello(&style.color);
        self.ctx.set_paint(color);
        self.ctx.set_stroke(KurboStroke::new(style.width));
        self.ctx.stroke_path(&path);
    }

    fn draw_path(&mut self, path: &BezPath, style: &FillStrokeStyle) {
        let transformed_path = self.apply_transform_to_path(path);

        self.apply_fill_and_stroke(
            style,
            |renderer| renderer.ctx.fill_path(&transformed_path),
            |renderer| renderer.ctx.stroke_path(&transformed_path),
        );
    }

    fn draw_gradient_path(
        &mut self,
        path: &BezPath,
        gradient: &GradientDef,
        stroke: Option<&Stroke>,
    ) {
        use vello_cpu::kurbo::Point as KurboPoint;
        use vello_cpu::peniko::color::{ColorSpaceTag, DynamicColor, HueDirection};
        use vello_cpu::peniko::{
            ColorStop, ColorStops, GradientKind, InterpolationAlphaSpace, LinearGradientPosition,
        };
        use vello_cpu::peniko::{Extend, Gradient};

        let transformed_path = self.apply_transform_to_path(path);

        let stops: Vec<ColorStop> = gradient
            .stops
            .iter()
            .map(|(offset, color)| ColorStop {
                offset: *offset as f32,
                color: DynamicColor::from_alpha_color(color.to_vello()),
            })
            .collect();

        // 使用路径的包围盒来确定渐变坐标
        let bounds = transformed_path.bounding_box();
        let peniko_gradient = Gradient {
            kind: GradientKind::Linear(LinearGradientPosition {
                start: KurboPoint::new(bounds.x0, bounds.y0),
                end: KurboPoint::new(bounds.x1, bounds.y0),
            }),
            extend: Extend::Pad,
            interpolation_cs: ColorSpaceTag::Srgb,
            hue_direction: HueDirection::default(),
            interpolation_alpha_space: InterpolationAlphaSpace::Premultiplied,
            stops: ColorStops::from(stops.as_slice()),
        };

        self.ctx.set_paint(peniko_gradient);
        self.ctx.fill_path(&transformed_path);

        if let Some(stroke) = stroke {
            let color = Self::color_to_vello(&stroke.color);
            self.ctx.set_paint(color);
            self.set_stroke_style(stroke);
            self.ctx.stroke_path(&transformed_path);
        }
    }

    /// 绘制文本（简化签名）
    ///
    /// # 参数
    /// - `position`: 文本起始位置
    /// - `color`: 文本颜色
    /// - `rotation`: 旋转角度（弧度）
    /// - `layout`: 预计算的文本布局
    fn draw_text(
        &mut self,
        position: Point,
        color: Color,
        rotation: f64,
        layout: Option<&TextLayout>,
    ) {
        let Some(layout) = layout else {
            return;
        };

        // 应用当前变换栈 + 位置/旋转
        let base_transform = self.current_transform();
        let transform =
            base_transform * Affine::translate((position.x, position.y)) * Affine::rotate(rotation);

        let vello_color = Self::color_to_vello(&color);
        self.ctx.set_paint(vello_color);

        // 遍历布局中的每一行和每个 glyph run
        for line in layout.lines() {
            for item in line.items() {
                match item {
                    parley::layout::PositionedLayoutItem::GlyphRun(glyph_run) => {
                        let run = glyph_run.run();

                        // 获取字体数据
                        let font_data = run.font();
                        let run_font_size = run.font_size();

                        // 将 parley 的 glyph 转换为 vello_cpu 的 glyph
                        let glyphs: Vec<vello_cpu::Glyph> = glyph_run
                            .positioned_glyphs()
                            .map(|g| vello_cpu::Glyph {
                                id: g.id,
                                x: g.x,
                                y: g.y,
                            })
                            .collect();

                        if glyphs.is_empty() {
                            continue;
                        }

                        // 使用 vello_cpu 渲染 glyph run
                        self.ctx
                            .glyph_run(&mut self.resources, font_data)
                            .font_size(run_font_size)
                            .glyph_transform(transform)
                            .fill_glyphs(glyphs.into_iter());
                    }
                    parley::layout::PositionedLayoutItem::InlineBox(_inline_box) => {
                        // 内联盒子：目前暂不处理
                    }
                }
            }
        }
    }

    fn draw_image(
        &mut self,
        bounds: Rect,
        data: &[u8],
        width: u32,
        height: u32,
        opacity: Option<f32>,
    ) {
        let w = width as u16;
        let h = height as u16;
        let expected = w as usize * h as usize * 4;
        if expected == 0 || data.len() < expected {
            return;
        }

        // 把 RGBA8 像素写入一个 vello_cpu Pixmap。
        let mut pixmap = Pixmap::new(w, h);
        pixmap.data_as_u8_slice_mut()[..expected].copy_from_slice(&data[..expected]);

        // 使用 Pixmap 变体：像素随场景包一起发送，无需注册/清理，不泄漏。
        let source = vello_cpu::ImageSource::Pixmap(std::sync::Arc::new(pixmap));
        let image = vello_cpu::Image {
            image: source,
            sampler: vello_cpu::peniko::ImageSampler::default(),
        };

        let bw = bounds.x1 - bounds.x0;
        let bh = bounds.y1 - bounds.y0;
        let sx = bw / width as f64;
        let sy = bh / height as f64;

        let base = self.current_transform();
        self.ctx.set_transform(base);
        self.ctx.set_paint(vello_cpu::PaintType::Image(image));
        self.ctx.set_paint_transform(Affine::new([
            sx, 0.0, 0.0, sy, bounds.x0, bounds.y0,
        ]));
        if let Some(op) = opacity {
            self.ctx.push_opacity_layer(op);
        }
        self.ctx.fill_rect(&Rect::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1));
        if opacity.is_some() {
            self.ctx.pop_layer();
        }
        self.ctx.reset_paint_transform();
    }

    fn draw_box_shadow(&mut self, rect: Rect, radius: f64, shadow: &BoxShadowDef) {
        // 应用变换（矩形变换后可能不是矩形，用路径表示）
        let transform = self.current_transform();
        let rect_path = {
            let mut path = BezPath::new();
            path.move_to((rect.x0, rect.y0));
            path.line_to((rect.x1, rect.y0));
            path.line_to((rect.x1, rect.y1));
            path.line_to((rect.x0, rect.y1));
            path.close_path();
            path.apply_affine(transform);
            path
        };
        let transformed_bounds = rect_path.bounding_box();

        let offset_x = shadow.offset_x;
        let offset_y = shadow.offset_y;
        let blur = shadow.blur_radius;
        let spread = shadow.spread_radius;

        // 阴影矩形 = 原矩形 + 偏移 + 扩展
        let shadow_rect = Rect::new(
            transformed_bounds.x0 + offset_x - spread,
            transformed_bounds.y0 + offset_y - spread,
            transformed_bounds.x1 + offset_x + spread,
            transformed_bounds.y1 + offset_y + spread,
        );

        let shadow_radius = (radius + spread).max(0.0) as f32;

        self.ctx.set_paint(Self::color_to_vello(&shadow.color));

        if blur > 0.0 {
            // 使用 vello_cpu 的模糊圆角矩形
            self.ctx
                .fill_blurred_rounded_rect(&shadow_rect, shadow_radius, blur as f32);
        } else if shadow_radius > 0.0 {
            let path =
                kurbo::RoundedRect::from_rect(shadow_rect, shadow_radius as f64).to_path(0.1);
            self.ctx.fill_path(&path);
        } else {
            self.ctx.fill_rect(&shadow_rect);
        }
    }

    /// 推入变换（实现）
    fn push_transform(&mut self, transform: &Transform) {
        let affine = transform.to_affine();
        self.transform_stack.push(affine);
    }

    /// 弹出变换（实现）
    fn pop_transform(&mut self) {
        self.transform_stack.pop();
    }

    fn push_layer(&mut self, opacity: Option<f32>, clip_path: Option<&BezPath>) {
        if let Some(alpha) = opacity {
            self.ctx.push_opacity_layer(alpha);
        }

        if let Some(path) = clip_path {
            // 应用当前变换到裁剪路径
            let transformed_path = self.apply_transform_to_path(path);
            self.ctx.push_clip_layer(&transformed_path);
        }
    }

    fn pop_layer(&mut self) {
        self.ctx.pop_layer();
    }
}
