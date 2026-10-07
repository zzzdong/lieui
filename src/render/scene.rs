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

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;

use lieui_geom::{Color, Point, Rect, Size};
use lieui_text::{TextAlign, TextLayout, TextSpec};

use crate::track::{FocusState, Kind, NodeId, Track, Visibility};
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
    PushClip {
        rect: Rect,
        transform: Affine,
    },
    PopClip,
    /// 压入组不透明度（`PopOpacity` 成对）
    PushOpacity {
        opacity: f32,
    },
    PopOpacity,
}

impl Op {
    /// 该原语的**屏幕空间**包围盒（已含变换），用于调试与脏区核对
    pub fn screen_bounds(&self) -> Option<Rect> {
        match self {
            Op::Rect { rect, transform, .. }
            | Op::Shadow { rect, transform, .. }
            | Op::Border { rect, transform, .. }
            | Op::PushClip { rect, transform, .. } => Some(transform.bounding_box(*rect)),
            Op::Text {
                layout,
                origin,
                transform,
                ..
            } => {
                let r = Rect::new(origin.x, origin.y, layout.width(), layout.height());
                Some(transform.bounding_box(r))
            }
            Op::Image {
                image: _,
                rect,
                transform,
            } => Some(transform.bounding_box(*rect)),
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

/// 缓存条目数上限（超出则整表清空）。
///
/// **注意**：key 不含位置，只有"文本内容变化"才会新增条目，所以正常的
/// 滚动/动画**不会**推高这个数（原`TextCache` 的注释已指出这点）。
/// 触发场景是"同时存在 >2048 个互不相同的 (内容, 规格, 颜色)"，例如超长动态列表。
const TEXT_CACHE_MAX: usize = 2048;

/// 无分配的组合 hash —— 两段式 key 的第一段。
///
/// FNV-1a 逐字节混合：`(内容, 规格, 颜色)` → 64 位。
/// 冲突概率对缓存用途可忽略（冲突时退化为桶内线性比对，仍**正确**，只是多一次 memcmp）。
fn key_hash(text: &str, spec: u64, color: u32) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // 混入规格与颜色（用旋转乘法，避免 spec/color 集中在低位互相抵消）
    h ^= spec.rotate_left(17).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    h = h.wrapping_mul(0x0000_0100_0000_01b3);
    h ^= (color as u64).rotate_left(31).wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
    h
}

/// 缓存条目：排版结果 + 墨迹盒（仅 `spec.optical_align` 时计算并保留）
struct TextEntry {
    layout: Arc<TextLayout>,
    ink: Option<lieui_text::InkBounds>,
}

/// 文本排版缓存：`(内容, 规格, 颜色) → 排版 + 墨迹盒`
///
/// ## 两段式 key：为什么不是 `HashMap<TextKey, _>`
///
/// 直觉写法是 `HashMap<TextKey, TextEntry>`，其中 `TextKey.text: String`。
/// 但那样**命中路径也要分配** —— 每次查询都得先
/// `text.to_string()` 构造出key 才能查表，**外加一次字节拷贝**。
///
/// 缓存的基本前提恰恰是"命中足够便宜"。在软渲染里这个代价是**每帧每可见文本一次
/// 堆分配**（50 个可见文本 × 60fps ≈ 3000 次/秒），而缓存省下的是 parley 排版
/// —— 一个数量级更贵的操作。所以那点分配不是瓶颈，但它让"缓存"这个设计看起来
/// 像是没做完。
///
/// 改为 **`hash → 桶`**：先算无分配的 64 位 hash 定位桶，再在桶内用
/// `str == str`（memcmp，**零分配**）比对内容。命中路径全程不分配。
///
/// 桶通常只有 1 项（64 位 hash 冲突概率可忽略），所以线性比对不比哈希查找慢。
#[derive(Default)]
pub struct TextCache {
    /// `hash(内容, 规格, 颜色)` → 同 hash 的候选条目
    map: HashMap<u64, Vec<(TextKey, TextEntry)>>,
    /// **条目总数**（不是桶数）。手工维护以免每次查询都O(桶数) 求和 ——
    /// 那是 miss 路径上的 O(n²)（2048 次插入 × 2048 次求和）。
    count: usize,
}

impl TextCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 已缓存的**文本条目**数（不是桶数）
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.count = 0;
    }

    /// 取（或新建）文本排版。命中缓存时不做任何 parley 调用。
    pub fn get_or_build(&mut self, text: &str, spec: &TextSpec, color: Color) -> Arc<TextLayout> {
        self.get_full(text, spec, color).0
    }

    /// 同上，并返回是否命中缓存（`SceneStats` 的观测点）
    pub fn get_or_build_counted(&mut self, text: &str, spec: &TextSpec, color: Color) -> (Arc<TextLayout>, bool) {
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
        let spec_h = spec_hash(spec);
        let color_k = color_key(color);
        let h = key_hash(text, spec_h, color_k);

        // ── 命中路径：全程零堆分配 ──
        // `String == &str` 走 `memcmp`（`PartialEq<str> for String`），不分配。
        if let Some(bucket) = self.map.get_mut(&h) {
            for e in bucket.iter_mut() {
                if e.0.spec == spec_h && e.0.color == color_k && e.0.text == text {
                    // 命中时若这次需要墨迹而条目里没有（首次是非光学用法）⇒ 补算一次
                    if spec.optical_align && e.1.ink.is_none() {
                        e.1.ink = lieui_text::ink_bounds(&e.1.layout);
                    }
                    return (Arc::clone(&e.1.layout), e.1.ink, true);
                }
            }
        }

        // ── 未命中：排版（这里是真正贵的操作）──
        let layout = Arc::new(lieui_text::create_text_layout(text, spec, color));
        let ink = if spec.optical_align {
            lieui_text::ink_bounds(&layout)
        } else {
            None
        };
        // 防膨胀（正常 UI 远达不到上限）
        //
        // 用**条目总数**而不是桶数判定 —— 否则一次哈希碰撞就会让实际条目数
        // 悄悄超过上限，缓存无上限增长。
        if self.count >= TEXT_CACHE_MAX {
            self.map.clear();
            self.count = 0;
        }
        // 只有 miss 路径才分配 key（`text.to_owned()`）
        self.map.entry(h).or_default().push((
            TextKey {
                text: text.to_owned(),
                spec: spec_h,
                color: color_k,
            },
            TextEntry {
                layout: Arc::clone(&layout),
                ink,
            },
        ));
        self.count += 1;
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

/// 脏区 + **裁剪栈**（剔除用）。`all = true` 时不做脏区剔除。
///
/// ## 为什么裁剪状态放这里，而不是留在 `WalkCtx`
///
/// 裁剪栈需要在整个 `walk` 递归过程中**可变**（进入子节点时压入、
/// 返回时恢复），而它同时要被**逐原语**的剔除用到——那些站点在
/// `widgets::draw` 里，只拿到 `&Cull`。
///
/// 早期版本把 clip 放在 `walk` 的参数 `WalkCtx` 里，于是逐原语站点
/// 看不到它 ⇒ 只能判"与脏区相交"。**两份状态**（`WalkCtx.clip` 与
/// `Cull.clip`）会立刻产生"更新了一处忘了另一处"的不一致风险。
///
/// 所以**统一收敛到 `Cull`**：它本来就以 `&Cull` 传遍全程，
/// 加一个 `Cell<Option<Rect>>` 即可获得内部可变性（`Rect: Copy`），
/// **零签名变更**，且只有一个裁剪状态来源。
#[derive(Debug)]
pub(crate) struct Cull {
    rects: Vec<Rect>,
    all: bool,
    /// 当前路径上的祖先裁剪（**窗口坐标**）。`None` = 无裁剪。
    ///
    /// 用 `Cell` 而非 `&mut`：`Cull` 以 `&Cull` 传递（`draw` 内部还要用），
    /// 而 `walk` 需要在递归中改它。
    clip: Cell<Option<Rect>>,
}

impl Cull {
    pub(crate) fn new(damage: &[Rect], damage_all: bool, window: Size) -> Self {
        let (rects, all) = if damage_all || damage.is_empty() {
            (vec![Rect::new(0.0, 0.0, window.width, window.height)], true)
        } else {
            (damage.to_vec(), false)
        };
        Self {
            rects,
            all,
            clip: Cell::new(None),
        }
    }

    /// 包围盒是否与脏区相交（`all` 时恒为真）。
    ///
    /// ⚠️ **只判脏区，不判裁剪** —— 逐原语站点应该用 [`Self::hit_visible`]。
    /// 保留这个方法是因为**节点级**剔除已经先把 `screen` 交过一次叉
    /// （见 `walk` 里的 `visible`），再判一次会重复劳动。
    pub(crate) fn hit(&self, r: &Rect) -> bool {
        self.all || self.rects.iter().any(|d| d.intersects(r))
    }

    /// 包围盒是否**既与脏区相交、又未被祖先裁剪完全裁掉**。
    ///
    /// ★ 与 [`Self::hit`] 的区别就是"祖先裁剪"这一项。
    ///
    /// ## 为什么这一步是安全的（而"按容器 bbox 裁子树"不安全）
    ///
    /// `clip` 来自 `clip_content` / `overflow_scroll` / 显式 `n.clip`
    /// —— 它们都是**硬约束**：被裁掉的内容**确定不会显示**。
    /// 所以"原语完全落在 clip 外 ⇒ 不提交"不会丢任何东西。
    ///
    /// 相反，若按**容器自身 bbox** 去裁子树就是错的：不裁剪的容器
    /// **允许子节点溢出**（overflow visible，tooltip 定位、弹窗等），
    /// 按 bbox 裁掉会**丢失溢出内容**。实测（`render_cost_bench` 的
    /// `unclipped_container`）这类容器确实存在且 visited 线性增长，
    /// 但**没有正确的方法**在不加裁剪语义的前提下跳过它。
    pub(crate) fn hit_visible(&self, r: &Rect) -> bool {
        if !self.hit(r) {
            return false;
        }
        match self.clip.get() {
            // 无裁剪 ⇒ 脏区相交即可见
            None => true,
            Some(c) => c.intersects(r),
        }
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

/// 递归上下文：`walk` 的参数打包。
///
/// ## 为什么要打包
///
/// 加了"祖先裁剪"之后 `walk` 变成 8 个参数，`clippy::too_many_arguments` 直接报错
/// —— 这类门禁**是设计信号而非噪音**：参数列表失控通常意味着状态没归位。
///
/// 这里只保留**全程不变**的两项。
/// 随递归变化的裁剪栈**已收敛进 [`Cull`]**（`clip: Cell<Option<Rect>>`）——
/// 因为逐原语剔除也要用它，而那些站点只拿到 `&Cull`。
/// 两份状态（这里一份、`Cull` 一份）必然产生"更新一处忘另一处"的风险。
struct WalkCtx<'a> {
    cull: &'a Cull,
    opts: &'a SceneOptions,
}

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
    pub fn build(&mut self, track: &Track, opts: &SceneOptions, damage: &[Rect], damage_all: bool) -> Scene {
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

        // ★ z 序遍历（从下到上）：`z = (Layer, 嵌套深度, 声明序号)`。
        //   命中侧 `hit.rs` 遍历**同一序列的逆序** ⇒ 绘制顺序与命中顺序必然一致。
        let mut ctx = WalkCtx { cull: &cull, opts };
        for root in track.z_ordered_roots() {
            // 层遮罩（Modal 的 backdrop / 自定义）
            if let Some(color) = root.opts.backdrop {
                scene.push(Op::Rect {
                    rect: window_rect,
                    radius: 0.0,
                    color,
                    transform: Affine::IDENTITY,
                });
            }
            // ★ 层根的祖先裁剪 = **窗口本身**（不是无限大）。
            //   否则"溢出窗口的层根"会带着一个看似无限的裁剪区往下传，
            //   永远命中脏区 —— 逐原语剔除会因此完全失效。
            cull.clip.set(Some(window_rect));
            self.walk(track, root.node, Affine::IDENTITY, &mut ctx, &mut scene);
        }

        scene
    }

    fn walk(&mut self, track: &Track, id: NodeId, parent: Affine, ctx: &mut WalkCtx<'_>, out: &mut Scene) {
        let cull = ctx.cull;
        let opts = ctx.opts;
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

        // ★★降 overdraw 的关键：把**祖先裁剪**纳入 culling。
        //
        // 此前 `Cull` 只有窗口级脏区，不携带 clip 上下文，所以一个**被祖先裁剪矩形
        // 完全裁掉**的节点（例如长列表里滚出容器可视区的行）仍会通过 `cull.hit`
        // 的检查 ⇒ 它的原语被提交 ⇒ 光栅器要对这些像素做求交后**才丢弃** ——
        // 光栅化开销已经发生了，"被裁掉"只是最后一步白做。
        //
        // 在长列表上这是主要浪费来源：容器 bbox 很高、必然与脏区相交，
        // 于是**全部**子节点都会被遍历和提交，而实际可见的只有十几行。
        let visible = match cull.clip.get() {
            Some(pc) => screen.intersect(&pc),
            None => Some(screen),
        };
        let Some(visible) = visible else {
            // 自身 bbox 与祖先裁剪区无交集 ⇒ 整棵子树都不可能有可见像素
            out.stats.nodes_culled += 1;
            return;
        };

        // 带裁剪的节点：整体在脏区外 ⇒ 整棵子树都可跳过（子节点不可能画到裁剪区外）
        let clips_children = n.paint.clip_content || n.layout.overflow_scroll || n.clip.is_some();
        if clips_children && !cull.hit(&visible) {
            out.stats.nodes_culled += 1;
            return;
        }

        // 组不透明度
        let opacity = n.paint.opacity.clamp(0.0, 1.0);
        if opacity < 1.0 {
            out.push(Op::PushOpacity { opacity });
        }

        // 裁剪（滚动容器 / clip_content 用自身矩形；显式 clip 取其交集）
        let clip = match (if clips_children { Some(rect) } else { None }, n.clip) {
            (Some(a), Some(b)) => a.intersect(&b),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        // 传给子节点的裁剪 = 自身 clip ∩ 祖先 clip（**窗口坐标**）
        //
        // ★★ 这里有一个容易写错的点：**当前节点没有 clip 时必须 _继承_ 祖先 clip**。
        //   写成 `clip.and_then(...)` 会在"无 clip 的节点"处把 `child_clip` 变成 `None`，
        //   裁剪链就断了 —— 外层裁剪对更深层完全失效。
        //   实测症状：外层 100×100 裁剪区内放一个 400×400 的**无 clip** 容器，
        //   其子节点 culled = 0（一个都没挡住）。
        let child_clip: Option<Rect> = match clip {
            Some(c) => {
                let s = transform.bounding_box(c);
                match cull.clip.get() {
                    Some(pc) => match s.intersect(&pc) {
                        Some(x) => Some(x),
                        None => {
                            // 自身 clip 与祖先 clip 无交集 ⇒ 整棵子树都不可见
                            out.stats.nodes_culled += 1;
                            if opacity < 1.0 {
                                out.push(Op::PopOpacity);
                            }
                            return;
                        }
                    },
                    None => Some(s),
                }
            }
            // 无自身 clip ⇒ 沿用祖先裁剪（**不要**置 None）
            None => cull.clip.get(),
        };
        if let Some(c) = clip {
            if cull.hit(&visible) || cull.is_all() {
                out.push(Op::PushClip { rect: c, transform });
            } else {
                // 裁剪区本身在脏区外 ⇒ 子树不可能命中
                out.stats.nodes_culled += 1;
                if opacity < 1.0 {
                    out.push(Op::PopOpacity);
                }
                return;
            }
        }

        // ★ 压入裁剪栈，供**本节点自身**的逐原语剔除使用。
        //   必须在 `draw` 之前设置 —— `draw` 内部（`push_culled` 之类站点）
        //   只拿到 `&Cull`，裁剪状态就在这里。子节点会再压入更深的一层。
        //
        // ★★ `entry_clip` 必须在**压入之前**捕获，且**返回前恢复成它**。
        //   否则下一个兄弟节点会读到"上一个兄弟子树残留的裁剪"。
        //   实测症状：`scrolled_window` 里滚动容器 `sc` 与红箱 `red` 是兄弟，
        //   `sc` 返回后 `cull.clip` 仍停在 `sc` 的裁剪区（x < 100），
        //   于是 `red`（x ∈ [100,200]）被**整个裁掉** ⇒ 渲染成底色。
        let entry_clip = cull.clip.get();
        cull.clip.set(child_clip);

        // 节点自身内容（按 `Kind` 枚举分派，见 `widgets::draw`）
        crate::widgets::draw(&mut self.text, track, id, transform, cull, out, &opts.theme);

        // 焦点框（键盘聚焦才画 —— `FocusState` 的存在意义）；颜色走主题 token
        if opts.focus_ring && n.focus_state == FocusState::Keyboard && n.interaction.focused {
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
        // 子节点递归。
        for child in n.children.iter().copied() {
            self.walk(track, child, transform, ctx, out);
        }
        // 子节点各自会压入更深一层并可能留下残留 ⇒ 先归位到**本节点**的裁剪，
        // 让滚动条覆盖层看到正确的范围。
        cull.clip.set(child_clip);

        // 滚动条覆盖层：画在**子项之后**（否则被列表项盖住），仍在容器裁剪内
        crate::widgets::draw_scrollbar_overlay(out, cull, track, id, transform, &opts.theme);

        // ★ 离开本节点前把裁剪栈**恢复成进入时的样子**（与 `Op::PushClip`/`PopClip` 同构）。
        //   不恢复的话，调用方的**下一个兄弟**会拿本节点的裁剪区去算自己的 `child_clip`
        //   ⇒ 兄弟之间互相裁剪。实测症状见上方 `entry_clip` 处的注释。
        cull.clip.set(entry_clip);

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
#[path = "scene_tests.rs"]
mod tests;
