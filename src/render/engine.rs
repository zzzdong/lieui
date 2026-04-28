// src/render/engine.rs

use kurbo::{Affine, Shape};
use vello_cpu::{Pixmap, RenderContext};

use crate::render::RenderNode;
use crate::render::node::BoxShadow;

pub struct VelloRenderer {}

impl VelloRenderer {
    /// 创建新的 VelloRenderer
    pub fn new() -> Self {
        VelloRenderer {}
    }

    pub fn render(&self, root: &RenderNode, pixmap: &mut Pixmap) {
        let mut ctx = RenderContext::new(pixmap.width(), pixmap.height());
        self.render_node(&mut ctx, root);
        ctx.flush();
        ctx.render_to_pixmap(pixmap);
    }

    fn render_node(&self, ctx: &mut RenderContext, node: &RenderNode) {
        match node {
            RenderNode::View { children, .. } => {
                for child in children {
                    self.render_node(ctx, child);
                }
            }
            RenderNode::Div {
                bounds,
                background,
                border_color,
                border_width,
                border_radius,
                opacity,
                box_shadow,
                children,
            } => {
                self.render_container(
                    ctx,
                    *bounds,
                    background.as_ref(),
                    border_color.as_ref(),
                    *border_width,
                    *border_radius,
                    *opacity,
                    box_shadow.as_ref(),
                    children,
                );
            }
            RenderNode::Text { bounds, layout } => {
                self.render_text(ctx, *bounds, layout);
            }
            RenderNode::Image {
                bounds,
                data,
                width,
                height,
                ..
            } => {
                self.render_image(ctx, *bounds, data, *width, *height);
            }
            RenderNode::Canvas { bounds, draw } => {
                draw(ctx, *bounds);
            }
        }
    }

    fn render_container(
        &self,
        ctx: &mut RenderContext,
        bounds: crate::geometry::Rect,
        background: Option<&crate::geometry::Color>,
        border_color: Option<&crate::geometry::Color>,
        border_width: Option<f32>,
        border_radius: Option<f32>,
        opacity: Option<f32>,
        box_shadow: Option<&BoxShadow>,
        children: &[RenderNode],
    ) {
        let rect = kurbo::Rect::new(
            bounds.x as f64,
            bounds.y as f64,
            (bounds.x + bounds.width) as f64,
            (bounds.y + bounds.height) as f64,
        );

        let radius = border_radius.unwrap_or(0.0) as f64;

        // 1. 绘制阴影（在背景之前）
        if let Some(shadow) = box_shadow {
            self.render_shadow(ctx, &rect, radius, shadow);
        }

        // 2. 构建圆角路径（背景+边框+clip 共用）
        let path = if radius > 0.0 {
            kurbo::RoundedRect::from_rect(rect, radius).to_path(0.1)
        } else {
            rect.to_path(0.1)
        };

        // 3. 绘制背景
        if let Some(bg) = background {
            ctx.set_paint(bg.0);
            if radius > 0.0 {
                ctx.fill_path(&path);
            } else {
                ctx.fill_rect(&rect);
            }
        }

        // 4. 绘制边框
        if let Some(bc) = border_color {
            let width = border_width.unwrap_or(1.0) as f64;
            ctx.set_paint(bc.0);
            ctx.set_stroke(kurbo::Stroke::new(width));
            if radius > 0.0 {
                ctx.stroke_path(&path);
            } else {
                ctx.stroke_rect(&rect);
            }
        }

        // 5. 设置 clip 并绘制子节点（支持 opacity layer）
        let has_clip = radius > 0.0;
        let has_opacity = opacity.map(|o| o < 1.0).unwrap_or(false);

        if has_opacity {
            ctx.push_opacity_layer(opacity.unwrap_or(1.0));
        }

        if has_clip {
            ctx.push_clip_layer(&path);
        }

        for child in children {
            self.render_node(ctx, child);
        }

        if has_clip {
            ctx.pop_layer();
        }

        if has_opacity {
            ctx.pop_layer();
        }
    }

    fn render_shadow(
        &self,
        ctx: &mut RenderContext,
        rect: &kurbo::Rect,
        radius: f64,
        shadow: &BoxShadow,
    ) {
        let offset_x = shadow.offset_x as f64;
        let offset_y = shadow.offset_y as f64;
        let blur = shadow.blur_radius as f64;
        let spread = shadow.spread_radius as f64;

        // 阴影矩形 = 原矩形 + 偏移 + 扩展
        let shadow_rect = kurbo::Rect::new(
            rect.x0 + offset_x - spread,
            rect.y0 + offset_y - spread,
            rect.x1 + offset_x + spread,
            rect.y1 + offset_y + spread,
        );

        let shadow_radius = (radius + spread).max(0.0) as f32;

        ctx.set_paint(shadow.color.0);

        if blur > 0.0 {
            // 使用 vello_cpu 的模糊圆角矩形
            ctx.fill_blurred_rounded_rect(&shadow_rect, shadow_radius, blur as f32);
        } else if shadow_radius > 0.0 {
            let path =
                kurbo::RoundedRect::from_rect(shadow_rect, shadow_radius as f64).to_path(0.1);
            ctx.fill_path(&path);
        } else {
            ctx.fill_rect(&shadow_rect);
        }
    }

    fn render_text(
        &self,
        ctx: &mut RenderContext,
        bounds: crate::geometry::Rect,
        layout: &crate::text::TextLayout,
    ) {
        let transform = Affine::translate((bounds.x as f64, bounds.y as f64));

        log::debug!(
            "render text at ({}, {}), w: {}, h: {}",
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height
        );

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

                        let brush = glyph_run.style().brush;
                        ctx.set_paint(brush.0.clone());

                        // 使用 vello_cpu 渲染 glyph run
                        ctx.glyph_run(&font_data)
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

    fn render_image(
        &self,
        _ctx: &mut RenderContext,
        _bounds: crate::geometry::Rect,
        _data: &[u8],
        _width: u32,
        _height: u32,
    ) {
        // TODO: 实现图片渲染
        // 可以使用 vello_cpu 的 draw_image 方法
    }
}
