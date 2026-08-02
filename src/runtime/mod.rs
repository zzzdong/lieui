//! Runtime — v2 核心"服务端"
pub mod element;
pub mod reconciler;
use crate::core::layers::{Anchor, LayerKind, LayerStack};
use crate::geometry::Size;
use crate::layout::context::LayoutContext;
use crate::render::visual::LayeredElement;
use crate::runtime::reconciler::Reconciler;
use crate::state;
use crate::view::node::ViewNode;
pub use element::ElementTree;

pub struct Runtime {
    pub layers: LayerStack,
    pub(crate) viewport: Size,
    needs_layout: bool,
    needs_render: bool,
    /// 强制重排：滚动偏移变化时不标记脏节点，但需重跑布局以重写子节点偏移坐标。
    force_layout: bool,
    pending_view_tree: Option<std::rc::Rc<ViewNode>>,
    /// 上一次提交的 ViewNode 树，用于缓存 diff 短路。
    /// 当 builder 产出与上次完全相同的树时，跳过 Reconciler 和 callback 清空。
    /// 与 pending 共享同一 Rc，避免每次提交都深克隆整棵树。
    last_view_tree: Option<std::rc::Rc<ViewNode>>,
    pub debug_stats: DebugStats,
}
#[derive(Debug, Default)]
pub struct DebugStats {
    pub reconciler: crate::runtime::reconciler::ReconcilerStats,
    pub element_count: usize,
}
impl Runtime {
    pub fn new(viewport: Size) -> Self {
        Self {
            layers: LayerStack::new(),
            viewport,
            needs_layout: true,
            needs_render: true,
            force_layout: false,
            pending_view_tree: None,
            last_view_tree: None,
            debug_stats: DebugStats::default(),
        }
    }
    pub fn set_viewport(&mut self, vp: Size) {
        self.viewport = vp;
        self.layers.tree.mark_dirty_all();
        self.needs_layout = true;
        self.needs_render = true;
    }
    /// 提交新的 ViewNode 树。若与上次提交的树完全相同，返回 `false`，
    /// 调用方可据此跳过 Reconciler、布局与渲染管线的重复执行。
    ///
    /// `rebuild_requested`：当为 `true` 时跳过 `tree_eq()` 比较，直接进入 Reconciler。
    /// 因为 `state::request_rebuild()` 已被消费，此处通过参数传递。
    pub fn submit_view_tree(&mut self, vt: ViewNode, rebuild_requested: bool) -> bool {
        if !rebuild_requested
            && self
                .last_view_tree
                .as_ref()
                .is_some_and(|last| last.tree_eq(&vt))
        {
            // 树无变化，保留上一次的缓存，不触发 Reconciler。
            self.pending_view_tree = None;
            return false;
        }
        let vt = std::rc::Rc::new(vt);
        self.last_view_tree = Some(std::rc::Rc::clone(&vt));
        self.pending_view_tree = Some(vt);
        self.needs_render = true;
        true
    }

    /// 请求在下一次 frame 时重排（由事件回调的 `EventEffects` 触发）
    pub fn request_layout(&mut self) {
        self.needs_layout = true;
    }

    /// 请求在下一次 frame 时重新生成渲染树（由事件回调的 `EventEffects` 触发）
    pub fn request_render(&mut self) {
        self.needs_render = true;
    }

    pub fn frame(&mut self, wid: winit::window::WindowId) -> Vec<LayeredElement> {
        if (state::take_rebuild_requested(wid) || self.pending_view_tree.is_some())
            && let Some(vt) = self.pending_view_tree.take()
        {
            let vt: &ViewNode = &vt;
            let span = crate::perf::Span::start("reconcile");
            let mut r = Reconciler::new();
            if self.layers.content_root_id().is_none() {
                let id = self.layers.tree.create_from_node(vt);
                self.layers.tree.set_root(id);
                self.layers.set_content_root(id);
                let mut p = Vec::new();
                r.diff(vt, id, &self.layers.tree, &mut p);
                r.apply(p, &mut self.layers.tree);
            } else {
                let rid = self.layers.content_root_id().unwrap();
                let mut p = Vec::new();
                r.diff(vt, rid, &self.layers.tree, &mut p);
                r.apply(p, &mut self.layers.tree);
                self.debug_stats.reconciler = r.stats;
            }
            span.finish();
            if crate::perf::enabled() {
                let s = &self.debug_stats.reconciler;
                eprintln!(
                    "[lieui-perf] reconcile-stats  created={} updated={} removed={} moved={}",
                    s.created, s.updated, s.removed, s.moved
                );
            }
            self.needs_layout = true;
        }

        // 处理待显示的浮层命令（Modal / Overlay / Popup / Tooltip / System）。
        // 注意：必须用 create_subtree_from_node 递归构建完整子树，
        // 否则浮层只有根节点（如 dialog 背景）而无内容（标题/按钮）。
        let pending_layers = state::take_pending_layers();
        if !pending_layers.is_empty() {
            for cmd in pending_layers {
                use crate::core::layers::LayerKind;
                match cmd {
                    state::LayerCmd::Hide { kind } => match kind {
                        LayerKind::Modal => {
                            self.layers.remove_default_modal();
                        }
                        LayerKind::Overlay => {
                            self.layers.remove_default_overlay();
                        }
                        _ => {}
                    },
                    state::LayerCmd::Show { kind, spec, view } => match kind {
                        LayerKind::Modal => {
                            let id = self.layers.tree.create_subtree_from_node(&view);
                            self.layers.push_default_modal(id);
                        }
                        LayerKind::Overlay => {
                            let id = self.layers.tree.create_subtree_from_node(&view);
                            self.layers.push_default_overlay(id);
                        }
                        _ => {
                            self.layers
                                .push(kind, *view, spec.anchor, spec.focus, spec.opts);
                        }
                    },
                }
            }
            self.needs_layout = true;
        }
        if self.needs_layout {
            let span = crate::perf::Span::start("layout");
            self.perform_layout();
            span.finish();
            self.needs_layout = false;
            // 调试：LIEUI_DUMP_LAYOUT=1 时打印布局树
            if cfg!(debug_assertions) && std::env::var("LIEUI_DUMP_LAYOUT").is_ok_and(|v| v == "1")
            {
                use std::sync::OnceLock;
                static DUMPED: OnceLock<bool> = OnceLock::new();
                if DUMPED.set(true).is_ok() {
                    eprintln!("=== LIEUI Layout Tree ===");
                    for entry in self.layers.sorted_entries_for_render() {
                        let rid = entry.root_id;
                        eprintln!("--- Layer: {:?} (z={}) ---", entry.kind, entry.z());
                        fn dump_layout(
                            tree: &element::ElementTree,
                            id: crate::core::ElementId,
                            depth: usize,
                        ) {
                            let indent = "  ".repeat(depth);
                            let nt = tree.get_node_ref(id).map(|n| n.type_name()).unwrap_or("?");
                            let l = tree.layout(id);
                            eprintln!(
                                "{}{:?} [{}] pos=({:.1},{:.1}) size=({:.1},{:.1})",
                                indent, id, nt, l.x, l.y, l.width, l.height
                            );
                            for cid in tree.children_of(id) {
                                dump_layout(tree, cid, depth + 1);
                            }
                        }
                        dump_layout(&self.layers.tree, rid, 0);
                    }
                }
            }
        }
        if self.needs_render {
            self.needs_render = false;
            self.debug_stats.element_count = self.layers.tree.len();
            let span = crate::perf::Span::start("render-tree");
            let e = self.build_render_tree();
            span.finish();
            e
        } else {
            Vec::new()
        }
    }

    /// 仅重新生成渲染树（不跑 builder/reconciliation/layout）
    /// 用于 hover/pressed 等视觉交互更新
    pub fn frame_render_only(&self) -> Vec<LayeredElement> {
        self.build_render_tree()
    }

    /// 增量视觉更新：必要时重排，然后重新生成渲染树。
    /// 用于事件回调通过 `EventEffects` 请求 layout/render 的增量更新场景
    /// （如 `ctx.request_layout()` / `ctx.request_render()`）。
    pub fn frame_visual_update(&mut self) -> Vec<LayeredElement> {
        if !self.needs_layout && !self.needs_render {
            return Vec::new();
        }
        if self.needs_layout {
            self.perform_layout();
            self.needs_layout = false;
        }
        if self.needs_render {
            self.needs_render = false;
            self.debug_stats.element_count = self.layers.tree.len();
            self.build_render_tree()
        } else {
            Vec::new()
        }
    }

    fn perform_layout(&mut self) {
        // 若整棵树没有 dirty 节点且无需强制重排，则跳过（viewport 变化时会全量标记 dirty）。
        if !self.force_layout && !self.layers.tree.has_dirty_node() {
            return;
        }
        self.force_layout = false;
        for entry in self.layers.sorted_entries_for_render() {
            let rid = entry.root_id;
            if entry.kind == LayerKind::Content {
                // Content 走完整 Flex 布局（现有流程），根节点填充视口。
                LayoutContext::compute(rid, &self.layers.tree, self.viewport, true);
            } else {
                // 其他 LayerKind（Modal/Popup/Overlay/Tooltip）：按内容自然尺寸布局，
                // 避免根节点被撑满整个视口（如 modal 高度占满窗口），再按 Anchor 定位。
                LayoutContext::compute(rid, &self.layers.tree, self.viewport, false);
                Self::apply_anchor(&mut self.layers, entry.anchor, rid, self.viewport);
            }
        }
        self.layers.tree.clear_dirty();
    }

    /// 依据 Anchor 将层根子树平移到目标位置。
    /// 布局阶段先按 viewport 约束在 (0,0) 计算子树尺寸，这里再按锚点计算平移量。
    fn apply_anchor(
        layers: &mut LayerStack,
        anchor: Anchor,
        root: crate::core::ElementId,
        viewport: crate::geometry::Size,
    ) {
        let l = layers.tree.layout(root);
        let (w, h) = (l.width, l.height);
        let (dx, dy) = match anchor {
            Anchor::None => (0.0, 0.0),
            Anchor::Fixed { x, y } => (x - l.x, y - l.y),
            Anchor::ScreenCenter => {
                // root 在 (0,0) 计算，translate 使其中点对齐 viewport 中心。
                (
                    (viewport.width - w) / 2.0 - l.x,
                    (viewport.height - h) / 2.0 - l.y,
                )
            }
            Anchor::Above { anchor, gap } => {
                let r = anchor;
                (r.x + (r.width - w) / 2.0 - l.x, r.y - gap - h - l.y)
            }
            Anchor::Below { anchor, gap } => {
                let r = anchor;
                (r.x + (r.width - w) / 2.0 - l.x, r.y + r.height + gap - l.y)
            }
            Anchor::LeftOf { anchor, gap } => {
                let r = anchor;
                (r.x - gap - w - l.x, r.y + (r.height - h) / 2.0 - l.y)
            }
            Anchor::RightOf { anchor, gap } => {
                let r = anchor;
                (r.x + r.width + gap - l.x, r.y + (r.height - h) / 2.0 - l.y)
            }
        };
        layers.tree.translate_subtree(root, dx, dy);
    }

    /// 调整滚动容器偏移并钳制到内容范围，请求重排+重绘。
    /// 滚动不改变 Style（不触发 rebuild/重测量），只驱动布局平移与裁剪。
    /// 如果容器绑定了 `scroll_state`，自动同步写入。
    pub fn scroll_by(&mut self, id: crate::core::ElementId, dx: f32, dy: f32) {
        let cl = self.layers.tree.layout(id);
        if !cl.overflow_scroll {
            return;
        }
        let (ox, oy) = self.layers.tree.scroll_offset(id);
        let (cw, ch) = self.layers.tree.content_size(id);
        let vw = cl.width;
        let vh = cl.height;
        let max_x = (cw - vw).max(0.0);
        let max_y = (ch - vh).max(0.0);
        let nox = (ox + dx).clamp(0.0, max_x);
        let noy = (oy + dy).clamp(0.0, max_y);
        self.layers.tree.set_scroll_offset(id, (nox, noy));
        // 同步更新绑定的 scroll_state（如 VirtualList 需要感知偏移以重新计算窗口）
        if let Some(s) = self.layers.tree.scroll_state(id) {
            s.set((nox, noy));
        }
        self.needs_layout = true;
        self.needs_render = true;
        self.force_layout = true;
    }

    /// 直接将滚动容器偏移跳转到指定位置（像素）。
    /// 与 `scroll_by` 不同，`scroll_to` 是绝对定位而非增量。
    pub fn scroll_to(&mut self, id: crate::core::ElementId, x: f32, y: f32) {
        let cl = self.layers.tree.layout(id);
        if !cl.overflow_scroll {
            return;
        }
        let (cw, ch) = self.layers.tree.content_size(id);
        let vw = cl.width;
        let vh = cl.height;
        let max_x = (cw - vw).max(0.0);
        let max_y = (ch - vh).max(0.0);
        let nox = x.clamp(0.0, max_x);
        let noy = y.clamp(0.0, max_y);
        self.layers.tree.set_scroll_offset(id, (nox, noy));
        if let Some(s) = self.layers.tree.scroll_state(id) {
            s.set((nox, noy));
        }
        self.needs_layout = true;
        self.needs_render = true;
        self.force_layout = true;
    }

    /// 滚轮事件：从命中目标向上查找最近的 overflow_scroll 容器并滚动它。
    pub fn handle_wheel_scroll(&mut self, hit: &crate::event::HitTestResult, dx: f32, dy: f32) {
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        for &id in hit.path.iter().rev() {
            if let Some(node) = self.layers.tree.get_node_ref(id)
                && node.layout().overflow_scroll
            {
                self.scroll_by(id, dx, dy);
                return;
            }
        }
    }

    fn build_render_tree(&self) -> Vec<LayeredElement> {
        let mut e = Vec::new();
        for entry in self.layers.sorted_entries_for_render() {
            if !entry.visible.get() {
                continue;
            }
            let z = entry.z();
            // Modal backdrop：在 Modal 内容之前绘制，z = entry.z() - 1
            if let Some(color) = entry.backdrop {
                let vp = &self.viewport;
                e.push(crate::render::visual::LayeredElement::new(
                    crate::render::visual::VisualElement::RoundedRect {
                        rect: crate::render::visual::KRect::new(
                            0.0,
                            0.0,
                            vp.width as f64,
                            vp.height as f64,
                        ),
                        radius: 0.0,
                        style: crate::render::visual::FillStrokeStyle::new().with_fill(color),
                    },
                    z - 1,
                ));
            }
            Self::cv(entry.root_id, z, &self.layers, &mut e, None);
        }
        e
    }

    fn cv(
        id: crate::core::ElementId,
        z_index: i32,
        layers: &LayerStack,
        elements: &mut Vec<LayeredElement>,
        listener_state: Option<crate::core::state::ElementState>,
    ) {
        let node_ref = match layers.tree.get_node_ref(id) {
            Some(n) => n,
            None => return,
        };
        let computed = layers.tree.layout(id);
        // HTML 式状态继承：当前节点自身有 listener 或声明了 hover/pressed
        // 视觉样式时使用自身状态；否则继承最近的可交互祖先状态。
        // 这样 Button/Checkbox 等组件根设置 hover/pressed 后，内部子节点会跟随变化，
        // 无回调的 IconButton 也能正确显示 hover/pressed 反馈。
        let own_state = layers.tree.state(id);
        let interactive = layers.tree.has_any_listener(id) || layers.tree.is_interactive(id);
        let state = if interactive {
            own_state
        } else {
            listener_state.unwrap_or_default()
        };
        let next_listener_state = if interactive {
            Some(own_state)
        } else {
            listener_state
        };

        // 在 cv 中内联渲染（替代 ViewNode::render() 间接调用）
        if let crate::view::node::ViewNode::Text { content, style, .. } = node_ref {
            let r = computed.rect();
            // 计算内容区宽度：实际布局宽度减去水平 padding/border。
            // 文本排版缓存应以此宽度作为 max_width，否则长文本可能不按要求换行。
            let layout = node_ref.layout();
            let content_w =
                (computed.width - layout.horizontal_padding() - layout.horizontal_border())
                    .max(0.0);
            let mut layout_style = style.clone();
            // wrap=false 时跳过 max_width 二次限制，避免渲染阶段又把单行文本压缩换行。
            if style.wrap {
                layout_style.max_width = Some(
                    layout_style
                        .max_width
                        .unwrap_or(f64::MAX)
                        .min(content_w as f64),
                );
            }
            // 交互状态变色：图标/文本在 hover/pressed 时切换颜色
            // （Text 节点无 listener 时继承最近祖先的交互状态，见上方状态传播逻辑）。
            let color = if state.pressed {
                layout_style.pressed_color.unwrap_or(layout_style.color)
            } else if state.hovered {
                layout_style.hover_color.unwrap_or(layout_style.color)
            } else {
                layout_style.color
            };

            // 复用 TextLayout 缓存，避免每帧重新排版：
            // 命中时直接返回 Arc 克隆，未命中时才创建并写入缓存一次。
            let lay = match layers.tree.peek_text_layout_cache(id) {
                Some(cached) => cached,
                None => {
                    let lay = std::sync::Arc::new(crate::text::create_text_layout(
                        content,
                        &layout_style,
                    ));
                    layers
                        .tree
                        .set_text_layout_cache(id, std::sync::Arc::clone(&lay));
                    lay
                }
            };
            elements.push(
                crate::render::visual::LayeredElement::new(
                    crate::render::visual::VisualElement::TextRun {
                        text: std::sync::Arc::from(content.as_str()),
                        position: kurbo::Point::new(r.x as f64, r.y as f64),
                        color,
                        font_size: layout_style.font_size,
                        font_family: layout_style.font_family.clone(),
                        rotation: 0.0,
                        max_width: layout_style.max_width,
                        layout: Some(lay),
                    },
                    z_index,
                )
                .with_id(id.as_ffi()),
            );
        } else if let crate::view::node::ViewNode::Image { data, style, .. } = node_ref {
            let r = computed.rect();
            elements.push(
                crate::render::visual::LayeredElement::new(
                    crate::render::visual::VisualElement::Image {
                        bounds: crate::render::visual::KRect::new(
                            r.x as f64,
                            r.y as f64,
                            (r.x + r.width) as f64,
                            (r.y + r.height) as f64,
                        ),
                        data: std::sync::Arc::clone(data),
                        width: style.width,
                        height: style.height,
                        opacity: Some(style.opacity),
                        fit: style.fit,
                        border_radius: style.border_radius,
                    },
                    z_index,
                )
                .with_id(id.as_ffi()),
            );
        } else if let crate::view::node::ViewNode::Div { paint, .. } = node_ref {
            let bg = if state.pressed && paint.pressed_background.is_some() {
                paint.pressed_background
            } else if state.hovered && paint.hover_background.is_some() {
                paint.hover_background
            } else {
                paint.background_color
            };
            let rect = computed.rect();
            // 投影：在背景矩形之前绘制，保证位于其下方
            if let Some(sh) = paint.shadow {
                let rx = rect.x as f64;
                let ry = rect.y as f64;
                let rw = rect.width as f64;
                let rh = rect.height as f64;
                let sx = rx + sh.offset_x as f64 - sh.spread as f64;
                let sy = ry + sh.offset_y as f64 - sh.spread as f64;
                let sw = rw + 2.0 * sh.spread as f64;
                let shh = rh + 2.0 * sh.spread as f64;
                elements.push(
                    crate::render::visual::LayeredElement::new(
                        crate::render::visual::VisualElement::ShadowRoundedRect {
                            rect: crate::render::visual::KRect::new(sx, sy, sx + sw, sy + shh),
                            radius: (paint.border_radius + sh.spread) as f64,
                            std_dev: sh.blur as f64,
                            color: sh.color,
                        },
                        z_index,
                    )
                    .with_id(id.as_ffi()),
                );
            }
            if let Some(bg) = bg {
                let mut fs = crate::render::visual::FillStrokeStyle::new().with_fill(bg);
                if let Some(bc) = paint.border_color
                    && paint.border_width > 0.0
                {
                    fs = fs.with_stroke(bc, paint.border_width as f64);
                }
                elements.push(
                    crate::render::visual::LayeredElement::new(
                        crate::render::visual::VisualElement::RoundedRect {
                            rect: crate::render::visual::KRect::new(
                                rect.x as f64,
                                rect.y as f64,
                                (rect.x + rect.width) as f64,
                                (rect.y + rect.height) as f64,
                            ),
                            radius: paint.border_radius as f64,
                            style: fs,
                        },
                        z_index,
                    )
                    .with_id(id.as_ffi()),
                );
            }

            // 子节点渲染：clip_content 或 overflow_scroll 时收集到 Group 并设置 clip_rect，
            // 否则保持平铺以最大化渲染性能。
            if paint.clip_content || computed.overflow_scroll {
                let mut child_elements = Vec::new();
                for &cid in layers.tree.children_ref(id) {
                    Self::cv(
                        cid,
                        z_index,
                        layers,
                        &mut child_elements,
                        next_listener_state,
                    );
                }
                if !child_elements.is_empty() {
                    let r = computed.rect();
                    if crate::perf::enabled() && computed.overflow_scroll {
                        let scroll_off = layers.tree.scroll_offset(id);
                        eprintln!(
                            "[scroll] r={:.0},{:.0}+{:.0}x{:.0} offset=({:.1},{:.1}) content=({:.0},{:.0})",
                            r.x,
                            r.y,
                            r.width,
                            r.height,
                            scroll_off.0,
                            scroll_off.1,
                            layers.tree.content_size(id).0,
                            layers.tree.content_size(id).1,
                        );
                    }
                    elements.push(
                        crate::render::visual::LayeredElement::new(
                            crate::render::visual::VisualElement::Group {
                                children: child_elements,
                                transform: None,
                                clip_rect: Some(crate::render::visual::KRect::new(
                                    r.x as f64,
                                    r.y as f64,
                                    (r.x + r.width) as f64,
                                    (r.y + r.height) as f64,
                                )),
                            },
                            z_index,
                        )
                        .with_id(id.as_ffi()),
                    );
                }

                // 引擎层自绘滚动条——布局后已知确切实时视口/内容尺寸，thumb 计算精确
                if computed.overflow_scroll && node_ref.layout().show_scrollbar {
                    let rr = computed.rect();
                    let sw = 8.0f32;
                    let (_ox, oy) = layers.tree.scroll_offset(id);
                    let (_cw, ch) = layers.tree.content_size(id);
                    if ch > 0.0 && rr.height > 0.0 {
                        let track_x = rr.x + rr.width - sw;
                        let track_y = rr.y;
                        let track_h = rr.height;
                        let max_scroll = (ch - rr.height).max(1.0);
                        let thumb_ratio = (rr.height / ch).clamp(0.02, 1.0);
                        let thumb_size = (track_h * thumb_ratio).max(8.0);
                        let thumb_off = (oy / max_scroll) * (track_h - thumb_size);
                        let radius = 3.0;
                        // track
                        elements.push(
                            crate::render::visual::LayeredElement::new(
                                crate::render::visual::VisualElement::RoundedRect {
                                    rect: crate::render::visual::KRect::new(
                                        track_x as f64,
                                        track_y as f64,
                                        (track_x + sw) as f64,
                                        (track_y + track_h) as f64,
                                    ),
                                    radius,
                                    style: crate::render::visual::FillStrokeStyle::new()
                                        .with_fill(crate::geometry::Color::rgba(0, 0, 0, 16)),
                                },
                                z_index + 1,
                            )
                            .with_id(id.as_ffi()),
                        );
                        // thumb
                        elements.push(
                            crate::render::visual::LayeredElement::new(
                                crate::render::visual::VisualElement::RoundedRect {
                                    rect: crate::render::visual::KRect::new(
                                        track_x as f64,
                                        (track_y + thumb_off) as f64,
                                        (track_x + sw) as f64,
                                        (track_y + thumb_off + thumb_size) as f64,
                                    ),
                                    radius,
                                    style: crate::render::visual::FillStrokeStyle::new()
                                        .with_fill(crate::geometry::Color::rgba(0, 0, 0, 80)),
                                },
                                z_index + 2,
                            )
                            .with_id(id.as_ffi()),
                        );
                    }
                }
            } else {
                for &cid in layers.tree.children_ref(id) {
                    Self::cv(cid, z_index, layers, elements, next_listener_state);
                }
            }
            return;
        }

        // Text / Image 为叶子节点，无 children；Div 分支已在上方处理。
        for &cid in layers.tree.children_ref(id) {
            Self::cv(cid, z_index, layers, elements, next_listener_state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Color, Size};
    use crate::layout::style::FlexStyle;
    use crate::render::renderer::Renderer;
    use crate::render::visual::VisualElement;
    use crate::view::paint::PaintStyle;
    use crate::widget::BuildContext;
    use crate::widget::Widget;
    use crate::widget::scroll_view::ScrollView;
    use winit::window::WindowId;

    fn div(layout: FlexStyle, paint: PaintStyle, children: Vec<ViewNode>) -> ViewNode {
        ViewNode::Div {
            layout,
            paint,
            key: None,
            children,
            listeners: Vec::new(),
        }
    }

    /// 测试用 widget：产出一个固定尺寸、纯色的盒子（模拟溢出内容）。
    struct SolidBox;
    impl Widget for SolidBox {
        fn build(&self, _ctx: &mut BuildContext) -> ViewNode {
            colored_box(200.0, 200.0, Color::RED)
        }
    }

    fn colored_box(w: f32, h: f32, color: Color) -> ViewNode {
        ViewNode::Div {
            layout: FlexStyle::default().width(w).height(h),
            paint: PaintStyle::default().background(color),
            key: None,
            children: vec![],
            listeners: Vec::new(),
        }
    }

    #[test]
    fn clip_content_generates_group_with_clip_rect() {
        let root = div(
            FlexStyle::block().width(100.0).height(100.0),
            PaintStyle::default().background(Color::WHITE).clip(true),
            vec![colored_box(200.0, 200.0, Color::RED)],
        );

        let mut rt = Runtime::new(Size::new(100.0, 100.0));
        rt.submit_view_tree(root, false);
        let rendered = rt.frame(WindowId::dummy());

        // 应该有容器背景 + 一个 Group
        assert_eq!(rendered.len(), 2, "expected background + one Group");
        let group = rendered
            .iter()
            .find(|e| matches!(e.element, VisualElement::Group { .. }))
            .expect("clip_content should produce a Group");
        if let VisualElement::Group {
            clip_rect: Some(rect),
            children,
            ..
        } = &group.element
        {
            assert!((rect.x1 - rect.x0 - 100.0).abs() < 0.01);
            assert!((rect.y1 - rect.y0 - 100.0).abs() < 0.01);
            assert_eq!(children.len(), 1);
        } else {
            panic!("Group should have a clip_rect");
        }
    }

    #[test]
    fn no_clip_content_keeps_children_flat() {
        let root = div(
            FlexStyle::block().width(100.0).height(100.0),
            PaintStyle::default(),
            vec![colored_box(200.0, 200.0, Color::RED)],
        );

        let mut rt = Runtime::new(Size::new(100.0, 100.0));
        rt.submit_view_tree(root, false);
        let rendered = rt.frame(WindowId::dummy());

        assert!(
            rendered
                .iter()
                .all(|e| !matches!(e.element, VisualElement::Group { .. })),
            "non-clip container should not produce Group elements"
        );
    }

    #[test]
    fn scroll_view_clips_overflowing_content() {
        // 构造一个真实 ScrollView widget（高度 100），内含一个 200x200 的红色子盒，
        // 子盒在垂直方向溢出视口，应被 ScrollView 的 clip 裁剪掉。
        let sv = ScrollView::new(100.0).child(SolidBox);

        let mut ctx = BuildContext::empty();
        let view = sv.build(&mut ctx);

        let mut rt = Runtime::new(Size::new(100.0, 100.0));
        rt.submit_view_tree(view, false);
        let elements = rt.frame(WindowId::dummy());

        // 必须存在带 clip_rect 的 Group（ScrollView 通过 paint.clip_content 触发）
        let has_clip_group = elements.iter().any(|e| {
            matches!(
                &e.element,
                VisualElement::Group {
                    clip_rect: Some(_),
                    ..
                }
            )
        });
        assert!(has_clip_group, "ScrollView should produce a clipped Group");

        // 渲染并做像素级验证：渲染图 200x200 大于 ScrollView 视口(100x100)，
        // 这样裁剪边界 (x/y = 100) 落在图内，可区分「被裁」与「可见」。
        let mut renderer = crate::render::engine::VelloRenderer::new(200, 200);
        let pix = renderer.render(&elements);
        let data = pix.data();
        let w = pix.width() as usize;

        // 视口内（x=50, y=50，均 < 100）应看到红色子盒
        let inside = data[50 * w + 50];
        assert_eq!(
            inside,
            vello_cpu::color::PremulRgba8::from_u8_array([255, 0, 0, 255]),
            "viewport should show the scrolled content"
        );

        // 视口外（x=150 > 100）溢出部分必须被裁剪，保持默认背景(240,240,240)而非红色
        let outside = data[50 * w + 150];
        assert_ne!(
            outside,
            vello_cpu::color::PremulRgba8::from_u8_array([255, 0, 0, 255]),
            "overflowing content outside the viewport must be clipped"
        );
    }
}
