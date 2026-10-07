//! 保留树（`Track`）：arena + `NodeId`，跨帧存活，持有**视图态**与**布局结果**。
//!
//! 设计要点（`docs/architecture-v3.md` §3.2 / §3.4 / §3.15）：
//! - 所有节点访问都经 `&mut Track`，节点之间只靠 `NodeId`（`Copy`）互指 ⇒ 无自引用、无 `Rc` 环；
//! - `Kind` 只放**框架自有组件**的数据；`KindDesc` 是 `view()` 产出的"数据"，对齐时只覆盖 desc 组，
//!   `state` 组（如 `Slider.dragging`）由事件/框架改写 ⇒ hover 不丢、滚动不跳、输入框不闪回；
//! - 层（Modal / Overlay / Popup / Tooltip / DragPreview）是**多根**，`Root.owner` 记录它在哪个层里声明
//!   ⇒ 父层消失时整棵子树（含嵌套子层）一起销毁，不需要父子句柄图；
//! - `damage` 只累积"要被重绘的矩形"，由 `align` / 事件阶段登记，渲染阶段消费（M3 接脏区上屏）。

use std::sync::Arc;

use lieui_geom::{Color, Point, Rect, Size};
use lieui_layout::{ComputedLayout, FlexStyle};
use lieui_text::TextEngine;

use crate::event::{HandlerSlot, PointerButton, PointerId};
use crate::style::{ImageStyle, PaintStyle, TextStyle};

// ───────────────────────── 句柄与位标志 ─────────────────────────

/// 节点身份：低 32 位 index，高 32 位 generation。
///
/// `Copy + 'static`，可以在用户态之外的任何地方自由存储/传递；节点回收后旧句柄自然失效。
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct NodeId(u64);

impl NodeId {
    /// 空句柄：永不匹配任何存活节点
    pub const NULL: NodeId = NodeId(u64::MAX);

    pub const fn new(index: u32, generation: u32) -> Self {
        Self(((generation as u64) << 32) | index as u64)
    }

    // 手写而非 derive：derive 会给出 `NodeId(0)`（index 0 / gen 0），那是一个**合法**节点身份，
    // 用来表达"没有节点"会埋雷。默认值必须是 NULL。
    pub const fn null() -> Self {
        Self::NULL
    }

    pub const fn index(self) -> u32 {
        self.0 as u32
    }

    pub const fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }

    pub const fn to_u64(self) -> u64 {
        self.0
    }

    pub const fn is_null(self) -> bool {
        self.0 == u64::MAX
    }
}

impl Default for NodeId {
    /// 默认是 [`NodeId::NULL`]，**不是** `NodeId(0)`（后者是合法身份）
    fn default() -> Self {
        Self::NULL
    }
}

/// 节点脏标志（手写位集）
#[derive(Copy, Clone, PartialEq, Eq, Default, Debug)]
pub struct Flags(u16);

impl Flags {
    pub const EMPTY: Self = Self(0);
    /// 固有尺寸可能变了 → 需要 measure
    pub const MEASURE_DIRTY: Self = Self(1);
    /// 位置/尺寸可能变了 → 需要 arrange
    pub const ARRANGE_DIRTY: Self = Self(1 << 1);
    /// 需要重绘（但不需要重排）
    pub const PAINT_DIRTY: Self = Self(1 << 2);
    /// 已挂载到树上
    pub const ATTACHED: Self = Self(1 << 3);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}

impl std::ops::BitOr for Flags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Flags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.insert(rhs);
    }
}

// ───────────────────────── 可视 / 交互属性 ─────────────────────────

/// 可见性（≈ WinUI `Visibility`）
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Visibility {
    #[default]
    Visible,
    /// 不绘制，但**占位**（参与布局）
    Hidden,
    /// 不绘制也不参与布局
    Collapsed,
}

/// 焦点来源（≈ WinUI `FocusState`）：决定是否画 focus ring
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum FocusState {
    #[default]
    Unfocused,
    Pointer,
    Keyboard,
    Programmatic,
}

/// 每节点的交互视图态（≈ `Control.IsPointerOver / IsPressed`）
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct InteractionState {
    pub pointer_over: bool,
    pub pressed: bool,
    pub focused: bool,
    /// ⚠️ **运行期修改请用 [`Track::set_interaction_enabled`]**，直接写字段会漏掉
    /// `mark_paint_dirty` ⇒ 禁用态要等下一次别的改动才会重绘。
    /// （声明式路径 `view()` 里的 `.enabled(..)` 除外：那是构建，下一帧 `align` 会统一处理。）
    pub enabled: bool,
}

impl Default for InteractionState {
    fn default() -> Self {
        Self {
            pointer_over: false,
            pressed: false,
            focused: false,
            enabled: true,
        }
    }
}

/// 节点变换（≈ `RenderTransform` + `CenterPoint`）
///
/// 只存参数；矩阵求逆/命中在 M2 做。`origin` 是**归一化**原点（0.5,0.5 = 中心）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Transform {
    pub translate: (f32, f32),
    pub scale: (f32, f32),
    /// 顺时针角度
    pub rotation_deg: f32,
    pub origin: (f32, f32),
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translate: (0.0, 0.0),
            scale: (1.0, 1.0),
            rotation_deg: 0.0,
            origin: (0.5, 0.5),
        }
    }
}

impl Transform {
    pub fn is_identity(&self) -> bool {
        self.translate == (0.0, 0.0) && self.scale == (1.0, 1.0) && self.rotation_deg == 0.0
    }
}

// ───────────────────────── 组件（Kind）─────────────────────────

/// 图片数据（M3 渲染消费）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    /// RGBA8
    pub rgba: Vec<u8>,
}

/// 滚动轴
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Axis {
    X,
    #[default]
    Y,
    Both,
}

/// 组件变体标签（对齐时的"类型是否相同"判定）
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum KindTag {
    Box,
    Text,
    Image,
    Button,
    Checkbox,
    Slider,
    Progress,
    Input,
    Switch,
    Radio,
    Custom,
}

/// 双向绑定槽（≈ Vue 的 `v-model` / WinUI 的 `TwoWay` 绑定）。
///
/// **只有存在绑定的节点才会有"改模型"的内置行为**（见 `widgets::handle`）——
/// 这正是 `v-model` 的语义：`checkbox(checked)` 只是"显示这个值"，
/// `checkbox_bound(sig)` 才是"拖动/点击后写回这个 signal"。
#[derive(Default, Clone)]
pub struct Bindings {
    /// 复选框/开关
    pub checked: Option<crate::reactive::Signal<bool>>,
    /// 滑块/数值输入
    pub value: Option<crate::reactive::Signal<f32>>,
    /// 文本输入
    pub text: Option<crate::reactive::Signal<String>>,
}

impl Bindings {
    pub fn any(&self) -> bool {
        self.checked.is_some() || self.value.is_some() || self.text.is_some()
    }
}

impl std::fmt::Debug for Bindings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bindings")
            .field("checked", &self.checked.is_some())
            .field("value", &self.value.is_some())
            .field("text", &self.text.is_some())
            .finish()
    }
}

/// `view()` 产出的**数据**（不含视图态）。
///
/// 对齐时只把 desc 应用到 `Kind` 的 desc 组字段；`state` 组（如 `Slider.dragging`）保持不动。
#[derive(Clone, PartialEq, Debug)]
pub enum KindDesc {
    Box,
    Text(String),
    Image(Arc<ImageData>),
    Button {
        label: String,
    },
    Checkbox {
        checked: bool,
    },
    /// `min`/`max` 属于**数据**（决定拖拽映射），`dragging` 属于视图态
    Slider {
        value: f32,
        min: f32,
        max: f32,
    },
    Progress {
        value: f32,
    },
    /// 文本输入：`text` 是模型给出的值（编辑后经绑定写回），`placeholder` 是空文本提示
    Input {
        text: String,
        placeholder: String,
    },
    /// 开关（视觉与交互都不同 的复选框）：`on` 是模型给出的值
    Switch {
        on: bool,
    },
    /// 单选：`selected` 是模型给出的值；`value` 是本项在组里的取值（点击后经
    /// `Signal<String>` 绑定写回，同组其它项由 view() 重跑自然更新——声明式免机制）
    Radio {
        selected: bool,
        value: String,
    },
    /// 用户自绘/自定义行为（`CustomCell` 按指针判等：同一个 cell = 实例跨帧保留）
    Custom(crate::custom::CustomCell),
}

impl KindDesc {
    pub fn tag(&self) -> KindTag {
        match self {
            KindDesc::Box => KindTag::Box,
            KindDesc::Text(_) => KindTag::Text,
            KindDesc::Image(_) => KindTag::Image,
            KindDesc::Button { .. } => KindTag::Button,
            KindDesc::Checkbox { .. } => KindTag::Checkbox,
            KindDesc::Slider { .. } => KindTag::Slider,
            KindDesc::Progress { .. } => KindTag::Progress,
            KindDesc::Input { .. } => KindTag::Input,
            KindDesc::Switch { .. } => KindTag::Switch,
            KindDesc::Radio { .. } => KindTag::Radio,
            KindDesc::Custom(_) => KindTag::Custom,
        }
    }

    /// 把 desc 应用到 `Kind`，返回**是否真的变了**（未变 ⇒ 对齐阶段零操作）。
    ///
    /// 标签不同时返回 `true`（调用方应先比 `tag()` 并走"重建"路径）。
    pub fn apply_to(&self, kind: &mut Kind) -> bool {
        match (self, kind) {
            (KindDesc::Box, Kind::Box) => false,
            (KindDesc::Text(new), Kind::Text(old)) => {
                if old == new {
                    false
                } else {
                    old.clone_from(new);
                    true
                }
            }
            (KindDesc::Image(new), Kind::Image(old)) => {
                if Arc::ptr_eq(old, new) {
                    false
                } else {
                    *old = Arc::clone(new);
                    true
                }
            }
            (KindDesc::Button { label }, Kind::Button { label: old }) => {
                if old == label {
                    false
                } else {
                    old.clone_from(label);
                    true
                }
            }
            (KindDesc::Checkbox { checked }, Kind::Checkbox { checked: old }) => {
                let changed = *old != *checked;
                *old = *checked;
                changed
            }
            (
                KindDesc::Slider { value, min, max },
                Kind::Slider {
                    value: old,
                    min: old_min,
                    max: old_max,
                    ..
                },
            ) => {
                // 用位比较，避免 NaN 每次都判"变了"
                let changed = old.to_bits() != value.to_bits()
                    || old_min.to_bits() != min.to_bits()
                    || old_max.to_bits() != max.to_bits();
                *old = *value;
                *old_min = *min;
                *old_max = *max;
                changed
            }
            (KindDesc::Progress { value }, Kind::Progress { value: old }) => {
                let changed = old.to_bits() != value.to_bits();
                *old = *value;
                changed
            }
            (
                KindDesc::Input { text, placeholder },
                Kind::Input {
                    text: old,
                    placeholder: old_ph,
                    caret,
                    anchor,
                    preedit,
                    ..
                },
            ) => {
                let mut changed = false;
                // 模型 → 视图：只在**真的不同**时覆盖编辑缓冲（编辑回写的值与之相同 ⇒ 不打断光标）
                if old != text && preedit.is_empty() {
                    old.clone_from(text);
                    *caret = old.len();
                    *anchor = *caret;
                    changed = true;
                }
                if old_ph != placeholder {
                    old_ph.clone_from(placeholder);
                    changed = true;
                }
                changed
            }
            // 同一个 cell = 实例跨帧保留（指针判等）；换了 cell = 数据变了
            (KindDesc::Custom(new), Kind::Custom(old)) => {
                if old.ptr_eq(new) {
                    false
                } else {
                    *old = new.clone();
                    true
                }
            }
            (KindDesc::Switch { on }, Kind::Switch { on: old }) => {
                let changed = *old != *on;
                *old = *on;
                changed
            }
            (
                KindDesc::Radio { selected, value },
                Kind::Radio {
                    selected: old_sel,
                    value: old_val,
                },
            ) => {
                let changed = *old_sel != *selected || old_val != value;
                *old_sel = *selected;
                old_val.clone_from(value);
                changed
            }
            _ => true,
        }
    }
}

/// 组件的运行时形态：**desc 组**（对齐覆盖）+ **state 组**（跨帧保留）
#[derive(Clone, Debug)]
pub enum Kind {
    Box,
    Text(String),
    Image(Arc<ImageData>),
    Button {
        label: String,
    },
    Checkbox {
        checked: bool,
    },
    Slider {
        // ── desc ──
        value: f32,
        min: f32,
        max: f32,
        // ── state ──
        dragging: bool,
    },
    Progress {
        value: f32,
    },
    /// 文本输入框：`text`/`placeholder` 来自**描述**（模型是唯一真相），
    /// `caret`/`anchor`/`preedit`/`scroll` 是**编辑会话的视图态**（描述不碰）。
    ///
    /// 编辑时先改这里的 `text`（立即重绘）再写回绑定 signal（保持模型同步），
    /// 因此下一帧 `view()` 产出的描述值与它一致 ⇒ 光标不会被"值回弹"打断。
    Input {
        // ── desc ──
        text: String,
        placeholder: String,
        // ── state ──
        /// 光标（字节偏移，保证落在 char 边界）
        caret: usize,
        /// 选区锚点（`anchor == caret` 表示无选区）
        anchor: usize,
        /// IME 组合中的文本（未提交）
        preedit: String,
        /// 水平滚动偏移（文本超宽时保持光标可见）
        scroll: f32,
    },
    /// 开关（pill 形 + 圆形 thumb；交互同复选框：点击翻转并写回 `checked` 绑定）
    Switch {
        on: bool,
    },
    /// 单选：点击把 `value` 写回 `Signal<String>` 绑定（同组互斥由模型+view 重跑自然完成）
    Radio {
        selected: bool,
        value: String,
    },
    /// 用户自绘/自定义行为：实例（desc+state 一体）跨帧保留在保留树里，
    /// 对齐按 **Rc 指针**判等 —— 换 cell = 换数据，同一 cell = 不动（视图态不丢）。
    /// 所有 hook 都不拿 `&mut Track`（见 `custom::CustomNode` 的文档）。
    Custom(crate::custom::CustomCell),
}

impl Kind {
    pub fn tag(&self) -> KindTag {
        match self {
            Kind::Box => KindTag::Box,
            Kind::Text(_) => KindTag::Text,
            Kind::Image(_) => KindTag::Image,
            Kind::Button { .. } => KindTag::Button,
            Kind::Checkbox { .. } => KindTag::Checkbox,
            Kind::Slider { .. } => KindTag::Slider,
            Kind::Progress { .. } => KindTag::Progress,
            Kind::Input { .. } => KindTag::Input,
            Kind::Switch { .. } => KindTag::Switch,
            Kind::Radio { .. } => KindTag::Radio,
            Kind::Custom(_) => KindTag::Custom,
        }
    }

    /// 从 desc 造一个全新节点（desc 组写入，state 组取默认）
    pub fn from_desc(desc: &KindDesc) -> Self {
        match desc {
            KindDesc::Box => Kind::Box,
            KindDesc::Text(s) => Kind::Text(s.clone()),
            KindDesc::Image(d) => Kind::Image(Arc::clone(d)),
            KindDesc::Button { label } => Kind::Button { label: label.clone() },
            KindDesc::Checkbox { checked } => Kind::Checkbox { checked: *checked },
            KindDesc::Slider { value, min, max } => Kind::Slider {
                value: *value,
                min: *min,
                max: *max,
                dragging: false,
            },
            KindDesc::Progress { value } => Kind::Progress { value: *value },
            KindDesc::Input { text, placeholder } => Kind::Input {
                text: text.clone(),
                placeholder: placeholder.clone(),
                caret: text.len(),
                anchor: text.len(),
                preedit: String::new(),
                scroll: 0.0,
            },
            KindDesc::Custom(cell) => Kind::Custom(cell.clone()),
            KindDesc::Switch { on } => Kind::Switch { on: *on },
            KindDesc::Radio { selected, value } => Kind::Radio {
                selected: *selected,
                value: value.clone(),
            },
        }
    }
}

// ───────────────────────── 文本编辑（Input 的内置行为 / 剪贴板共用的核心 API）─────────────────────────

/// 前一个 char 边界
pub fn prev_char_boundary(s: &str, i: usize) -> usize {
    let i = i.min(s.len());
    s[..i].char_indices().next_back().map(|(k, _)| k).unwrap_or(0)
}

/// 后一个 char 边界
pub fn next_char_boundary(s: &str, i: usize) -> usize {
    let i = i.min(s.len());
    s[i..].chars().next().map(|c| i + c.len_utf8()).unwrap_or(s.len())
}

/// 把任意字节偏移夹到 char 边界
pub fn clamp_to_char_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

// ───────────────────────── key / 层 ─────────────────────────

/// 列表项稳定 key（`keyed_list` 用；列表是唯一需要 key 匹配的地方）
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    Str(String),
    I64(i64),
    U64(u64),
}

impl From<&str> for Key {
    fn from(v: &str) -> Self {
        Key::Str(v.to_string())
    }
}

impl From<String> for Key {
    fn from(v: String) -> Self {
        Key::Str(v)
    }
}

impl From<i64> for Key {
    fn from(v: i64) -> Self {
        Key::I64(v)
    }
}

impl From<u64> for Key {
    fn from(v: u64) -> Self {
        Key::U64(v)
    }
}

/// 层类型（z 顺序 = 枚举顺序）；层在 `view()` 里**声明式嵌套**（设计 §3.15）
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Layer {
    /// 每窗口恰一个：用户 `view()` 的最外层内容
    Content,
    /// 装饰 / 水印 / 遮罩：默认命中穿透
    Overlay,
    /// 锚定浮层（菜单 / 下拉）：默认点击外部关闭
    Popup,
    Tooltip,
    /// 默认 backdrop + 阻断下层
    Modal,
    /// 拖拽预览：最高
    DragPreview,
}

impl Layer {
    /// **全部层，按 z 序从低到高**（= 枚举声明序）。
    ///
    /// ## 为什么要有这个常量
    ///
    /// 层序此前是**两份手写数组**：`render/scene.rs` 的 `LAYER_BOTTOM_UP`（自下而上）
    /// 与 `hit.rs` 的 `LAYER_TOP_DOWN`（自上而下）。两份必须**严格互逆**，否则
    /// 会出现"画在上面的层收不到点击"这种不报错的错。
    ///
    /// 但它们分散在两个文件、没有任何机制保证互逆 —— **加一个 `Layer` 变体时，
    /// 漏改一处就是静默 bug**（这正是 P1「`Layer` 增变体不报错」的成因）。
    ///
    /// 现在两侧都从本常量派生（渲染侧正序、命中侧逆序）⇒ **不可能不同步**。
    /// 语义仍由**枚举声明序**决定，本常量必须与之一致（由
    /// `layer_all_matches_enum_declaration_order` 测试钉住）。
    pub const ALL: [Layer; 6] = [
        Layer::Content,
        Layer::Overlay,
        Layer::Popup,
        Layer::Tooltip,
        Layer::Modal,
        Layer::DragPreview,
    ];
}

/// 浮层锚点方位
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Placement {
    Below,
    Above,
    RightOf,
    LeftOf,
    ScreenCenter,
    Fixed { x: f32, y: f32 },
}

/// 层焦点策略
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum FocusPolicy {
    /// 不干扰下层
    Transparent,
    /// 点击外部关闭
    Dismissable,
    /// 阻断其下所有层
    BlockBelow,
}

/// 浮层锚点的目标
#[derive(Clone, PartialEq, Debug)]
pub enum AnchorTarget {
    /// 命中第一个带该 key 的节点（`popup_at` 的声明式用法）
    Key(Key),
    /// 直接指定节点（框架内部用法：tooltip 锚到任意 hover 节点，不要求有 key）
    Node(NodeId),
    /// 锚到一个**逻辑坐标点**（典型用途：右键菜单"贴着鼠标出现"）。
    ///
    /// 定位时它被当成一个**零尺寸的退化矩形**（`Rect::new(x, y, 0, 0)`），
    /// 于是翻转与视口钳制逻辑与锚节点**完全一致**：`Below` 落在 `y + ANCHOR_GAP`，
    /// 下方放不下就翻到上方，靠近右/下边缘时自动平移回视口内。
    ///
    /// 用 [`crate::view::ViewBuf::popup_at_point`] 声明（它会顺带填好 `Layer::Popup`
    /// 的默认视觉与关闭策略）。
    Point(Point),
}

/// 浮层锚点：布局后才解析成 rect（避免"rect 还没算出来"的老问题）
#[derive(Clone, PartialEq, Debug)]
pub struct Anchor {
    pub target: AnchorTarget,
    pub placement: Placement,
}

/// 每层的可配项（默认值由 `Layer` 给出）
#[derive(Clone, PartialEq, Debug)]
pub struct LayerOpts {
    pub backdrop: Option<Color>,
    pub blocks_below: bool,
    pub dismiss_on_outside_click: bool,
    pub hit_test_visible: bool,
    pub anchor: Option<Anchor>,
    pub focus: FocusPolicy,
}

impl LayerOpts {
    pub fn for_layer(layer: Layer) -> Self {
        match layer {
            Layer::Content => Self {
                backdrop: None,
                blocks_below: false,
                dismiss_on_outside_click: false,
                hit_test_visible: true,
                anchor: None,
                focus: FocusPolicy::Transparent,
            },
            Layer::Overlay => Self {
                backdrop: None,
                blocks_below: false,
                dismiss_on_outside_click: false,
                // 水印/装饰：默认命中穿透
                hit_test_visible: false,
                anchor: None,
                focus: FocusPolicy::Transparent,
            },
            Layer::Popup | Layer::Tooltip => Self {
                backdrop: None,
                blocks_below: false,
                dismiss_on_outside_click: true,
                hit_test_visible: true,
                anchor: None,
                focus: FocusPolicy::Dismissable,
            },
            Layer::Modal => Self {
                // 无主题上下文的兜底；有主题时由 `ViewBuf::layer` 用 `theme.backdrop` 覆盖
                backdrop: Some(Color::rgba(0, 0, 0, 80)),
                blocks_below: true,
                dismiss_on_outside_click: false,
                hit_test_visible: true,
                anchor: None,
                focus: FocusPolicy::BlockBelow,
            },
            Layer::DragPreview => Self {
                backdrop: None,
                blocks_below: false,
                dismiss_on_outside_click: false,
                hit_test_visible: false,
                anchor: None,
                focus: FocusPolicy::Transparent,
            },
        }
    }
}

/// 层条目身份
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct RootId(pub u32);

/// 一个层条目（= 一个根）
#[derive(Clone, Debug)]
pub struct Root {
    pub id: RootId,
    pub node: NodeId,
    pub layer: Layer,
    /// 声明它的父层条目（`None` = Content）
    pub owner: Option<RootId>,
    pub opts: LayerOpts,
    /// 框架自管层（tooltip 等）：`align` 的 stale 清理**跳过**它们，
    /// 生命周期归框架（`WindowCtx` 的 tooltip 会话）而不是 view()。
    pub framework: bool,
    /// 声明时的标签（`ViewBuf::modal_tagged`）：按标签找回这个层
    /// （框架的 loading 遮罩靠它定位动画节点）。
    pub tag: Option<u64>,
}

/// 滚动条 thumb 的拖拽会话（`widgets::scroll_handle` 维护）
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct ScrollDrag {
    pub pointer: PointerId,
    /// `true` = 拖竖直 thumb
    pub vertical: bool,
    /// 按下点到 thumb 顶（竖直）/ 左（水平）缘的距离（保持抓取点不跳）
    pub grab: f32,
}

// ───────────────────────── 节点 ─────────────────────────

/// 保留树节点：视图描述 + 视图态 + 布局结果，归位一处
pub struct Node {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,

    pub kind: Kind,
    /// 对齐用的稳定 key（列表项）
    pub key: Option<Key>,
    /// 悬停提示（框架级 tooltip：hover 该节点 600ms 后自动浮出；`None` = 无）
    pub tooltip: Option<String>,
    /// 挂在该节点上的右键菜单（框架的"上下文菜单会话"读取；`None` = 无）
    pub context_menu: Option<crate::menu::ContextMenu>,
    /// **测度时实际使用的换行宽度**（布局引擎写回；绘制复用同一约束，
    /// 避免"测度按约束换行、绘制不换行"的顶对齐错位）
    pub text_wrap: Option<f32>,

    // ── 样式（内联；无选择器/继承）──
    pub layout: FlexStyle,
    pub paint: PaintStyle,
    pub text: TextStyle,
    pub image: ImageStyle,

    // ── 可视 / 命中属性（对齐 WinUI UIElement）──
    /// ⚠️ **运行期修改请用 [`Track::set_visibility`]**，直接写字段会漏掉
    /// 「父流重排」与「旧像素登记脏区」——两者都不报错，只是**看起来坏了**。
    /// （声明式路径 `view()` 里的 `.visible(..)` 除外：那是构建，下一帧 `align` 会统一处理。）
    pub visibility: Visibility,
    pub hit_test_visible: bool,
    pub clip: Option<Rect>,
    pub transform: Transform,
    pub tab_stop: bool,
    pub tab_index: i32,
    pub layout_rounding: bool,
    pub allow_drop: bool,
    pub can_drag: bool,

    // ── 视图态（框架拥有，对齐时不被描述覆盖）──
    pub interaction: InteractionState,
    pub focus_state: FocusState,
    /// 滚动偏移（`Kind::Box` + `layout.overflow_scroll` 时有效）
    pub scroll_offset: (f32, f32),
    /// 内容尺寸（布局阶段写回，用于钳制滚动）
    pub content_size: Size,
    /// 滚动条 thumb 拖拽会话（`scroll_handle` 维护；`grab` = 按下点到 thumb 顶/左缘的距离）
    pub scroll_drag: Option<ScrollDrag>,
    /// 偏移**真的变了**的滚动容器（等帧驱动派发 `ScrollChanged`）
    pub needs_scroll_event: bool,

    // ── 布局结果 ──
    pub flags: Flags,
    /// 这些标记是**哪一轮**布局期间提的（`refactor-plan` D58）。
    ///
    /// `clear_layout_flags` 只能清"本轮布局**开始之前**"就有的标记；
    /// 布局**过程中**新提的（比如 `write_back` 触发了新的 `mark_layout_dirty`）
    /// 必须留给下一轮 —— 否则会被一起清掉 ⇒漏失效 ⇒ 画面停在旧布局。
    pub layout_epoch: u32,
    pub desired: Size,
    pub computed: ComputedLayout,

    // ── 事件处理器（对齐时整体替换；替换**不**置脏）──
    pub handlers: Vec<HandlerSlot>,

    // ── 双向绑定（对齐时覆盖；**不**置脏）──
    pub bindings: Bindings,
}

impl Node {
    fn new(kind: Kind, key: Option<Key>) -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            kind,
            key,
            tooltip: None,
            context_menu: None,
            text_wrap: None,
            layout: FlexStyle::default(),
            layout_epoch: 0,
            paint: PaintStyle::default(),
            text: TextStyle::default(),
            image: ImageStyle::default(),
            visibility: Visibility::Visible,
            hit_test_visible: true,
            clip: None,
            transform: Transform::default(),
            tab_stop: false,
            tab_index: 0,
            layout_rounding: false,
            allow_drop: false,
            can_drag: false,
            interaction: InteractionState::default(),
            focus_state: FocusState::default(),
            scroll_offset: (0.0, 0.0),
            content_size: Size::zero(),
            scroll_drag: None,
            needs_scroll_event: false,
            // 新节点先标全脏；`ATTACHED` 在挂到树上时置位
            flags: Flags::MEASURE_DIRTY | Flags::ARRANGE_DIRTY | Flags::PAINT_DIRTY,
            desired: Size::zero(),
            computed: ComputedLayout::default(),
            handlers: Vec::new(),
            bindings: Bindings::default(),
        }
    }

    /// 节点自身的矩形（布局结果）
    pub fn rect(&self) -> Rect {
        Rect::new(
            self.computed.x,
            self.computed.y,
            self.computed.width,
            self.computed.height,
        )
    }

    /// **绘制影响范围**（比 [`Node::rect`] 更紧）：文本节点只在内容范围内画。
    ///
    /// 用途：脏区。文本节点常被拉伸到容器宽度（`align_items: stretch`），
    /// 但字形只占 `desired`（固有测度）那一块；按整条矩形标脏会让"改一个字"
    /// 变成"重画整行宽度"，脏区收益直接减半。
    pub fn paint_bounds(&self) -> Rect {
        let r = self.rect();
        let mut base = r;
        if matches!(self.kind, Kind::Text(_)) {
            let w = self.desired.width.min(r.width).max(0.0);
            let h = self.desired.height.min(r.height).max(0.0);
            if w > 0.0 && h > 0.0 && (w < r.width - 0.5 || h < r.height - 0.5) {
                let x = match self.text.spec.text_align {
                    lieui_text::TextAlign::Center => r.x + (r.width - w) * 0.5,
                    lieui_text::TextAlign::End => r.right() - w,
                    _ => r.x,
                };
                base = Rect::new(x, r.y, w, h);
            }
        }
        // ★ 阴影的可见范围**超出**节点矩形（D33）。
        //   绘制时阴影画在 `rect + offset`，再`inflate(spread)`，并带高斯模糊
        //   （`std_dev = blur * 0.5`，见 `widgets/mod.rs` 的 draw）。
        //   高斯的 3σ 覆盖 99.7% ⇒可见外扩 ≈ `blur * 1.5`。
        //   而脏区按 `paint_bounds`取 ⇒ **光晕外圈不在脏区内 ⇒ 残影**。
        //
        //   这里保守地四边都外扩（偏移方向未知时宁可多标一点）。
        match self.paint.shadow {
            Some(sh) => {
                let reach = sh.spread.max(0.0) + sh.blur.max(0.0) * 1.5;
                if reach > 0.0 {
                    return base.inflate(reach);
                }
                base
            }
            None => base,
        }
    }
}

// ───────────────────────── 保留树 ─────────────────────────

/// 按下态：**比"按下节点"多记了按下时的指针与按键**。
///
/// 为什么必须多记这两样：`Tapped`（点击）合成要校验"按下与抬起是**同一按键**"。
/// 只记节点的话，**右键按下 + 左键抬起落在同一节点会被判成左键点击** ⇒ 触发勾选 / 提交 / 删除
/// （`refactor-plan` D1，数据破坏级）。
///
/// `pointer` / `pos` 是为后续的位移阈值 / 长按阈值预留的：`input::step` 是纯函数、拿不到时钟，
/// 加时间阈值要改签名并穿透全部调用点，所以分两步走（见 `refactor-plan` A1）。
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PressState {
    /// 按下时命中的目标（`Tapped` 派发给它）
    pub node: NodeId,
    /// 按下时的指针 —— 触屏 / 多指下同一节点可能被不同指针先后按下
    pub pointer: PointerId,
    /// 按下时的按键：抬起时用它配对，决定合成 `Tapped` 还是 `RightTapped`
    pub button: PointerButton,
    /// 按下时的位置（位移阈值用）
    pub pos: Point,
}

/// 保留树：**每窗口一棵**，跨帧存活
#[derive(Default)]
pub struct Track {
    nodes: Vec<Option<Node>>,
    gens: Vec<u32>,
    free: Vec<u32>,
    roots: Vec<Root>,
    next_root: u32,
    /// 当前布局纪元（`refactor-plan` D58）。`layout()` 每轮开始时递增，
    /// `clear_layout_flags` 据此只清"本轮之前"提的标记。
    layout_epoch: u32,

    // ── 窗口级视图态 ──
    /// 当前 hover 的最深节点
    pub hover: Option<NodeId>,
    /// 命中链（根 → 最深命中），用于 hover 传播与标脏
    pub hover_path: Vec<NodeId>,
    /// 当前按下态（含指针 / 按键，见 [`PressState`]）
    pub pressed: Option<PressState>,
    pub pressed_path: Vec<NodeId>,
    /// 键盘焦点
    pub focused: Option<NodeId>,
    /// 指针捕获（多指针）
    pub captures: Vec<(PointerId, NodeId)>,
    /// 光标闪烁**相位**（窗口级：同一时刻只有焦点输入框画光标；由帧驱动翻转）
    ///
    /// 分工：这里的 `blink_on` 是"当前该不该画光标"（**状态**，跟焦点节点走），
    /// "下一次翻转的时刻"（**时钟**）在 `WindowCtx` 的会话里 —— 它是一种唤醒源，
    /// 没焦点时为 `None` ⇒ 不产生定时唤醒 ⇒ 空闲零功耗。
    pub blink_on: bool,

    // ── 脏区（align / 事件阶段登记，渲染阶段消费）──
    /// 需要重绘的矩形（合并/上屏在 M3）
    pub damage: Vec<Rect>,
    /// 整窗脏（层出现/消失、首次布局、尺寸变化）
    pub damage_all: bool,
}

impl Track {
    pub fn new() -> Self {
        Self::default()
    }

    // ── arena ──

    pub fn len(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.get(id).is_some()
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        let i = id.index() as usize;
        match self.nodes.get(i) {
            Some(Some(n)) if self.gens[i] == id.generation() => Some(n),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        let i = id.index() as usize;
        match self.nodes.get_mut(i) {
            Some(Some(n)) if self.gens[i] == id.generation() => Some(n),
            _ => None,
        }
    }

    /// 创建游离节点（未挂载）。挂载用 [`Track::append_child`] / [`Track::add_root`]。
    pub fn create(&mut self, kind: Kind, key: Option<Key>) -> NodeId {
        // 自定义节点：挂载即通知（资源初始化 / 订阅外部事件的时机）
        if let Kind::Custom(cell) = &kind
            && let Some(mut inst) = cell.try_borrow_mut()
        {
            inst.on_attached();
        }
        let node = Node::new(kind, key);
        self.alloc(node)
    }

    fn alloc(&mut self, node: Node) -> NodeId {
        if let Some(idx) = self.free.pop() {
            let i = idx as usize;
            self.nodes[i] = Some(node);
            NodeId::new(idx, self.gens[i])
        } else {
            let idx = self.nodes.len() as u32;
            self.nodes.push(Some(node));
            self.gens.push(0);
            NodeId::new(idx, 0)
        }
    }

    fn free_slot(&mut self, id: NodeId) {
        let i = id.index() as usize;
        if i >= self.nodes.len() || self.nodes[i].is_none() {
            return;
        }
        self.nodes[i] = None;
        self.gens[i] = self.gens[i].wrapping_add(1);
        self.free.push(id.index());
    }

    // ── 树结构 ──

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.get(id).map(|n| n.children.as_slice()).unwrap_or(&[])
    }

    pub fn parent_of(&self, id: NodeId) -> Option<NodeId> {
        self.get(id).and_then(|n| n.parent)
    }

    /// 自身 → 根（不含根之后的）
    pub fn ancestors(&self, id: NodeId) -> Ancestors<'_> {
        Ancestors {
            track: self,
            cursor: self.parent_of(id),
        }
    }

    /// 深度优先（含自身）
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        self.collect_subtree(id, &mut out);
        out
    }

    fn collect_subtree(&self, id: NodeId, out: &mut Vec<NodeId>) {
        out.push(id);
        if let Some(n) = self.get(id) {
            for c in &n.children {
                self.collect_subtree(*c, out);
            }
        }
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        self.insert_child(parent, usize::MAX, child);
    }

    pub fn insert_child(&mut self, parent: NodeId, pos: usize, child: NodeId) {
        // 先摘除（避免重复出现）
        self.detach(child);
        if let Some(c) = self.get_mut(child) {
            c.parent = Some(parent);
            c.flags.insert(Flags::ATTACHED);
        }
        let added = if let Some(p) = self.get_mut(parent) {
            let pos = pos.min(p.children.len());
            p.children.insert(pos, child);
            true
        } else {
            false
        };
        debug_assert!(added, "insert_child: 父节点不存在");
        self.mark_layout_dirty(parent);
    }

    /// 从父节点的 children 里摘掉（**不销毁**子树）
    pub fn detach(&mut self, child: NodeId) {
        let Some(parent) = self.parent_of(child) else {
            return;
        };
        if let Some(p) = self.get_mut(parent) {
            p.children.retain(|c| *c != child);
        }
        if let Some(c) = self.get_mut(child) {
            c.parent = None;
            c.flags.remove(Flags::ATTACHED);
        }
    }

    /// 一次性重排某个父节点的子节点顺序（`children` 必须恰好是当前子集）
    pub fn set_children(&mut self, parent: NodeId, children: &[NodeId]) {
        if let Some(p) = self.get_mut(parent) {
            debug_assert_eq!(p.children.len(), children.len(), "set_children 必须给全量顺序");
            p.children.clear();
            p.children.extend_from_slice(children);
        }
        self.mark_layout_dirty(parent);
    }

    /// 销毁子树（不含从父节点摘除的语义：会一并摘除）
    pub fn destroy(&mut self, id: NodeId) -> usize {
        let ids = self.descendants(id);
        // ★ 破坏**前**登记旧绘制范围（refactor-plan D2）。
        // 脏区是"旧像素 ∪ 新像素"：布局写回会登记旧∪新矩形，但删除路径不会——
        // 于是同帧只要还有别的脏区（非空），被删节点所占的那块像素就没人重绘 ⇒ 残影。
        // 之前只有"脏区恰好为空 ⇒ 渲染层按整窗处理"这条兜底掩盖着它。
        // 必须在节点还活着时问damage_bounds（它要沿祖先链取变换）。
        for i in &ids {
            if let Some(r) = self.damage_bounds(*i) {
                self.damage_rect(r);
            }
        }
        let parent = self.parent_of(id);
        self.detach(id);
        // 自定义节点：销毁前通知（资源清理的时机）
        for i in &ids {
            if let Some(n) = self.get(*i)
                && let Kind::Custom(cell) = &n.kind
                && let Some(mut inst) = cell.try_borrow_mut()
            {
                inst.on_detached();
            }
        }
        // 兄弟位置变化 ⇒ 父节点需要重排
        if let Some(p) = parent {
            self.mark_layout_dirty(p);
        }
        for rid in ids.iter().rev() {
            self.free_slot(*rid);
        }
        // 该子树若曾是某个层根的根节点，顺手摘层
        self.roots.retain(|r| !ids.contains(&r.node));
        ids.len()
    }

    // ── 层（roots）──

    pub fn roots(&self) -> &[Root] {
        &self.roots
    }

    pub fn root(&self, id: RootId) -> Option<&Root> {
        self.roots.iter().find(|r| r.id == id)
    }

    pub fn root_mut(&mut self, id: RootId) -> Option<&mut Root> {
        self.roots.iter_mut().find(|r| r.id == id)
    }

    /// `Layer::Content` 的根（每窗口恰一个）
    pub fn content_root(&self) -> Option<&Root> {
        self.roots.iter().find(|r| r.layer == Layer::Content)
    }

    pub fn roots_of(&self, layer: Layer) -> impl Iterator<Item = &Root> {
        self.roots.iter().filter(move |r| r.layer == layer)
    }

    /// **按 z 序（从下到上）返回全部层根** —— 渲染遍历顺序。
    ///
    /// 见 [`Self::z_order_impl`] 的排序键说明。
    pub fn z_ordered_roots(&self) -> Vec<&Root> {
        self.z_order_impl(false)
    }

    /// **按 z 序（从上到下）返回全部层根** —— 命中遍历顺序。
    ///
    /// ## 为什么不能直接 `z_ordered_roots().rev()`
    ///
    /// 排序是**稳定**的，"同 `(Layer, 深度)` 内后声明的在上"这条规则
    /// **靠稳定性保证**。直接 `.rev()` 会把同 key 内的顺序也翻转
    /// ⇒ 变成"先声明的在上" ⇒ **子菜单会跑到父菜单下面**。
    ///
    /// 所以命中侧用 `sort_by(|a, b| key(b).cmp(&key(a)))`：**key 降序、
    /// 同 key 仍保持声明序**。
    pub fn z_ordered_roots_top_down(&self) -> Vec<&Root> {
        self.z_order_impl(true)
    }

    /// z 序排序核心。
    ///
    /// ## 排序键：`z = (Layer, 嵌套深度)` + **稳定性承载声明序号**
    ///
    /// - **Layer**：主序（`Layer::ALL` 的声明序）；
    /// - **嵌套深度**：让**嵌套在父层里的层高于父层**（设计 §3.7）。
    ///   例如 Modal 里声明的 Popup（`owner` = Modal）应盖在 Modal 之上，
    ///   否则它会被 Modal 的 backdrop 盖住且收不到点击。
    /// - **声明序号**：不显式参与 key，而是**由稳定排序承载** ——
    ///   同 `(Layer, 深度)` 时后声明的自然排在上（子菜单正是靠这条叠放）。
    ///
    /// ## 性能
    ///
    /// O(n + Σdepth)，稳定排序额外 O(n log n)。层数是**小常数**（每窗口通常 < 20），
    /// 不是热路径。刻意**没有**加 `RootId → index` 索引表：那会让 `add_root` /
    /// `remove_root` / `root_mut` / `roots` 四处都背上"索引可能失效"的耦合，
    /// 而 `root()` 的 O(n) 查找在 n=6 量级完全不是问题。**先量后优化**。
    ///
    /// ## 悬空 / 成环的 owner
    ///
    /// `remove_root` 会级联删除嵌套子层，但 `Root.owner` 可能指向**已消失**的层
    /// （父层被 `Cmd` 单独移除、或对齐阶段先删了父）。此时该层**按顶层处理**（深度 0）。
    /// `owner` 链若成环（A 指 B、B 指 A），用 `MAX_NESTING_DEPTH` 截断 ——
    /// 否则排序会无限循环。两种情况都**不 panic**，只是退化。
    fn z_order_impl(&self, top_down: bool) -> Vec<&Root> {
        /// owner 链深度上限：防御成环，同时远大于任何真实嵌套层数。
        const MAX_NESTING_DEPTH: usize = 16;

        let key = |r: &Root| -> (usize, usize) {
            let mut depth = 0usize;
            // ★ 嵌套层**继承 owner 链顶层的 Layer 基准**，而不是用自己的 Layer 槽。
            //
            //   设计 §3.7 写的是 `z = (Layer, 嵌套深度, 序号)`，但**字面实现会让
            //   Modal 内的 Popup（Layer 槽 2）排在 Modal（槽 4）之下** ——
            //   也就是"Modal 里弹菜单"仍然不可用，整个嵌套功能失去意义。
            //   （实测：`nested_popup_above_its_modal_parent` 正是这样失败的。）
            //
            //   正确语义：`Layer` 是**槽位**，嵌套声明意味着"我属于这个槽位内部"，
            //   所以外层基准取顶层祖先的 Layer，深度只用来区分"槽内 / 槽外"。
            //   独立弹窗（无 owner）仍用自己的 Layer 槽 ⇒ 不会被模态框盖住，
            //   也不会盖住模态框 —— 这正是期望的模态语义。
            let mut top = r.layer;
            let mut cur = r.owner;
            while let Some(oid) = cur
                && depth < MAX_NESTING_DEPTH
            {
                match self.root(oid) {
                    // 悬空 owner ⇒ 停止上溯，沿用已累积的基准（退化为该层自己的槽位）
                    None => break,
                    Some(p) => {
                        depth += 1;
                        top = p.layer;
                        cur = p.owner;
                    }
                }
            }
            (top as usize, depth)
        };

        // 显式把「声明序号」一起排进key，**不依赖稳定性** —— 行为更直白，
        // 且两个方向写起来完全对称。
        //
        // top_down : (key, 序号) 全降序  ⇒ 同层同深时**后声明的先命中**
        // 自下而上: (key, 序号) 全升序  ⇒ 同层同深时**后声明的先绘制**（在上）
        //
        // ★ 曾经踩过的坑：只按 key 降序 + 稳定排序，会让同 key 内保持**升序**
        //   （先声明的先来），正好与"后声明的在上"相反 —— 子菜单会跑到父菜单下面。
        let mut idx: Vec<(usize, &Root)> = self.roots.iter().enumerate().collect();
        match top_down {
            true => idx.sort_by(|(ia, a), (ib, b)| key(b).cmp(&key(a)).then_with(|| ib.cmp(ia))),
            false => idx.sort_by(|(ia, a), (ib, b)| key(a).cmp(&key(b)).then_with(|| ia.cmp(ib))),
        }
        idx.into_iter().map(|(_, r)| r).collect()
    }

    /// 按标签找层根（`ViewBuf::modal_tagged` 声明时打的标签）
    pub fn root_by_tag(&self, tag: u64) -> Option<&Root> {
        self.roots.iter().find(|r| r.tag == Some(tag))
    }

    /// 挂一个层根（节点应为游离或已挂载的子树根）
    pub fn add_root(&mut self, layer: Layer, owner: Option<RootId>, node: NodeId) -> RootId {
        let id = RootId(self.next_root);
        self.next_root += 1;
        let opts = LayerOpts::for_layer(layer);
        self.roots.push(Root {
            id,
            node,
            layer,
            owner,
            opts,
            framework: false,
            tag: None,
        });
        if let Some(n) = self.get_mut(node) {
            n.flags.insert(Flags::ATTACHED);
        }
        // 新层需要一次布局（它的 rect 还没算过），并按边界规则冒泡
        self.mark_layout_dirty(node);
        // 层出现 → 整窗脏（backdrop / 遮挡关系可能影响任意像素）
        self.damage_whole_window();
        id
    }

    /// 挂一个**框架自管**层根（tooltip 等）：`align` 的 stale 清理会跳过它，
    /// 生命周期归调用方（打开者负责用 [`Track::remove_root`] 关闭）。
    pub fn add_framework_root(&mut self, layer: Layer, owner: Option<RootId>, node: NodeId) -> RootId {
        let rid = self.add_root(layer, owner, node);
        if let Some(r) = self.root_mut(rid) {
            r.framework = true;
        }
        rid
    }

    /// 摘掉一个层根并销毁其子树；返回**一并移除的层根数量**（含嵌套子层）
    pub fn remove_root(&mut self, id: RootId) -> usize {
        let Some(root) = self.root(id).cloned() else {
            return 0;
        };
        // 嵌套子层一起收走
        let descendants: Vec<RootId> = self
            .roots
            .iter()
            .filter(|r| r.owner == Some(id))
            .map(|r| r.id)
            .collect();
        let mut removed = 1;
        for d in descendants {
            removed += self.remove_root(d);
        }
        self.roots.retain(|r| r.id != id);
        self.destroy(root.node);
        self.damage_whole_window();
        removed
    }

    /// 按**位置**替换子节点（保持顺序 + 更新父指针）。
    ///
    /// 用途：`align` 发现"同一位置的组件类型变了"时重建该节点（设计 §3.4"类型不同即重建"）。
    /// 注意：必须在**销毁旧节点之前**调用 —— 销毁会把旧节点从 `children` 里摘掉，位置就丢了。
    pub fn replace_child_at(&mut self, parent: NodeId, index: usize, new: NodeId) {
        if let Some(p) = self.get_mut(parent)
            && index < p.children.len()
        {
            p.children[index] = new;
        }
        if let Some(n) = self.get_mut(new) {
            n.parent = Some(parent);
            n.flags.insert(Flags::ATTACHED);
        }
        self.mark_layout_dirty(parent);
    }

    // ── 视图态 ──

    pub fn state(&self, id: NodeId) -> InteractionState {
        self.get(id).map(|n| n.interaction).unwrap_or_default()
    }

    /// 置 hover 态。**只有声明了交互视觉的节点才标脏**——否则整窗大小的容器
    /// 一进 hover 就会把整个窗口标脏（旧实现正是如此），脏区优化就废了。
    pub fn set_pointer_over(&mut self, id: NodeId, v: bool) {
        let mut repaint = false;
        if let Some(n) = self.get_mut(id)
            && n.interaction.pointer_over != v
        {
            n.interaction.pointer_over = v;
            repaint = n.paint.is_interactive() || n.text.is_interactive();
        }
        if repaint {
            self.mark_paint_dirty(id);
        }
    }

    /// 置按下态（同上：无按下视觉的节点不标脏）
    pub fn set_pressed(&mut self, id: NodeId, v: bool) {
        let mut repaint = false;
        if let Some(n) = self.get_mut(id)
            && n.interaction.pressed != v
        {
            n.interaction.pressed = v;
            repaint = n.paint.pressed_background.is_some() || n.text.pressed_color.is_some();
        }
        if repaint {
            self.mark_paint_dirty(id);
        }
    }

    pub fn set_focused(&mut self, id: NodeId, state: FocusState) {
        if let Some(n) = self.get_mut(id)
            && n.focus_state != state
        {
            n.focus_state = state;
            n.interaction.focused = state != FocusState::Unfocused;
            self.mark_paint_dirty(id);
        }
    }

    /// 改可见性（返回是否变化）。**运行期改用它，不要直接写 `n.visibility`。**
    ///
    /// ## 为什么必须走这个方法
    ///
    /// `visibility` 同时被**三条**完全不同的路径读取，改动必须同时惊动它们：
    ///
    /// | 读取点 | 语义 |
    /// |---|---|
    /// | `hit.rs:73` | `!= Visible` ⇒ **不参与命中** |
    /// | `layout.rs:236` | `!= Collapsed` ⇒ **参与布局** |
    /// | `scene.rs:557` | `!= Visible` ⇒ **不绘制** |
    ///
    /// 直接写字段只会改到内存里的值：**既不会让父流重排**（兄弟的位置不对），
    /// **也不会把旧像素登记进脏区**（屏幕上留着残影）。
    /// 这两样都不会报错，只是**看起来坏了**—— GUI 里最难查的一类。
    ///
    /// ## 为什么是 `flow` 而不是 `layout`
    ///
    /// `Collapsed` 会让节点**退出父流**（占位消失）⇒ 兄弟要重排 ⇒ 必须连父节点
    /// 一起标脏，这就是 [`Self::mark_flow_dirty`] 与 [`Self::mark_layout_dirty`]
    /// 的唯一区别。
    ///
    /// ## 声明式路径不受影响
    ///
    /// `view()` 里的 `.visible(..)` / `.hidden()` / `.collapsed()` 仍然直接写字段
    /// ——那是**构建**路径，下一帧 `align` 会统一标脏（`align.rs:142-150`）。
    pub fn set_visibility(&mut self, id: NodeId, v: Visibility) -> bool {
        let changed = match self.get_mut(id) {
            Some(n) if n.visibility != v => {
                n.visibility = v;
                true
            }
            _ => false,
        };
        if changed {
            self.mark_flow_dirty(id);
            self.mark_paint_dirty(id);
        }
        changed
    }

    /// 改交互使能（返回是否变化）。**运行期改用它，不要直接写 `n.interaction.enabled`。**
    ///
    /// ## 为什么必须走这个方法
    ///
    /// `enabled` 同样被多条路径读取，其中**绘制侧就有4 处**会因它改变外观：
    ///
    /// | 读取点 | 语义 |
    /// |---|---|
    /// | `render/mod.rs` × 4 | 禁用态前景色 / 光标闪烁 / 命中反馈 |
    /// | `event.rs:802` | 事件**路由**跳过（菜单里的禁用项最典型） |
    /// | `focus.rs:63,78` | **焦点链**（`tab_order` / `focusable_ancestor`） |
    /// | `track.rs:1748` | `input_is_active` |
    ///
    /// 漏掉 `mark_paint_dirty` 的后果：禁用态要等**下一次**别的改动才会重绘。
    ///
    /// 事件路由与焦点链是**即时查询**（`hit` / `tab_order` 每次重新算），
    /// 所以**不需要**额外脏标记；但绘制需要。
    pub fn set_interaction_enabled(&mut self, id: NodeId, v: bool) -> bool {
        let changed = match self.get_mut(id) {
            Some(n) if n.interaction.enabled != v => {
                n.interaction.enabled = v;
                true
            }
            _ => false,
        };
        if changed {
            self.mark_paint_dirty(id);
        }
        changed
    }

    pub fn scroll_offset(&self, id: NodeId) -> (f32, f32) {
        self.get(id).map(|n| n.scroll_offset).unwrap_or((0.0, 0.0))
    }

    /// 写入滚动偏移（返回是否变化）。
    ///
    /// 子原点的 -offset 平移是**布局时烘焙**的（`write_back`）——所以偏移变化必须
    /// 标 LAYOUT（容器作为边界重排子树，子节点按新偏移重新平移），只标 PAINT
    /// 会出现"滚动条动了、内容没动"。变化时同时标记 `needs_scroll_event`
    /// （帧驱动据此派发 `ScrollChanged`；Track 无事件队列，只排队，派发归 app 层）。
    pub fn set_scroll_offset(&mut self, id: NodeId, v: (f32, f32)) -> bool {
        if let Some(n) = self.get_mut(id)
            && n.scroll_offset != v
        {
            n.scroll_offset = v;
            n.needs_scroll_event = true;
            // 容器（尺寸确定 ⇒ 是边界）标记重排 ⇒ 其子树按新偏移平移
            self.mark_layout_dirty(id);
            return true;
        }
        false
    }

    /// 取走"偏移变了的滚动容器"集合（帧驱动派发 `ScrollChanged` 用）
    pub fn take_scroll_changes(&mut self) -> Vec<NodeId> {
        let mut out = Vec::new();
        for (i, slot) in self.nodes.iter_mut().enumerate() {
            if let Some(n) = slot
                && n.needs_scroll_event
            {
                n.needs_scroll_event = false;
                out.push(NodeId::new(i as u32, self.gens[i]));
            }
        }
        out
    }

    /// 按 key 找节点（层锚点解析 / `raw` 逃逸接口定位用）
    pub fn find_by_key(&self, key: &Key) -> Option<NodeId> {
        self.node_ids()
            .find(|id| self.get(*id).and_then(|n| n.key.as_ref()) == Some(key))
    }

    /// 滑块：按指针 x 更新值，并写回绑定 signal。返回是否变化。
    ///
    /// 未绑定时只改节点自身（下一帧 `align` 会用描述的值覆盖回来 —— desc 是唯一真相）。
    pub fn slider_drag_to(&mut self, id: NodeId, x: f32) -> bool {
        let rect = self.get(id).map(|n| n.rect()).unwrap_or_default();
        // 结果 = (新值, 绑定)；None 表示值没变
        let (changed, binding) = {
            let Some(n) = self.get_mut(id) else {
                return false;
            };
            let Kind::Slider { value, min, max, .. } = &mut n.kind else {
                return false;
            };
            let t = if rect.width > 0.0 {
                ((x - rect.x) / rect.width).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let v = *min + t * (*max - *min);
            let changed = if v.to_bits() != value.to_bits() {
                *value = v;
                Some(v)
            } else {
                None
            };
            (changed, n.bindings.value.clone())
        };
        if let Some(v) = changed {
            // 立即重绘（不必等下一帧 view() → align 再落地）
            self.mark_paint_dirty(id);
            if let Some(sig) = binding {
                sig.set(v);
            }
        }
        changed.is_some()
    }

    /// 复选框：翻转值并写回绑定 signal（返回新值）
    pub fn toggle_checked(&mut self, id: NodeId) -> Option<bool> {
        let (next, binding) = {
            let n = self.get_mut(id)?;
            let Kind::Checkbox { checked } = &mut n.kind else {
                return None;
            };
            *checked = !*checked;
            (*checked, n.bindings.checked.clone())
        };
        self.mark_paint_dirty(id);
        if let Some(sig) = binding {
            sig.set(next);
        }
        Some(next)
    }

    /// 开关：翻转值并写回绑定 signal（返回新值）
    pub fn toggle_switch(&mut self, id: NodeId) -> Option<bool> {
        let (next, binding) = {
            let n = self.get_mut(id)?;
            let Kind::Switch { on } = &mut n.kind else {
                return None;
            };
            *on = !*on;
            (*on, n.bindings.checked.clone())
        };
        self.mark_paint_dirty(id);
        if let Some(sig) = binding {
            sig.set(next);
        }
        Some(next)
    }

    /// 单选：把本项 `value` 写回 `Signal<String>` 绑定。
    ///
    /// **不**遍历兄弟改互斥——信号变化 ⇒ `view()` 重跑 ⇒ 同组所有 radio 的
    /// `selected` 由对齐器自然更新（声明式免机制）。返回写回的取值。
    pub fn select_radio(&mut self, id: NodeId) -> Option<String> {
        let (value, binding) = {
            let n = self.get(id)?;
            let Kind::Radio { value, .. } = &n.kind else {
                return None;
            };
            (value.clone(), n.bindings.text.clone())
        };
        if let Some(sig) = binding {
            sig.set(value.clone());
        }
        Some(value)
    }

    // ── Input：编辑 API（内置行为与平台剪贴板共用）──

    /// 归一化的选区 `(start, end)`；无选区时返回 `None`
    pub fn input_selection(&self, id: NodeId) -> Option<(usize, usize)> {
        let n = self.get(id)?;
        let Kind::Input {
            caret, anchor, text, ..
        } = &n.kind
        else {
            return None;
        };
        if caret == anchor {
            return None;
        }
        let (a, b) = (*caret, *anchor);
        Some((a.min(b).min(text.len()), a.max(b).min(text.len())))
    }

    /// 选中的文本（无选区 ⇒ `None`）
    pub fn input_selected_text(&self, id: NodeId) -> Option<String> {
        let (a, b) = self.input_selection(id)?;
        let n = self.get(id)?;
        let Kind::Input { text, .. } = &n.kind else {
            return None;
        };
        Some(text[a..b].to_string())
    }

    /// 插入字符串（有选区则替换），并写回绑定。返回是否变化。
    pub fn input_insert(&mut self, id: NodeId, s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        let sel = self.input_selection(id);
        let binding = self.input_binding(id);
        let Some(n) = self.get_mut(id) else {
            return false;
        };
        let Kind::Input {
            text,
            caret,
            anchor,
            preedit,
            ..
        } = &mut n.kind
        else {
            return false;
        };
        // 提交时的第一个字符先清掉 IME 组合串（组合串还没进 text）
        if !preedit.is_empty() {
            preedit.clear();
        }
        let (a, b) = match sel {
            Some((a, b)) => (a, b),
            None => (*caret, *caret),
        };
        let (a, b) = (clamp_to_char_boundary(text, a), clamp_to_char_boundary(text, b));
        text.replace_range(a..b, s);
        let pos = a + s.len();
        *caret = pos;
        *anchor = pos;
        let new = text.clone();
        self.input_finish(id, true, binding, new);
        self.input_ensure_caret_visible(id);
        true
    }

    /// 退格：删选区，或删光标前一个字符
    pub fn input_backspace(&mut self, id: NodeId) -> bool {
        self.input_delete_range(id, true)
    }

    /// 删除：删选区，或删光标后一个字符
    pub fn input_delete(&mut self, id: NodeId) -> bool {
        self.input_delete_range(id, false)
    }

    fn input_delete_range(&mut self, id: NodeId, backward: bool) -> bool {
        let sel = self.input_selection(id);
        let binding = self.input_binding(id);
        let Some(n) = self.get_mut(id) else {
            return false;
        };
        let Kind::Input {
            text, caret, anchor, ..
        } = &mut n.kind
        else {
            return false;
        };
        let (a, b) = match sel {
            Some((a, b)) => (a, b),
            None => {
                if backward {
                    let c = clamp_to_char_boundary(text, *caret);
                    (prev_char_boundary(text, c), c)
                } else {
                    let c = clamp_to_char_boundary(text, *caret);
                    (c, next_char_boundary(text, c))
                }
            }
        };
        if a == b {
            return false;
        }
        text.replace_range(a..b, "");
        *caret = a;
        *anchor = a;
        let new = text.clone();
        self.input_finish(id, true, binding, new);
        self.input_ensure_caret_visible(id);
        true
    }

    /// 移动光标：`delta` 以**字符**为单位；`extend` = 按住 Shift 扩选区
    pub fn input_move_caret(&mut self, id: NodeId, delta: isize, extend: bool) -> bool {
        let cur = match self.get(id).map(|n| &n.kind) {
            Some(Kind::Input { caret, .. }) => *caret,
            _ => return false,
        };
        let mut pos = cur;
        if let Some(Kind::Input { text, .. }) = self.get(id).map(|n| &n.kind) {
            if delta < 0 {
                for _ in 0..delta.unsigned_abs() {
                    pos = prev_char_boundary(text, pos);
                }
            } else {
                for _ in 0..delta as usize {
                    pos = next_char_boundary(text, pos);
                }
            }
        }
        self.input_set_caret(id, pos, extend)
    }

    /// 设定光标位置（按 char 边界夹取）；`extend` = 保持锚点（扩选）
    pub fn input_set_caret(&mut self, id: NodeId, pos: usize, extend: bool) -> bool {
        let Some(n) = self.get_mut(id) else {
            return false;
        };
        let Kind::Input {
            text, caret, anchor, ..
        } = &mut n.kind
        else {
            return false;
        };
        let pos = clamp_to_char_boundary(text, pos);
        let changed = *caret != pos || (!extend && *anchor != pos);
        *caret = pos;
        if !extend {
            *anchor = pos;
        }
        if changed {
            self.mark_paint_dirty(id);
            self.input_ensure_caret_visible(id);
        }
        changed
    }

    /// 全选
    pub fn input_select_all(&mut self, id: NodeId) -> bool {
        let Some(n) = self.get_mut(id) else {
            return false;
        };
        let Kind::Input {
            text, caret, anchor, ..
        } = &mut n.kind
        else {
            return false;
        };
        let len = text.len();
        let changed = *caret != len || *anchor != 0;
        *caret = len;
        *anchor = 0;
        if changed {
            self.mark_paint_dirty(id);
        }
        changed
    }

    /// IME：设置组合串（未提交）。空串 = 结束组合。
    pub fn input_set_preedit(&mut self, id: NodeId, text: String) -> bool {
        let Some(n) = self.get_mut(id) else {
            return false;
        };
        let Kind::Input { preedit, .. } = &mut n.kind else {
            return false;
        };
        if *preedit == text {
            return false;
        }
        *preedit = text;
        self.mark_paint_dirty(id);
        true
    }

    /// 读 IME 组合串
    pub fn input_preedit(&self, id: NodeId) -> Option<&str> {
        match self.get(id).map(|n| &n.kind) {
            Some(Kind::Input { preedit, .. }) => Some(preedit.as_str()),
            _ => None,
        }
    }

    /// 输入框是否处于编辑状态（有焦点 + 可用）
    pub fn input_is_active(&self, id: NodeId) -> bool {
        self.focused == Some(id)
            && self
                .get(id)
                .map(|n| n.interaction.enabled && matches!(n.kind, Kind::Input { .. }))
                .unwrap_or(false)
    }

    fn input_binding(&self, id: NodeId) -> Option<crate::reactive::Signal<String>> {
        self.get(id).and_then(|n| n.bindings.text.clone())
    }

    /// 编辑收尾：标脏 + 写回绑定
    fn input_finish(
        &mut self,
        id: NodeId,
        layout_may_change: bool,
        binding: Option<crate::reactive::Signal<String>>,
        text: String,
    ) {
        if layout_may_change {
            // 输入框一般有固定尺寸 ⇒ 冒泡到自身即止；万一用户让它自适应也正确
            self.mark_layout_dirty(id);
        }
        self.mark_paint_dirty(id);
        if let Some(sig) = binding {
            sig.set(text);
        }
    }

    /// 水平滚动钳制：让光标始终落在可视区内（文本超宽时用）。
    ///
    /// 可视窗口是 `[scroll, scroll + inner]`（inner = 宽 - 左右内边距），
    /// 光标在 `caret_x`（前缀实测宽）⇒ `scroll ∈ [caret_x - inner, caret_x]`。
    fn input_ensure_caret_visible(&mut self, id: NodeId) {
        let (rect, pad_l, pad_r, caret_px, old_scroll) = {
            let Some(n) = self.get(id) else {
                return;
            };
            let Kind::Input {
                text, caret, scroll, ..
            } = &n.kind
            else {
                return;
            };
            let prefix = clamp_to_char_boundary(text, *caret);
            let caret_px = TextEngine::measure_text(&text[..prefix], &n.text.spec).0 as f32;
            (
                n.rect(),
                n.layout.padding[lieui_layout::CSSDirection::Left as usize],
                n.layout.padding[lieui_layout::CSSDirection::Right as usize],
                caret_px,
                *scroll,
            )
        };
        let inner = (rect.width - pad_l - pad_r).max(0.0);
        let new_scroll = old_scroll.clamp((caret_px - inner).max(0.0), caret_px.max(0.0));
        if (new_scroll - old_scroll).abs() > 0.01
            && let Some(n) = self.get_mut(id)
            && let Kind::Input { scroll, .. } = &mut n.kind
        {
            *scroll = new_scroll;
            self.mark_paint_dirty(id);
        }
    }

    /// 拖动状态（滑块等）
    pub fn set_dragging(&mut self, id: NodeId, v: bool) -> bool {
        let changed = match self.get_mut(id) {
            Some(n) => match &mut n.kind {
                Kind::Slider { dragging, .. } => {
                    let c = *dragging != v;
                    *dragging = v;
                    c
                }
                _ => false,
            },
            None => false,
        };
        if changed {
            self.mark_paint_dirty(id);
        }
        changed
    }

    /// 指针捕获（多指针；同一指针只保留最后一次捕获）
    pub fn capture_pointer(&mut self, pointer: PointerId, id: NodeId) {
        self.captures.retain(|(p, _)| *p != pointer);
        self.captures.push((pointer, id));
    }

    pub fn release_pointer(&mut self, pointer: PointerId) -> Option<NodeId> {
        let prev = self.captures.iter().find(|(p, _)| *p == pointer).map(|(_, id)| *id);
        self.captures.retain(|(p, _)| *p != pointer);
        prev
    }

    pub fn captured_by(&self, pointer: PointerId) -> Option<NodeId> {
        self.captures.iter().find(|(p, _)| *p == pointer).map(|(_, id)| *id)
    }

    // ── 脏标志与脏区 ──

    pub fn mark_flags(&mut self, id: NodeId, f: Flags) {
        if let Some(n) = self.get_mut(id) {
            n.flags.insert(f);
        }
    }

    /// 追加一个"内置行为"处理器（`handled_events_too = true`）：
    /// 即便用户处理器已 `mark_handled`，它仍然会被调用。
    ///
    /// 用途：框架内置交互（滑块拖拽、文本编辑、滚动条……）在 `align` 之后挂到节点上；
    /// 它们不出现在用户态，也不需要 `view()` 重新声明。
    pub fn add_builtin_handler(
        &mut self,
        id: NodeId,
        kind: crate::event::EventKind,
        f: impl Fn(&mut crate::event::Ctx) + 'static,
    ) {
        if let Some(n) = self.get_mut(id) {
            n.handlers.push(HandlerSlot {
                kind,
                handler: std::rc::Rc::new(f),
                handled_events_too: true,
            });
        }
    }

    /// 是否有节点需要重排（M1 的帧驱动用它决定 `LAYOUT` 标志）
    pub fn has_layout_dirty(&self) -> bool {
        self.nodes
            .iter()
            .flatten()
            .any(|n| n.flags.contains(Flags::MEASURE_DIRTY) || n.flags.contains(Flags::ARRANGE_DIRTY))
    }

    /// 标记"需要重绘"，并把该节点**当前**的绘制范围并入脏区
    pub fn mark_paint_dirty(&mut self, id: NodeId) {
        self.mark_flags(id, Flags::PAINT_DIRTY);
        if let Some(r) = self.damage_bounds(id) {
            self.damage_rect(r);
        }
    }

    /// 该节点绘制影响的**窗口空间**矩形：含自身与祖先的变换，文本只算内容范围。
    ///
    /// （渲染层按 `Transform` 画到别处时，脏区必须跟过去，否则会留残影。）
    pub fn damage_bounds(&self, id: NodeId) -> Option<Rect> {
        let mut acc = crate::transform::Affine::IDENTITY;
        let mut cur = Some(id);
        while let Some(c) = cur {
            // 祖先链中途断开（节点刚被释放 / parent 悬空）时，**带着已累积的变换继续**，
            // 而不是整条放弃 —— 后者会让这次脏区登记静默消失，症状同样是残影（D57）。
            let Some(n) = self.get(c) else { break };
            acc = acc.then(n.transform.matrix(n.rect()));
            cur = n.parent;
        }
        let r = acc.bounding_box(self.get(id)?.paint_bounds());
        if r.width > 0.0 && r.height > 0.0 { Some(r) } else { None }
    }

    /// 标记尺寸/位置可能变化，并向祖先冒泡到**重排边界**（设计 §3.6）。
    ///
    /// 边界判据：节点自身宽高都已确定（`dim` 两轴 defined）⇒ 其尺寸不受子树影响，
    /// 子树重排不会改变它的尺寸，冒泡到此为止。这样 `layout()` 只需重排"最上层脏节点"的子树，
    /// 而不是整窗（对比旧实现的全表 `has_dirty_node()` + 全量重建 flex 树）。
    pub fn mark_layout_dirty(&mut self, id: NodeId) {
        let epoch = self.layout_epoch;
        let mut cur = Some(id);
        while let Some(c) = cur {
            let parent = {
                let Some(n) = self.get_mut(c) else { break };
                n.flags.insert(Flags::MEASURE_DIRTY | Flags::ARRANGE_DIRTY);
                n.layout_epoch = epoch; // ★ 记录"哪一轮提的"（D58）
                if size_stable(&n.layout) {
                    // 自身尺寸确定 ⇒ 它就是边界，不再向上冒泡
                    break;
                }
                n.parent
            };
            cur = parent;
        }
        // 布局变化必然带来绘制变化
        self.mark_flags(id, Flags::PAINT_DIRTY);
    }

    /// 开始新一轮布局（`layout()` 在动手之前调用）。
    ///
    /// 递增 epoch ⇒ 本轮期间新提的脏标能被 [`Self::clear_layout_flags`] 区分出来并保留。
    pub(crate) fn begin_layout_epoch(&mut self) {
        self.layout_epoch = self.layout_epoch.wrapping_add(1);
    }

    /// 标记"节点在父的**流**里发生了变化"（尺寸改变 / 增删 / 可见性变化 / FlexStyle 变化）。
    ///
    /// 与 [`Track::mark_layout_dirty`] 的区别：后者是"我自己的尺寸可能变了"，
    /// 对**尺寸确定**的节点而言不影响兄弟；而"我在流里变了"（被收起、被移走、margin/gap 变了）
    /// 一定会让兄弟重排，所以要连父节点一起标脏。
    pub fn mark_flow_dirty(&mut self, id: NodeId) {
        self.mark_layout_dirty(id);
        if let Some(p) = self.parent_of(id) {
            self.mark_layout_dirty(p);
        }
    }

    /// 整窗重排（窗口尺寸变化 / 主题字体变化时用）：无条件标记所有节点
    pub fn mark_all_layout_dirty(&mut self) {
        for n in self.nodes.iter_mut().flatten() {
            n.flags
                .insert(Flags::MEASURE_DIRTY | Flags::ARRANGE_DIRTY | Flags::PAINT_DIRTY);
        }
        self.damage_whole_window();
    }

    /// 布局结束后清除重排标记（`layout()` 消费完自己的义务）
    pub fn clear_layout_flags(&mut self) {
        let cur = self.layout_epoch;
        for n in self.nodes.iter_mut().flatten() {
            // ★ 只清"本轮之前"提的标记（D58）：布局**过程中**新提的
            //   （`write_back` 触发 `mark_layout_dirty` 等）属于下一轮的活儿，
            //   一起清掉就漏失效了 ⇒ 画面停在旧布局且下一帧也不再重排。
            if n.layout_epoch < cur {
                n.flags.remove(Flags::MEASURE_DIRTY | Flags::ARRANGE_DIRTY);
            }
        }
    }

    /// 遍历所有存活节点的身份（arena 顺序）
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(move |(i, n)| n.as_ref().map(|_| NodeId::new(i as u32, self.gens[i])))
    }

    /// 需要重排的**最上层**节点（自身脏、父不脏）——`layout()` 的边界集合
    pub fn layout_boundaries(&self) -> Vec<NodeId> {
        let dirty = |id: NodeId| {
            self.get(id)
                .map(|n| n.flags.contains(Flags::ARRANGE_DIRTY))
                .unwrap_or(false)
        };
        self.node_ids()
            .filter(|id| dirty(*id) && self.parent_of(*id).map(|p| !dirty(p)).unwrap_or(true))
            .collect()
    }

    pub fn damage_rect(&mut self, r: Rect) {
        if r.width > 0.0 && r.height > 0.0 {
            self.damage.push(r);
        }
    }

    pub fn damage_whole_window(&mut self) {
        self.damage_all = true;
    }

    pub fn take_damage(&mut self) -> (Vec<Rect>, bool) {
        (std::mem::take(&mut self.damage), std::mem::take(&mut self.damage_all))
    }
}

impl std::fmt::Debug for Track {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Track")
            .field("nodes", &self.len())
            .field("roots", &self.roots)
            .finish()
    }
}

/// 节点自身尺寸是否确定（宽高都 defined）⇒ 可作重排边界
fn size_stable(style: &FlexStyle) -> bool {
    lieui_layout::is_defined(style.dim[0]) && lieui_layout::is_defined(style.dim[1])
}

/// 祖先迭代器（自身 → 根，不含自身）
pub struct Ancestors<'a> {
    track: &'a Track,
    cursor: Option<NodeId>,
}

impl Iterator for Ancestors<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<Self::Item> {
        let cur = self.cursor?;
        self.cursor = self.track.parent_of(cur);
        Some(cur)
    }
}

#[cfg(test)]
#[path = "track_tests.rs"]
mod tests;
