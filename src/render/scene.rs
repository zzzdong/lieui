//! 绘制列表（draw list）：从保留树**只读展开**成一串绘制原语。
//!
//! 这一层取代旧实现的 `cv()`（ViewNode → VisualElement 递归，280 行）与 `LayeredElement` 树：
//! - 旧实现产出**嵌套的 Group 树**，这里产出**扁平 op 列表**（`PushClip`/`PopClip` 显式成对），
//!   op 自带 `transform`（已按祖先链组合好）⇒ 光栅层只需顺序执行、无需理解树结构；
//! - **脏区剔除**：不落在脏区里的原语不提交（节点带裁剪时整棵子树可跳过）；
//! - **视图态在这里解析**：hover / pressed / disabled 的配色在展开时决定，
//!   所以"鼠标划过"只需重绘、不需要重跑 `view()`（这正是 R1 响应式的前提）。
//!
//! 文本排版（parley）按 `(内容, 规格, 颜色)` 缓存，由 [`TextCache`] 持有。

use std::collections::HashMap;
use std::sync::Arc;

use lieui_geom::{Color, Point, Rect, Size};
use lieui_text::{TextAlign, TextLayout, TextSpec};

use crate::track::{FocusState, Kind, Layer, NodeId, Track, Visibility};
use crate::transform::Affine;

// ───────────────────────── 绘制原语 ─────────────────────────

/// 一个绘制原语。`transform` 已经把祖先链的组合算好（渲染时直接 `set_transform`）。
#[derive(Clone, Debug)]
pub enum Op {
    /// 实心矩形（含圆角）
    Rect {
        rect: Rect,
        radius: f32,
        color: Color,
        transform: Affine,
    },
    /// 投影（高斯模糊圆角矩形）
    Shadow {
        rect: Rect,
        radius: f32,
        std_dev: f32,
        color: Color,
        transform: Affine,
    },
    /// 边框
    Border {
        rect: Rect,
        radius: f32,
        width: f32,
        color: Color,
        transform: Affine,
    },
    /// 文本（已排版好的 glyph run）
    Text {
        layout: Arc<TextLayout>,
        origin: Point,
        color: Color,
        transform: Affine,
    },
    /// 位图（RGBA8，等比缩放进 rect；光栅层手动 blit）
    Image {
        image: std::sync::Arc<crate::track::ImageData>,
        rect: Rect,
        transform: Affine,
    },
    /// 压入裁剪矩形（`PopClip` 成对）
    PushClip { rect: Rect, transform: Affine },
    PopClip,
    /// 压入组不透明度（`PopOpacity` 成对）
    PushOpacity { opacity: f32 },
    PopOpacity,
}

impl Op {
    /// 该原语的**屏幕空间**包围盒（已含变换），用于调试与脏区核对
    pub fn screen_bounds(&self) -> Option<Rect> {
        match self {
            Op::Rect {
                rect, transform, ..
            }
            | Op::Shadow {
                rect, transform, ..
            }
            | Op::Border {
                rect, transform, ..
            }
            | Op::PushClip {
                rect, transform, ..
            } => Some(transform.bounding_box(*rect)),
            Op::Text {
                layout,
                origin,
                transform,
                ..
            } => {
                let r = Rect::new(origin.x, origin.y, layout.width(), layout.height());
                Some(transform.bounding_box(r))
            }
            Op::Image { image: _, rect, transform } => Some(transform.bounding_box(*rect)),
            _ => None,
        }
    }

    pub fn is_clip_push(&self) -> bool {
        matches!(self, Op::PushClip { .. })
    }

    pub fn is_clip_pop(&self) -> bool {
        matches!(self, Op::PopClip)
    }
}

// ───────────────────────── 绘制列表 ─────────────────────────

/// 展开统计（测试与调试的观测点）
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SceneStats {
    pub nodes_visited: usize,
    /// 因"带裁剪且整体在脏区外"而跳过的子树数
    pub nodes_culled: usize,
    /// 因不带裁剪但落点不在脏区而**不提交**的原语数
    pub prims_culled: usize,
    pub ops: usize,
    pub text_layouts_built: usize,
    pub text_cache_hits: usize,
}

/// 绘制列表
#[derive(Default)]
pub struct Scene {
    ops: Vec<Op>,
    pub stats: SceneStats,
}

impl Scene {
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub(crate) fn push(&mut self, op: Op) {
        self.ops.push(op);
        self.stats.ops = self.ops.len();
    }

    /// 裁剪层是否成对（渲染前的自检；不配对会在 vello 内部 assert）
    pub fn clips_are_balanced(&self) -> bool {
        let mut depth = 0i32;
        let mut opacity = 0i32;
        for op in &self.ops {
            match op {
                Op::PushClip { .. } => depth += 1,
                Op::PopClip => depth -= 1,
                Op::PushOpacity { .. } => opacity += 1,
                Op::PopOpacity => opacity -= 1,
                _ => {}
            }
            if depth < 0 || opacity < 0 {
                return false;
            }
        }
        depth == 0 && opacity == 0
    }
}

impl std::fmt::Debug for Scene {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scene")
            .field("ops", &self.ops.len())
            .field("stats", &self.stats)
            .finish()
    }
}

// ───────────────────────── 文本排版缓存 ─────────────────────────

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct TextKey {
    text: String,
    spec: u64,
    color: u32,
}

/// 缓存条目：排版结果 + 墨迹盒（仅 `spec.optical_align` 时计算并保留）
struct TextEntry {
    layout: Arc<TextLayout>,
    ink: Option<lieui_text::InkBounds>,
}

/// 文本排版缓存：`(内容, 规格, 颜色) → 排版 + 墨迹盒`
#[derive(Default)]
pub struct TextCache {
    map: HashMap<TextKey, TextEntry>,
}

impl TextCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// 取（或新建）文本排版。命中缓存时不做任何 parley 调用。
    pub fn get_or_build(&mut self, text: &str, spec: &TextSpec, color: Color) -> Arc<TextLayout> {
        self.get_full(text, spec, color).0
    }

    /// 同上，并返回是否命中缓存（`SceneStats` 的观测点）
    pub fn get_or_build_counted(
        &mut self,
        text: &str,
        spec: &TextSpec,
        color: Color,
    ) -> (Arc<TextLayout>, bool) {
        let (l, _ink, hit) = self.get_full(text, spec, color);
        (l, hit)
    }

    /// 排版 + 墨迹盒 + 是否命中缓存。
    ///
    /// 墨迹盒只在 `spec.optical_align` 时计算（其余场景多算一次字形 bbox 是纯浪费），
    /// 且与排版一起缓存 ⇒ 光学对齐的**每帧成本为零**（布局时算过一次就复用）。
    pub fn get_full(
        &mut self,
        text: &str,
        spec: &TextSpec,
        color: Color,
    ) -> (Arc<TextLayout>, Option<lieui_text::InkBounds>, bool) {
        let key = TextKey {
            text: text.to_string(),
            spec: spec_hash(spec),
            color: color_key(color),
        };
        if let Some(entry) = self.map.get_mut(&key) {
            // 命中时若这次需要墨迹而条目里没有（首次是非光学用法）⇒ 补算一次
            if spec.optical_align && entry.ink.is_none() {
                entry.ink = lieui_text::ink_bounds(&entry.layout);
            }
            return (Arc::clone(&entry.layout), entry.ink, true);
        }
        let layout = Arc::new(lieui_text::create_text_layout(text, spec, color));
        let ink = if spec.optical_align {
            lieui_text::ink_bounds(&layout)
        } else {
            None
        };
        // 简单防膨胀（正常 UI 远达不到上限）
        if self.map.len() >= 2048 {
            self.map.clear();
        }
        self.map.insert(
            key,
            TextEntry {
                layout: Arc::clone(&layout),
                ink,
            },
        );
        (layout, ink, false)
    }
}

fn color_key(c: Color) -> u32 {
    (c.r as u32) << 24 | (c.g as u32) << 16 | (c.b as u32) << 8 | c.a as u32
}

/// 规格哈希（只用影响排版结果的字段）
fn spec_hash(s: &TextSpec) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    mix(s.font_size.to_bits());
    mix(match s.font_weight {
        lieui_text::FontWeight::Normal => 400,
        lieui_text::FontWeight::Medium => 500,
        lieui_text::FontWeight::Bold => 700,
        lieui_text::FontWeight::Weight(w) => w as u64,
    });
    mix(s.wrap as u64);
    mix(s.max_width.map(|w| w.to_bits()).unwrap_or(u64::MAX));
    mix(s.line_height.map(|v| v.to_bits()).unwrap_or(u64::MAX));
    mix(match s.text_align {
        TextAlign::Start => 0,
        TextAlign::Center => 1,
        TextAlign::End => 2,
        TextAlign::Justify => 3,
    });
    for b in s.font_family.as_bytes() {
        mix(*b as u64);
    }
    h
}

// ───────────────────────── 脏区剔除 ─────────────────────────

/// 脏区（剔除用）。`all = true` 时不做剔除。
#[derive(Clone, Debug)]
pub(crate) struct Cull {
    rects: Vec<Rect>,
    all: bool,
}

impl Cull {
    pub(crate) fn new(damage: &[Rect], damage_all: bool, window: Size) -> Self {
        if damage_all || damage.is_empty() {
            Self {
                rects: vec![Rect::new(0.0, 0.0, window.width, window.height)],
                all: true,
            }
        } else {
            Self {
                rects: damage.to_vec(),
                all: false,
            }
        }
    }

    /// 包围盒是否与脏区相交（`all` 时恒为真）
    pub(crate) fn hit(&self, r: &Rect) -> bool {
        self.all || self.rects.iter().any(|d| d.intersects(r))
    }

    pub(crate) fn is_all(&self) -> bool {
        self.all
    }
}

// ───────────────────────── 展开 ─────────────────────────

/// 场景级选项
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneOptions {
    pub window: Size,
    /// 窗口底色（必须**不透明**：softbuffer 无 alpha 通道）
    pub background: Color,
    /// 是否绘制焦点框（键盘聚焦才画，≈ WinUI `FocusVisualKind`）
    pub focus_ring: bool,
    /// 主题快照：绘制期颜色（光标/选区/滚动条/accent 兜底）的来源
    pub theme: crate::theme::Theme,
}

impl Default for SceneOptions {
    fn default() -> Self {
        Self {
            window: Size::new(800.0, 600.0),
            background: Color::new(240, 240, 240),
            focus_ring: true,
            theme: crate::theme::Theme::default(),
        }
    }
}

/// 层序：**从下到上**（与 `Layer` 枚举序一致）
const LAYER_BOTTOM_UP: [Layer; 6] = [
    Layer::Content,
    Layer::Overlay,
    Layer::Popup,
    Layer::Tooltip,
    Layer::Modal,
    Layer::DragPreview,
];

/// 展开器：持有跨帧的文本排版缓存
#[derive(Default)]
pub struct SceneBuilder {
    text: TextCache,
}

impl SceneBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text_cache(&self) -> &TextCache {
        &self.text
    }

    pub fn clear_caches(&mut self) {
        self.text.clear();
    }

    /// 展开成绘制列表。
    ///
    /// `damage` / `damage_all` 来自 `FrameStats`（M2 起就有的脏区），用于剔除不在脏区里的原语。
    pub fn build(
        &mut self,
        track: &Track,
        opts: &SceneOptions,
        damage: &[Rect],
        damage_all: bool,
    ) -> Scene {
        let cull = Cull::new(damage, damage_all, opts.window);
        let mut scene = Scene::default();
        let window_rect = Rect::new(0.0, 0.0, opts.window.width, opts.window.height);

        // 底色（光栅层也会按行带铺一次；这里放在列表里让 draw list 自描述）
        scene.push(Op::Rect {
            rect: window_rect,
            radius: 0.0,
            color: opts.background,
            transform: Affine::IDENTITY,
        });

        for layer in LAYER_BOTTOM_UP {
            for root in track.roots_of(layer) {
                // 层遮罩（Modal 的 backdrop / 自定义）
                if let Some(color) = root.opts.backdrop {
                    scene.push(Op::Rect {
                        rect: window_rect,
                        radius: 0.0,
                        color,
                        transform: Affine::IDENTITY,
                    });
                }
                self.walk(track, root.node, Affine::IDENTITY, &cull, opts, &mut scene);
            }
        }

        scene
    }

    fn walk(
        &mut self,
        track: &Track,
        id: NodeId,
        parent: Affine,
        cull: &Cull,
        opts: &SceneOptions,
        out: &mut Scene,
    ) {
        let Some(n) = track.get(id) else {
            return;
        };
        out.stats.nodes_visited += 1;
        // 不绘制的东西直接跳过（Collapsed 连布局都不参与；Hidden 占位但不画）
        if n.visibility != Visibility::Visible {
            return;
        }

        let rect = n.rect();
        let transform = parent.then(n.transform.matrix(rect));
        let screen = transform.bounding_box(rect);

        // 带裁剪的节点：整体在脏区外 ⇒ 整棵子树都可跳过（子节点不可能画到裁剪区外）
        let clips_children = n.paint.clip_content || n.layout.overflow_scroll || n.clip.is_some();
        if clips_children && !cull.hit(&screen) {
            out.stats.nodes_culled += 1;
            return;
        }

        // 组不透明度
        let opacity = n.paint.opacity.clamp(0.0, 1.0);
        if opacity < 1.0 {
            out.push(Op::PushOpacity { opacity });
        }

        // 裁剪（滚动容器 / clip_content 用自身矩形；显式 clip 取其交集）
        let clip = match (
            if clips_children { Some(rect) } else { None },
            n.clip,
        ) {
            (Some(a), Some(b)) => a.intersect(&b),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        if let Some(c) = clip {
            if cull.hit(&transform.bounding_box(c)) || cull.is_all() {
                out.push(Op::PushClip {
                    rect: c,
                    transform,
                });
            } else {
                // 裁剪区本身在脏区外 ⇒ 子树不可能命中
                out.stats.nodes_culled += 1;
                if opacity < 1.0 {
                    out.push(Op::PopOpacity);
                }
                return;
            }
        }

        // 节点自身内容（按 `Kind` 枚举分派，见 `widgets::draw`）
        crate::widgets::draw(&mut self.text, track, id, transform, cull, out, &opts.theme);

        // 焦点框（键盘聚焦才画 —— `FocusState` 的存在意义）；颜色走主题 token
        if opts.focus_ring
            && n.focus_state == FocusState::Keyboard
            && n.interaction.focused
        {
            let r = rect.inflate(-1.0);
            if r.width > 2.0 && r.height > 2.0 {
                out.push(Op::Border {
                    rect: r,
                    radius: n.paint.border_radius,
                    width: 2.0,
                    color: opts.theme.focus_ring,
                    transform,
                });
            }
        }

        // 子节点（按树序 = 绘制序，后者在上）
        for child in n.children.iter().copied() {
            self.walk(track, child, transform, cull, opts, out);
        }

        // 滚动条覆盖层：画在**子项之后**（否则被列表项盖住），仍在容器裁剪内
        crate::widgets::draw_scrollbar_overlay(out, cull, track, id, transform, &opts.theme);

        if let Some(_c) = clip {
            out.push(Op::PopClip);
        }
        if opacity < 1.0 {
            out.push(Op::PopOpacity);
        }
    }
}

/// 便捷：`Kind` 的名字（调试 / 测试用）
pub fn kind_name(k: &Kind) -> &'static str {
    match k {
        Kind::Box => "Box",
        Kind::Text(_) => "Text",
        Kind::Image(_) => "Image",
        Kind::Button { .. } => "Button",
        Kind::Checkbox { .. } => "Checkbox",
        Kind::Slider { .. } => "Slider",
        Kind::Progress { .. } => "Progress",
        Kind::Input { .. } => "Input",
        Kind::Switch { .. } => "Switch",
        Kind::Radio { .. } => "Radio",
        Kind::Custom(_) => "Custom",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout;
    use crate::style::PaintStyle;
    use crate::track::{Flags, Layer, LayerOpts};

    fn scene_opts(w: f32, h: f32) -> SceneOptions {
        SceneOptions {
            window: Size::new(w, h),
            background: Color::new(255, 255, 255),
            focus_ring: true,
            theme: crate::theme::Theme::light(),
        }
    }

    /// 内容根 + 一个 100×100 的红块
    fn setup() -> (Track, NodeId, NodeId) {
        let mut t = Track::new();
        let root = t.create(Kind::Box, None);
        t.add_root(Layer::Content, None, root);
        let block = t.create(Kind::Box, None);
        {
            let n = t.get_mut(block).unwrap();
            n.layout.dim = [100.0, 100.0];
            n.paint = PaintStyle::new().background(Color::RED);
        }
        t.append_child(root, block);
        layout(&mut t, Size::new(300.0, 300.0));
        (t, root, block)
    }

    fn rects(scene: &Scene) -> Vec<Rect> {
        scene
            .ops()
            .iter()
            .filter_map(|op| match op {
                Op::Rect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn scene_starts_with_the_window_background_and_is_balanced() {
        let (t, _, _) = setup();
        let mut b = SceneBuilder::new();
        let scene = b.build(&t, &scene_opts(300.0, 300.0), &[], true);

        assert!(matches!(
            scene.ops()[0],
            Op::Rect { rect, .. } if rect == Rect::new(0.0, 0.0, 300.0, 300.0)
        ));
        assert!(scene.clips_are_balanced());
        // 底色 + 红块
        let rs = rects(&scene);
        assert_eq!(rs.len(), 2);
        assert_eq!(rs[1], Rect::new(0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn hover_and_pressed_colors_are_resolved_at_expand_time() {
        let (mut t, _, block) = setup();
        {
            let n = t.get_mut(block).unwrap();
            n.paint = PaintStyle::new()
                .background(Color::RED)
                .hover_background(Color::GREEN)
                .pressed_background(Color::BLUE);
        }
        let mut b = SceneBuilder::new();
        let opts = scene_opts(300.0, 300.0);

        let color_of_block = |scene: &Scene| {
            scene
                .ops()
                .iter()
                .find_map(|op| match op {
                    Op::Rect { rect, color, .. } if rect.width == 100.0 => Some(*color),
                    _ => None,
                })
                .unwrap()
        };

        assert_eq!(color_of_block(&b.build(&t, &opts, &[], true)), Color::RED);

        t.set_pointer_over(block, true);
        assert_eq!(color_of_block(&b.build(&t, &opts, &[], true)), Color::GREEN);

        t.set_pressed(block, true);
        assert_eq!(color_of_block(&b.build(&t, &opts, &[], true)), Color::BLUE);
    }

    #[test]
    fn collapsed_and_hidden_nodes_are_not_drawn() {
        let (mut t, _, block) = setup();
        let mut b = SceneBuilder::new();
        let opts = scene_opts(300.0, 300.0);

        t.get_mut(block).unwrap().visibility = Visibility::Hidden;
        assert_eq!(rects(&b.build(&t, &opts, &[], true)).len(), 1, "只剩底色");

        t.get_mut(block).unwrap().visibility = Visibility::Collapsed;
        assert_eq!(rects(&b.build(&t, &opts, &[], true)).len(), 1);
    }

    #[test]
    fn scroll_container_pushes_a_clip() {
        let (mut t, root, _) = setup();
        let scroller = t.create(Kind::Box, None);
        {
            let n = t.get_mut(scroller).unwrap();
            n.layout.dim = [80.0, 60.0];
            n.layout.overflow_scroll = true;
        }
        t.append_child(root, scroller);
        layout(&mut t, Size::new(300.0, 300.0));
        let expect = crate::layout::rect_of(&t, scroller);

        let mut b = SceneBuilder::new();
        let scene = b.build(&t, &scene_opts(300.0, 300.0), &[], true);
        assert!(scene.clips_are_balanced());
        assert!(
            scene
                .ops()
                .iter()
                .any(|op| matches!(op, Op::PushClip { rect, .. } if *rect == expect)),
            "滚动容器应压入自身矩形的裁剪"
        );
    }

    #[test]
    fn opacity_pushes_a_layer() {
        let (mut t, _, block) = setup();
        t.get_mut(block).unwrap().paint.opacity = 0.5;
        let mut b = SceneBuilder::new();
        let scene = b.build(&t, &scene_opts(300.0, 300.0), &[], true);

        assert!(scene.clips_are_balanced());
        assert!(scene.ops().iter().any(|op| matches!(
            op,
            Op::PushOpacity { opacity } if (*opacity - 0.5).abs() < 1e-6
        )));
    }

    #[test]
    fn damage_culls_ops_outside_the_region() {
        let (t, _, _) = setup();
        let mut b = SceneBuilder::new();
        let opts = scene_opts(300.0, 300.0);

        // 脏区在右下角：100×100 的红块（左上角）不该被提交
        let damage = [Rect::new(200.0, 200.0, 50.0, 50.0)];
        let scene = b.build(&t, &opts, &damage, false);
        assert_eq!(rects(&scene).len(), 1, "只剩底色");
        assert!(scene.ops().len() < 2, "红块被剔除：{scene:?}");
    }

    #[test]
    fn damage_culls_a_whole_clipped_subtree() {
        let (mut t, root, _) = setup();
        // 一个带 clip 的容器（在右下角外）
        let clipped = t.create(Kind::Box, None);
        {
            let n = t.get_mut(clipped).unwrap();
            n.layout.dim = [20.0, 20.0];
            n.paint.clip_content = true;
        }
        let inner = t.create(Kind::Box, None);
        t.get_mut(inner).unwrap().paint = PaintStyle::new().background(Color::BLUE);
        t.append_child(clipped, inner);
        t.append_child(root, clipped);
        layout(&mut t, Size::new(300.0, 300.0));

        let mut b = SceneBuilder::new();
        // 脏区在左上角；clipped 容器在 (0,100) 下面一行（因为红块占了第一行）
        let damage = [Rect::new(0.0, 0.0, 50.0, 50.0)];
        let scene = b.build(&t, &scene_opts(300.0, 300.0), &damage, false);

        assert!(scene.stats.nodes_culled >= 1, "带裁剪的子树应整体跳过");
        assert!(!scene.ops().iter().any(|op| matches!(
            op,
            Op::Rect { color, .. } if *color == Color::BLUE
        )));
    }

    #[test]
    fn layers_are_drawn_bottom_up() {
        let (mut t, _, _) = setup();
        let modal = t.create(Kind::Box, None);
        t.get_mut(modal).unwrap().paint = PaintStyle::new().background(Color::BLUE);
        t.add_root(Layer::Modal, None, modal);
        layout(&mut t, Size::new(300.0, 300.0));

        let mut b = SceneBuilder::new();
        let scene = b.build(&t, &scene_opts(300.0, 300.0), &[], true);

        // backdrop（Modal 默认有不透明遮罩）→ Modal 内容 → 最后才是它
        let blue_pos = scene
            .ops()
            .iter()
            .position(|op| matches!(op, Op::Rect { color, .. } if *color == Color::BLUE))
            .unwrap();
        let backdrop_pos = scene
            .ops()
            .iter()
            .position(|op| matches!(op, Op::Rect { color, .. }
                if *color == LayerOpts::for_layer(Layer::Modal).backdrop.unwrap()))
            .unwrap();
        assert!(backdrop_pos < blue_pos, "遮罩在 Modal 内容之下");
    }

    #[test]
    fn transform_is_baked_into_ops() {
        let (mut t, _, block) = setup();
        t.get_mut(block).unwrap().transform.translate = (50.0, 25.0);
        let mut b = SceneBuilder::new();
        let scene = b.build(&t, &scene_opts(300.0, 300.0), &[], true);

        let op = scene
            .ops()
            .iter()
            .find(|op| matches!(op, Op::Rect { rect, .. } if rect.width == 100.0))
            .unwrap();
        let b = op.screen_bounds().unwrap();
        assert_eq!(b, Rect::new(50.0, 25.0, 100.0, 100.0));
    }

    #[test]
    fn focus_ring_only_for_keyboard_focus() {
        let (mut t, _, block) = setup();
        t.get_mut(block).unwrap().tab_stop = true;
        let mut b = SceneBuilder::new();
        let opts = scene_opts(300.0, 300.0);

        let border_count = |scene: &Scene| {
            scene
                .ops()
                .iter()
                .filter(|op| matches!(op, Op::Border { .. }))
                .count()
        };

        assert_eq!(border_count(&b.build(&t, &opts, &[], true)), 0);
        crate::focus::set_focus(&mut t, Some(block), FocusState::Pointer);
        assert_eq!(border_count(&b.build(&t, &opts, &[], true)), 0, "指针焦点不画");
        crate::focus::set_focus(&mut t, Some(block), FocusState::Keyboard);
        assert_eq!(border_count(&b.build(&t, &opts, &[], true)), 1, "键盘焦点画");
    }

    #[test]
    fn text_cache_avoids_rebuilding_the_layout() {
        let mut cache = TextCache::new();
        let spec = TextSpec {
            font_size: 16.0,
            ..Default::default()
        };
        let a = cache.get_or_build("hello", &spec, Color::BLACK);
        let b = cache.get_or_build("hello", &spec, Color::BLACK);
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(cache.len(), 1);

        // 颜色不同 ⇒ 需要重新排版（画笔色在整形时写入）
        let c = cache.get_or_build("hello", &spec, Color::RED);
        assert!(!Arc::ptr_eq(&a, &c));
        assert_eq!(cache.len(), 2);

        // 规格不同 ⇒ 重新排版
        let other = TextSpec {
            font_size: 24.0,
            ..Default::default()
        };
        let _ = cache.get_or_build("hello", &other, Color::BLACK);
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn builder_reuses_the_text_cache_across_frames() {
        let (mut t, root, _) = setup();
        let text = t.create(Kind::Text("hi".into()), None);
        t.append_child(root, text);
        layout(&mut t, Size::new(300.0, 300.0));

        let mut b = SceneBuilder::new();
        let opts = scene_opts(300.0, 300.0);

        let s1 = b.build(&t, &opts, &[], true);
        assert_eq!(s1.stats.text_layouts_built, 1, "首帧新建排版");
        assert_eq!(s1.stats.text_cache_hits, 0);
        assert!(s1.ops().iter().any(|op| matches!(op, Op::Text { .. })));

        let s2 = b.build(&t, &opts, &[], true);
        assert_eq!(s2.stats.text_layouts_built, 0, "第二帧命中缓存");
        assert_eq!(s2.stats.text_cache_hits, 1);
        assert_eq!(b.text_cache().len(), 1);
    }

    #[test]
    fn cull_helper_respects_bounds() {
        let cull = Cull::new(
            &[Rect::new(10.0, 10.0, 10.0, 10.0)],
            false,
            Size::new(100.0, 100.0),
        );
        assert!(cull.hit(&Rect::new(15.0, 15.0, 1.0, 1.0)));
        assert!(!cull.hit(&Rect::new(50.0, 50.0, 1.0, 1.0)));

        let all = Cull::new(&[], false, Size::new(100.0, 100.0));
        assert!(all.hit(&Rect::new(50.0, 50.0, 1.0, 1.0)), "空脏区视为整窗");
        assert!(all.is_all());
    }

    #[test]
    fn kind_names_are_stable_for_debugging() {
        assert_eq!(kind_name(&Kind::Box), "Box");
        assert_eq!(kind_name(&Kind::Text("x".into())), "Text");
        assert_eq!(kind_name(&Kind::Slider { value: 0.0, min: 0.0, max: 1.0, dragging: false }), "Slider");
        let _ = (Flags::PAINT_DIRTY, Layer::Content);
    }
}
