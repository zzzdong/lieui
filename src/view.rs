//! 描述 arena（`ViewBuf`）：用户态 `view()` 的**输出**。
//!
//! 设计要点（`docs/architecture-v3.md` §3.3 / §3.4 / §3.15）：
//! - 描述是**某时刻的数据**，由框架分配、跨次复用（`begin()` 只重置游标），`align` 消费完即弃；
//! - 描述里**没有 `NodeId`**：用户拿不到树身份，也就无法绕过对齐去改树；
//! - 样式方法作用在"当前容器"（进入容器闭包后即为该容器），叶子创建返回 [`DescRef`] 以便链式设样式；
//! - 层（`modal` / `overlay` / `popup_at` / …）在**当前层内部声明**，`DescRoot.owner` 记录父子关系
//!   ⇒ z 由 (layer, 嵌套深度, 声明序号) 决定，父层消失则嵌套子层一起消失。

use std::rc::Rc;

use lieui_geom::{Color, Point, Rect};
use lieui_layout::{CSSDirection, Dimension, FlexAlign, FlexDirection, FlexStyle};

use crate::event::{EventKind, HandlerSlot};
use crate::reactive::Signal;
use crate::style::{ImageStyle, PaintStyle, ShadowSpec, TextStyle};
use crate::theme::Theme;
use crate::track::{Anchor, Bindings, ImageData, Key, KindDesc, Layer, LayerOpts, Placement, Transform, Visibility};

/// 描述节点（`pub(crate)`：用户只通过 [`ViewBuf`] / [`DescRef`] 操作）
pub(crate) struct DescNode {
    pub kind: KindDesc,
    pub key: Option<Key>,
    pub tooltip: Option<String>,
    pub layout: FlexStyle,
    pub paint: PaintStyle,
    pub text: TextStyle,
    pub image: ImageStyle,
    pub visibility: Visibility,
    pub hit_test_visible: bool,
    pub enabled: bool,
    pub clip: Option<Rect>,
    pub transform: Transform,
    pub tab_stop: bool,
    pub tab_index: i32,
    pub handlers: Vec<HandlerSlot>,
    /// 挂在本节点上的右键菜单（[`crate::menu::ContextMenu`]；`None` = 无）。
    /// 由框架的"上下文菜单会话"读取：右键命中链里最深带它的节点胜出。
    pub context_menu: Option<crate::menu::ContextMenu>,
    /// 双向绑定（对齐时覆盖到节点，**不**置脏）
    pub bindings: Bindings,
    pub children: Vec<u32>,
    /// 与 `children` 等长；仅在 `keyed_list` 使用时有 `Some`
    pub child_keys: Vec<Option<Key>>,
}

impl DescNode {
    fn new(kind: KindDesc) -> Self {
        Self {
            kind,
            key: None,
            tooltip: None,
            context_menu: None,
            layout: FlexStyle::default(),
            paint: PaintStyle::default(),
            text: TextStyle::default(),
            image: ImageStyle::default(),
            visibility: Visibility::Visible,
            hit_test_visible: true,
            enabled: true,
            clip: None,
            transform: Transform::default(),
            tab_stop: false,
            tab_index: 0,
            handlers: Vec::new(),
            bindings: Bindings::default(),
            children: Vec::new(),
            child_keys: Vec::new(),
        }
    }
}

/// 虚拟列表的视图态：记住"可见窗口起点"（item 下标）。
///
/// `Clone` 只是 `Rc` 引用计数 +1（可以放进多个 VM / 多处传阅）。
/// 内部用 `Signal` 承载 —— 起点一变就触发 `view()` 重跑（R1 的既有回路）。
#[derive(Clone)]
pub struct VirtualListState {
    first: Signal<usize>,
}

impl VirtualListState {
    /// 造一个（需要 `Runtime`，与 `Signal` 一样）
    pub fn new(rt: &crate::reactive::Runtime) -> Self {
        Self {
            first: Signal::new(rt, 0),
        }
    }

    /// 当前窗口起点（item 下标）
    pub fn first(&self) -> usize {
        self.first.get()
    }

    /// 直接跳到某个 item（例如"定位到第 N 项"；越界由 [`ViewBuf::virtual_list`] 收敛）
    pub fn set_first(&self, i: usize) {
        self.first.set(i);
    }

    /// 给 `virtual_list` 注册 `ScrollChanged` 用
    fn signal(&self) -> Signal<usize> {
        self.first.clone()
    }
}

impl std::fmt::Debug for VirtualListState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualListState")
            .field("first", &self.first())
            .finish()
    }
}

/// 描述层根（`pub(crate)`）
pub(crate) struct DescRoot {
    pub node: u32,
    pub layer: Layer,
    /// 声明它的父层（`DescRoot` 下标；`None` = Content）
    pub owner: Option<u32>,
    pub opts: LayerOpts,
    /// 层根标签：布局后可用 `Track::root_by_tag` 找回这个层
    /// （框架的 loading 遮罩靠它定位，用户也能给自己的层打标签）。
    pub tag: Option<u64>,
}

/// 描述 arena：一次 `view()` 的产出
#[derive(Default)]
pub struct ViewBuf {
    pub(crate) nodes: Vec<DescNode>,
    pub(crate) roots: Vec<DescRoot>,
    /// 容器栈（"当前容器"；样式糖作用在它上面）
    stack: Vec<u32>,
    /// 层根栈（当前层）
    root_stack: Vec<u32>,
    /// `keyed_list` 正在渲染 item（允许往"被 keyed 接管"的容器里推子节点）
    in_keyed_item: bool,
    /// 下一个层根的标签（`modal_tagged` 用）
    layer_tag: Option<u64>,
    /// 本帧的主题快照（框架在 `view()` 前注入；DSL 用它烘焙默认配色，设计 §3.10）
    theme: Theme,
}

impl ViewBuf {
    pub fn new() -> Self {
        Self::default()
    }

    /// 本帧的主题快照（DSL 默认配色的来源；自定义默认色可用它）
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// 给"当前容器/层根"挂事件处理器。
    ///
    /// 典型用途：在弹层闭包内给层根挂 `Dismissed`（轻关闭 ⇒ 翻自己的状态）。
    pub fn on(&mut self, kind: EventKind, f: impl Fn(&mut crate::event::Ctx) + 'static) {
        let d = self.cur();
        d.on(kind, f);
    }

    /// 上下文菜单：挂到**当前容器**上（[`DescRef::context_menu`] 的样式糖版本）。
    ///
    /// 在 `container(|c| ..)` / `row(|c| ..)` 闭包里给容器挂菜单时用这个 ——
    /// 闭包参数是 `&mut ViewBuf`，拿不到 `DescRef`：
    ///
    /// ```ignore
    /// v.container(|row| {
    ///     row.text("第 1 页");
    ///     row.context_menu(|m| { m.item("复制").on_tap(..); });
    /// });
    /// ```
    pub fn context_menu(&mut self, f: impl Fn(&mut crate::menu::MenuRef<'_>) + 'static) {
        self.cur().context_menu(f);
    }

    pub(crate) fn set_theme(&mut self, t: Theme) {
        self.theme = t;
    }

    /// 描述 arena 复用：只重置游标（不清 `Vec` 的容量）
    pub fn begin(&mut self) {
        self.nodes.clear();
        self.roots.clear();
        self.stack.clear();
        self.root_stack.clear();
        self.in_keyed_item = false;
    }

    pub(crate) fn node(&self, idx: u32) -> &DescNode {
        &self.nodes[idx as usize]
    }

    fn node_mut(&mut self, idx: u32) -> &mut DescNode {
        &mut self.nodes[idx as usize]
    }

    fn alloc(&mut self, kind: KindDesc) -> u32 {
        self.nodes.push(DescNode::new(kind));
        (self.nodes.len() - 1) as u32
    }

    /// 把新节点挂到"当前容器"；若当前没有容器，则作为窗口的内容根。
    fn push_desc(&mut self, kind: KindDesc) -> u32 {
        let idx = self.alloc(kind);
        match self.stack.last().copied() {
            Some(parent) => {
                debug_assert!(
                    !self.node(parent).child_keys.iter().any(|k| k.is_some()) || self.in_keyed_item,
                    "该容器已被 keyed_list 接管，不能在它里面直接再加子节点"
                );
                let n = self.node_mut(parent);
                n.children.push(idx);
                if !n.child_keys.is_empty() {
                    n.child_keys.push(None);
                }
            }
            None => {
                debug_assert!(
                    self.roots.iter().all(|r| r.layer != Layer::Content),
                    "内容根只能声明一次：顶层第二个声明请改用层（modal/overlay/popup_at/…）"
                );
                let rid = self.roots.len() as u32;
                self.roots.push(DescRoot {
                    node: idx,
                    layer: Layer::Content,
                    owner: None,
                    opts: LayerOpts::for_layer(Layer::Content),
                    tag: None,
                });
                self.root_stack.push(rid);
            }
        }
        idx
    }

    /// 当前容器（供样式糖使用）
    fn cur(&mut self) -> DescRef<'_> {
        match self.stack.last().copied() {
            Some(idx) => DescRef { v: self, idx },
            None => panic!("样式糖必须在容器闭包内使用（v.column(|c| c.gap(8.0))）"),
        }
    }

    // ─────────────────── 容器 ───────────────────

    pub fn column(&mut self, f: impl FnOnce(&mut Self)) {
        let idx = self.push_desc(KindDesc::Box);
        self.node_mut(idx).layout.flex_direction = FlexDirection::Column;
        self.with_container(idx, f);
    }

    pub fn row(&mut self, f: impl FnOnce(&mut Self)) {
        let idx = self.push_desc(KindDesc::Box);
        self.node_mut(idx).layout.flex_direction = FlexDirection::Row;
        self.with_container(idx, f);
    }

    /// 块级容器（纵向、不扩展）
    pub fn container(&mut self, f: impl FnOnce(&mut Self)) {
        let idx = self.push_desc(KindDesc::Box);
        self.node_mut(idx).layout.flex_direction = FlexDirection::Column;
        self.with_container(idx, f);
    }

    /// [`Self::container`] + 返回该容器自己的元素引用。
    ///
    /// 写**组合构件**（菜单项、卡片行、可点击列表行……）时需要：闭包里造完子节点后，
    /// 还要给**容器本身**设 `hover_background` / 挂 `on_tap` / 标 `disabled`。
    /// `container` 返回 `()`，那些设置只能落在"当前容器"上（也就是闭包内），
    /// 于是组合件只能自己建节点 —— 这扇门就是给它们用的。
    ///
    /// ```ignore
    /// // 行：容器自己带 hover 底色与点击，点哪都算
    /// let row = v.container_ref(|row| {
    ///     row.text("第 1 页");
    /// });
    /// row.radius(4.0).hover_background(t.control_hover).on_tap(|| println!("hi"));
    /// ```
    pub fn container_ref(&mut self, f: impl FnOnce(&mut Self)) -> DescRef<'_> {
        let idx = self.push_desc(KindDesc::Box);
        self.node_mut(idx).layout.flex_direction = FlexDirection::Column;
        self.with_container(idx, f);
        DescRef { v: self, idx }
    }

    /// 滚动容器（`overflow_scroll` + 裁剪）
    pub fn scroll(&mut self, f: impl FnOnce(&mut Self)) {
        let idx = self.push_desc(KindDesc::Box);
        {
            let n = self.node_mut(idx);
            n.layout.flex_direction = FlexDirection::Column;
            n.layout.overflow_scroll = true;
            n.layout.show_scrollbar = true;
            n.paint.clip_content = true;
        }
        self.with_container(idx, f);
    }

    /// 占位/撑开（`flex_grow: 1`）
    pub fn spacer(&mut self) {
        let idx = self.push_desc(KindDesc::Box);
        self.node_mut(idx).layout.flex_grow = 1.0;
    }

    fn with_container(&mut self, idx: u32, f: impl FnOnce(&mut Self)) {
        self.stack.push(idx);
        f(self);
        self.stack.pop();
    }

    // ─────────────────── 叶子 ───────────────────

    pub fn text(&mut self, s: impl Into<String>) -> DescRef<'_> {
        let idx = self.push_desc(KindDesc::Text(s.into()));
        // 默认文字色随主题烘焙（`.color(..)` 链式覆盖仍可赢）
        self.node_mut(idx).text.color = self.theme().text;
        DescRef { v: self, idx }
    }

    pub fn button(&mut self, label: &str) -> DescRef<'_> {
        let idx = self.push_desc(KindDesc::Button {
            label: label.to_string(),
        });
        // 开箱可用：主题烘焙的底色/hover/pressed + 圆角（`.background(..)` 链式覆盖仍可赢）
        self.bake_button_style(idx);
        DescRef { v: self, idx }
    }

    /// 图标（**Material Icons** 字体；`name` 见 codepoints 表，如 `"close"`/`"settings"`）。
    ///
    /// 本质是"图标字体里的一个字符"——测度/布局/绘制/hover 变色全部复用文本管线，
    /// 字体在首次使用时自动注册（`crate::icon`）。字号默认 20，链式 `.font_size(..)` 可改。
    pub fn icon(&mut self, name: impl AsRef<str>) -> DescRef<'_> {
        let idx = self.push_desc(KindDesc::Text(crate::icon::icon_char(name.as_ref()).to_string()));
        {
            let t = *self.theme();
            let n = self.node_mut(idx);
            n.text.color = t.text;
            n.text.spec = crate::icon::icon_spec(20.0);
        }
        DescRef { v: self, idx }
    }

    /// 图标按钮：icon 字形 + 按钮交互（`.on_tap` 等与 [`Self::button`] 相同）。
    pub fn icon_button(&mut self, name: impl AsRef<str>) -> DescRef<'_> {
        let idx = self.push_desc(KindDesc::Button {
            label: crate::icon::icon_char(name.as_ref()).to_string(),
        });
        self.bake_button_style(idx);
        self.node_mut(idx).text.spec = crate::icon::icon_spec(20.0);
        DescRef { v: self, idx }
    }

    /// `button` / `icon_button` 共享的主题烘焙（底色/hover/pressed + 圆角 + 文字色 + 内边距）
    fn bake_button_style(&mut self, idx: u32) {
        let t = *self.theme();
        let n = self.node_mut(idx);
        n.paint = PaintStyle::new()
            .background(t.control)
            .hover_background(t.control_hover)
            .pressed_background(t.control_pressed)
            .radius(t.control_radius);
        n.text.color = t.text;
        n.layout = n
            .layout
            .clone()
            .padding_left(10.0)
            .padding_right(10.0)
            .padding_top(6.0)
            .padding_bottom(6.0);
    }

    /// 复选框：默认 18×18（没有文本可测度 ⇒ 否则尺寸为 0）
    pub fn checkbox(&mut self, checked: bool) -> DescRef<'_> {
        let idx = self.push_desc(KindDesc::Checkbox { checked });
        {
            let n = self.node_mut(idx);
            n.layout = n.layout.clone().width(18.0).height(18.0);
        }
        DescRef { v: self, idx }
    }

    /// 滑块：默认 140×20、范围 `0..1`
    pub fn slider(&mut self, value: f32) -> DescRef<'_> {
        self.slider_range(value, 0.0, 1.0)
    }

    /// 滑块（带范围的只读形态）
    pub fn slider_range(&mut self, value: f32, min: f32, max: f32) -> DescRef<'_> {
        let idx = self.push_desc(KindDesc::Slider { value, min, max });
        {
            let n = self.node_mut(idx);
            n.layout = n.layout.clone().width(140.0).height(20.0);
        }
        DescRef { v: self, idx }
    }

    /// 滑块（**双向绑定**）：拖动会写回 `sig`（≈ `v-model`）
    pub fn slider_bound(&mut self, sig: &Signal<f32>, min: f32, max: f32) -> DescRef<'_> {
        let value = sig.get().clamp(min, max);
        let idx = self.push_desc(KindDesc::Slider { value, min, max });
        {
            let n = self.node_mut(idx);
            n.layout = n.layout.clone().width(140.0).height(20.0);
            n.bindings.value = Some(sig.clone());
        }
        DescRef { v: self, idx }
    }

    /// 文本输入框：默认 200×28、可 Tab 聚焦、白底 + 边框
    pub fn input(&mut self, value: &str) -> DescRef<'_> {
        let idx = self.push_desc(KindDesc::Input {
            text: value.to_string(),
            placeholder: String::new(),
        });
        self.style_input(idx);
        DescRef { v: self, idx }
    }

    /// 文本输入框（**双向绑定**）：编辑会写回 `sig`（≈ `v-model`）
    pub fn input_bound(&mut self, sig: &Signal<String>) -> DescRef<'_> {
        let idx = self.push_desc(KindDesc::Input {
            text: sig.get(),
            placeholder: String::new(),
        });
        self.style_input(idx);
        self.node_mut(idx).bindings.text = Some(sig.clone());
        DescRef { v: self, idx }
    }

    /// 输入框的默认视觉（颜色随主题烘焙）
    fn style_input(&mut self, idx: u32) {
        let t = *self.theme();
        let n = self.node_mut(idx);
        n.layout = n
            .layout
            .clone()
            .width(200.0)
            .height(28.0)
            .padding_left(8.0)
            .padding_right(8.0)
            .padding_top(4.0)
            .padding_bottom(4.0);
        n.paint = PaintStyle::new()
            .background(t.input_background)
            .border(1.0, t.control_border)
            .radius(t.control_radius);
        n.text.color = t.text;
        n.tab_stop = true;
        // 文本超出盒子时裁掉（水平滚动留待后续）
        n.paint.clip_content = true;
    }

    /// 自定义节点（自绘 / 自定义行为）：实例跨帧保留，见 `custom::CustomNode`。
    ///
    /// 传入的应是 ViewModel 里持有的 `CustomCell`（同一 cell 的 `Rc` 指针相等
    /// ⇒ 对齐器视为"没变"）；每帧新建 cell 会被视为数据变化而替换实例。
    pub fn custom(&mut self, cell: &crate::custom::CustomCell) -> DescRef<'_> {
        let idx = self.push_desc(KindDesc::Custom(cell.clone()));
        DescRef { v: self, idx }
    }

    /// 开关：默认 40×20、可 Tab 聚焦
    pub fn switch(&mut self, on: bool) -> DescRef<'_> {
        let idx = self.push_desc(KindDesc::Switch { on });
        self.style_switch(idx);
        DescRef { v: self, idx }
    }

    /// 开关（**双向绑定**）：点击翻转并写回 `sig`
    pub fn switch_bound(&mut self, sig: &Signal<bool>) -> DescRef<'_> {
        let on = sig.get();
        let idx = self.push_desc(KindDesc::Switch { on });
        self.style_switch(idx);
        self.node_mut(idx).bindings.checked = Some(sig.clone());
        DescRef { v: self, idx }
    }

    /// 单选（组内互斥）：`selected` 是模型给出的值
    pub fn radio(&mut self, selected: bool) -> DescRef<'_> {
        let idx = self.push_desc(KindDesc::Radio {
            selected,
            value: String::new(),
        });
        self.style_radio(idx);
        DescRef { v: self, idx }
    }

    /// 单选（**双向绑定**）：点击把 `value` 写回组 signal（≈ `v-model` + 选项取值）。
    ///
    /// 同组互斥**不需要**框架遍历兄弟：信号变化 ⇒ `view()` 重跑 ⇒
    /// 同组所有 radio 的 `selected` 由对齐器自然更新。
    pub fn radio_bound(&mut self, sig: &Signal<String>, value: &str) -> DescRef<'_> {
        let selected = sig.get() == value;
        let idx = self.push_desc(KindDesc::Radio {
            selected,
            value: value.to_string(),
        });
        self.style_radio(idx);
        self.node_mut(idx).bindings.text = Some(sig.clone());
        DescRef { v: self, idx }
    }

    fn style_switch(&mut self, idx: u32) {
        let n = self.node_mut(idx);
        n.layout = n.layout.clone().width(40.0).height(20.0);
        n.tab_stop = true;
    }

    fn style_radio(&mut self, idx: u32) {
        let n = self.node_mut(idx);
        n.layout = n.layout.clone().width(18.0).height(18.0);
        n.tab_stop = true;
    }

    /// 复选框（**双向绑定**）：点击会写回 `sig`
    pub fn checkbox_bound(&mut self, sig: &Signal<bool>) -> DescRef<'_> {
        let checked = sig.get();
        let idx = self.push_desc(KindDesc::Checkbox { checked });
        {
            let n = self.node_mut(idx);
            n.layout = n.layout.clone().width(18.0).height(18.0);
            n.bindings.checked = Some(sig.clone());
        }
        DescRef { v: self, idx }
    }

    /// 进度条：默认 120×8
    pub fn progress(&mut self, value: f32) -> DescRef<'_> {
        let idx = self.push_desc(KindDesc::Progress { value });
        {
            let n = self.node_mut(idx);
            n.layout = n.layout.clone().width(120.0).height(8.0);
        }
        DescRef { v: self, idx }
    }

    pub fn image(&mut self, data: std::sync::Arc<ImageData>) -> DescRef<'_> {
        let idx = self.push_desc(KindDesc::Image(data));
        DescRef { v: self, idx }
    }

    // ─────────────────── 列表 ───────────────────

    /// 键控列表：**列表是唯一需要 key 匹配的地方**。
    ///
    /// 约束（M1）：它会接管当前容器的子节点，必须在该容器里独占使用；
    /// `item` 闭包必须恰好产生一个子节点。
    ///
    /// `key_fn` 收 `impl Fn`（不是 `fn` 指针）—— 这样它可以捕获环境
    /// （例如 [`Self::virtual_list`] 内部把 `fn(&T)` 适配成 `fn(&&T)`）。
    pub fn keyed_list<I, K, F>(&mut self, items: I, key_fn: impl Fn(&I::Item) -> K, mut item: F)
    where
        I: IntoIterator,
        K: Into<Key>,
        F: FnMut(&mut Self, &I::Item),
    {
        let Some(parent) = self.stack.last().copied() else {
            panic!("keyed_list 必须在容器闭包内使用");
        };
        {
            let n = self.node_mut(parent);
            debug_assert!(
                n.children.is_empty(),
                "keyed_list 会接管容器的子节点，请让该容器只放列表"
            );
            n.children.clear();
            n.child_keys.clear();
        }

        for it in items {
            let key: Key = key_fn(&it).into();
            let before = self.node(parent).children.len();
            self.in_keyed_item = true;
            item(self, &it);
            self.in_keyed_item = false;
            let after = self.node(parent).children.len();
            debug_assert_eq!(after, before + 1, "keyed_list 的 item 闭包必须恰好产生一个子节点");

            // key 同时记在：① 父节点的 child_keys（顺序对齐）；② 子节点自身（保留树侧按它匹配复用）
            let child = self.node(parent).children[after - 1];
            self.node_mut(child).key = Some(key.clone());
            let n = self.node_mut(parent);
            if n.child_keys.len() < n.children.len() {
                n.child_keys.resize(n.children.len(), None);
            }
            n.child_keys[after - 1] = Some(key);
        }
    }

    /// **虚拟列表**：只物化可见窗口的行（滚动容器内使用）。
    ///
    /// 与 [`Self::keyed_list`] 的分工：`keyed_list` 管**身份复用**，本方法管**窗口切片**
    /// —— 两者配合才能在滚动时既省节点又保住行内状态（选中、输入框、展开态……）。
    ///
    /// 机制：
    /// 1. [`VirtualListState`] 记住窗口起点：`ScrollChanged` 时把偏移换算成 item 下标回写
    ///    （只在**跨行**时才写，避免每帧无谓重跑 `view()`）；
    /// 2. 内容列用 `padding_top = first × item_h` / `padding_bottom = 余量 × item_h`
    ///    撑出**真实内容总高**（滚动范围与滚动条因此"诚实"），不需要插入空占位节点；
    /// 3. 可见窗口 `[first, last)`（多一行缓冲）走 `keyed_list`，按 `key_fn` 复用节点。
    ///
    /// 约束：必须声明在**滚动容器**内（本方法接管当前容器的子节点）；
    /// `item` 闭包必须恰好产生一个子节点（同 `keyed_list`）。
    ///
    /// `viewport_height` 需要显式给：`view()` 发生在布局**之前**，拿不到容器实测高度；
    /// 填滚动容器的固定高度即可（高度是弹性的场合，填保守值或上一帧量到的值）。
    ///
    /// ```ignore
    /// // VM：items: Vec<Row>、vl: VirtualListState
    /// v.scroll(|s| {
    ///     s.height(240.0).width(160.0);
    ///     s.virtual_list(&self.vl, &self.items, |r| r.id, 24.0, 240.0, |v, r| {
    ///         v.row(|row| {
    ///             row.height(24.0);
    ///             row.text(format!("#{} {}", r.id, r.title));
    ///         });
    ///     });
    /// });
    /// ```
    pub fn virtual_list<T, K, F>(
        &mut self,
        state: &VirtualListState,
        items: &[T],
        key_fn: fn(&T) -> K,
        item_height: f32,
        viewport_height: f32,
        item: F,
    ) where
        K: Into<Key>,
        F: FnMut(&mut Self, &T),
    {
        let count = items.len();
        let ih = if item_height.is_finite() && item_height > 0.0 {
            item_height
        } else {
            1.0
        };
        let vh = if viewport_height.is_finite() && viewport_height > 0.0 {
            viewport_height
        } else {
            ih
        };
        // 可见行数：满屏行数 + 1 行缓冲（滚动中不会看到空白；也容忍取整误差）
        let visible = ((vh / ih).ceil() as usize).max(1) + 1;
        let first = state.first().min(count);
        let last = (first + visible).min(count);

        // ① 滚动偏移 → 窗口起点
        //
        // 处理器挂在**最近的滚动容器祖先**上，而不是当前节点：调用点通常把虚拟列表
        // 包在普通 column 里（最常见的"标题 + 列表"排布），挂到当前节点会永远收不到
        // `ScrollChanged` ⇒ 窗口起点不推进 ⇒ 看起来"只有前若干项、滚下去没有新行"。
        // 没有滚动祖先时退回当前节点（等价于旧行为）。
        let cur = *self.stack.last().expect("virtual_list 必须在容器闭包内调用");
        let host = self
            .stack
            .iter()
            .rev()
            .find(|&&idx| self.node(idx).layout.overflow_scroll)
            .copied()
            .unwrap_or(cur);
        let sig = state.signal();
        self.node_mut(host).handlers.push(HandlerSlot {
            kind: EventKind::ScrollChanged,
            handler: Rc::new(move |cx: &mut crate::event::Ctx| {
                // 向下取整：保证"视口顶部露出半个行"时那一行也被物化（否则顶部会缺一块）
                let next = (cx.scroll_offset().1 / ih).floor().max(0.0) as usize;
                if sig.get() != next {
                    sig.set(next);
                }
            }),
            handled_events_too: false,
        });

        // ② 内容列（虚拟 padding 撑高）+ ③ 可见窗口（keyed 复用）
        let mut item = item;
        self.column(|col| {
            col.padding_top(first as f32 * ih);
            col.padding_bottom((count - last) as f32 * ih);
            col.keyed_list(items[first..last].iter(), |t: &&T| key_fn(*t), |v, t: &&T| item(v, *t));
        });
    }

    // ─────────────────── 层 ───────────────────

    /// Modal：默认 backdrop + 阻断下层（居中由层语义决定，不需要 anchor）
    pub fn modal(&mut self, f: impl FnOnce(&mut Self)) {
        self.layer(Layer::Modal, None, |_| {}, f);
    }

    /// 带标签的 Modal 层：语义同 [`Self::modal`]，且层根带 `tag`
    /// （布局后可用 `Track::root_by_tag(tag)` 找回）。框架的 loading 遮罩用它做定位。
    pub fn modal_tagged(&mut self, tag: u64, f: impl FnOnce(&mut Self)) {
        let prev = self.layer_tag;
        self.layer_tag = Some(tag);
        self.modal(f);
        self.layer_tag = prev;
    }

    /// 装饰层 / 水印：默认命中穿透
    pub fn overlay(&mut self, f: impl FnOnce(&mut Self)) {
        self.layer(Layer::Overlay, None, |_| {}, f);
    }

    /// 锚定浮层：`anchor` 是**节点 key**（布局后才解析成 rect）
    ///
    /// 位置与那一行的 rect 有关（`Placement::Below` = 行的左边缘下方），
    /// **与鼠标位置无关** —— 要"贴着右键点出现"请用 [`Self::popup_at_point`]。
    pub fn popup_at(&mut self, anchor_key: impl Into<Key>, placement: Placement, f: impl FnOnce(&mut Self)) {
        let key: Key = anchor_key.into();
        self.layer(
            Layer::Popup,
            None,
            |opts| {
                opts.anchor = Some(Anchor {
                    target: crate::track::AnchorTarget::Key(key),
                    placement,
                });
            },
            f,
        );
    }

    /// 锚定浮层到**某个逻辑坐标点** —— 右键菜单"贴着鼠标出现"的声明式用法。
    ///
    /// 与 [`Self::popup_at`] 只差锚点：一个是"某个节点的 rect"，一个是你给的点。
    /// **定位规则完全一致**（实现上把点当作零尺寸矩形），所以：
    ///
    /// - `Placement::Below` ⇒ 菜单左上角在点的下方 `ANCHOR_GAP`（4px）；
    /// - 点贴近下边缘 ⇒ 自动翻到点上方；贴近右边缘 ⇒ 自动平移回视口内
    ///   （不会跑出窗口 —— 这正是 [`Self::popup_at`] 里 `Fixed` 做不到的）；
    /// - 锚点**不需要有对应节点**，所以锚在虚拟列表行上也安全
    ///   （行被回收/滚出窗口都不会让菜单失去定位）。
    ///
    /// `pos` 是**逻辑坐标**（与布局、命中同一坐标系）：从窗口事件拿到的物理光标
    /// 位置要先除以 `scale`（见 [`crate::platform::physical_to_logical`]）。
    ///
    /// ```ignore
    /// // 右键时把光标位置（逻辑）存进状态，菜单按那个点声明
    /// if let Some(pos) = st.menu_at {
    ///     let me = Rc::clone(vm);
    ///     v.popup_at_point(pos, Placement::Below, |p| {
    ///         p.on(EventKind::Dismissed, move |_| me.close_menu());
    ///         // …菜单项
    ///     });
    /// }
    /// ```
    pub fn popup_at_point(&mut self, pos: Point, placement: Placement, f: impl FnOnce(&mut Self)) {
        self.layer(
            Layer::Popup,
            None,
            |opts| {
                opts.anchor = Some(Anchor {
                    target: crate::track::AnchorTarget::Point(pos),
                    placement,
                });
            },
            f,
        );
    }

    /// 锚定 tooltip
    pub fn tooltip_at(&mut self, anchor_key: impl Into<Key>, placement: Placement, f: impl FnOnce(&mut Self)) {
        let key: Key = anchor_key.into();
        self.layer(
            Layer::Tooltip,
            None,
            |opts| {
                opts.anchor = Some(Anchor {
                    target: crate::track::AnchorTarget::Key(key),
                    placement,
                });
            },
            f,
        );
    }

    /// **框架内部**：往描述树末尾追加一个"贴光标"的菜单弹层（`DescRef::context_menu` 会话用）。
    ///
    /// 为什么由框架"追加"而不是让 app 自己声明：菜单的**开关**本身就是框架状态
    /// （谁被右键了、光标在哪），app 侧只该声明"这个元素**有**菜单"。
    /// 追加进同一份描述树后，它就是一个普通的 `Layer::Popup` 根 ⇒ 对齐、脏区、
    /// 轻关闭、锚定落位全部复用现成 machinery（与 loading 遮罩同一手法）。
    ///
    /// 层根打上 [`crate::menu::CTX_MENU_TAG`] 标签，好让框架认回"这个弹层是我的"
    /// （app 自己声明的 popup 同在 `Layer::Popup` 组内，顺序会漂）。
    pub(crate) fn push_context_menu(&mut self, at: lieui_geom::Point, builder: &crate::menu::ContextMenu) {
        let prev = self.layer_tag;
        self.layer_tag = Some(crate::menu::CTX_MENU_TAG);
        self.layer(
            Layer::Popup,
            None,
            |opts| {
                opts.anchor = Some(Anchor {
                    target: crate::track::AnchorTarget::Point(at),
                    placement: Placement::Below,
                });
            },
            |p| p.menu(|m| builder(m)),
        );
        self.layer_tag = prev;
    }

    /// 拖拽预览（最高层）
    pub fn drag_preview(&mut self, f: impl FnOnce(&mut Self)) {
        self.layer(Layer::DragPreview, None, |_| {}, f);
    }

    /// 通用层入口：`owner` = 当前层（嵌套声明）
    fn layer(
        &mut self,
        layer: Layer,
        owner: Option<u32>,
        configure: impl FnOnce(&mut LayerOpts),
        f: impl FnOnce(&mut Self),
    ) {
        let owner = owner.or_else(|| self.root_stack.last().copied());
        let node = self.alloc(KindDesc::Box);
        let rid = self.roots.len() as u32;
        let mut opts = LayerOpts::for_layer(layer);
        configure(&mut opts);
        // Modal 遮罩走主题 token（`LayerOpts::for_layer` 里的默认值只是无主题时的兜底）
        if matches!(layer, Layer::Modal) {
            opts.backdrop = Some(self.theme().backdrop);
        }

        // 弹层根的默认视觉：不透明底 + 边框 + 投影 + 内边距。
        // 此前是全透明的 Box——弹层内容直接叠在下层内容上，几乎不可读（M6 反馈）。
        if matches!(layer, Layer::Popup | Layer::Tooltip) {
            let t = *self.theme();
            let n = self.node_mut(node);
            n.paint = PaintStyle::new()
                .background(t.input_background)
                .border(1.0, t.control_border)
                .radius(6.0)
                .shadow(ShadowSpec::new(0.0, 4.0, 12.0, 2.0, t.shadow));
            n.layout = n
                .layout
                .clone()
                .padding_left(6.0)
                .padding_right(6.0)
                .padding_top(6.0)
                .padding_bottom(6.0);
        }

        self.roots.push(DescRoot {
            node,
            layer,
            owner,
            opts,
            tag: self.layer_tag.take(),
        });

        self.root_stack.push(rid);
        self.stack.push(node);
        f(self);
        self.stack.pop();
        self.root_stack.pop();
    }

    /// 设置当前层根的可选项（如 `p.dismiss(...)` 对应的关闭语义由上层消息决定）
    pub fn layer_opts(&mut self, f: impl FnOnce(&mut LayerOpts)) {
        let Some(rid) = self.root_stack.last().copied() else {
            panic!("layer_opts 只能在层闭包内使用");
        };
        let mut opts = self.roots[rid as usize].opts.clone();
        f(&mut opts);
        self.roots[rid as usize].opts = opts;
    }

    // ─────────────────── 当前容器的样式糖（等价于 `self.cur().xxx(...)`）──
    //
    // 说明：显式写出来而不是宏生成 —— 目前只有十来个，宏会让调用点难读；
    // 若后续继续膨胀，再抽 `macro_rules!`。

    pub fn gap(&mut self, g: f32) {
        self.cur().gap(g);
    }
    /// 当前容器的布局逃生舱（与 [`DescRef::layout`] 对应；层根 / 容器闭包内用）
    pub fn layout(&mut self, f: impl FnOnce(&mut FlexStyle)) {
        self.cur().layout(f);
    }
    pub fn padding(&mut self, p: f32) {
        self.cur().padding(p);
    }
    pub fn padding_y(&mut self, p: f32) {
        self.cur().padding_y(p);
    }
    pub fn padding_top(&mut self, p: f32) {
        self.cur().padding_top(p);
    }
    pub fn padding_bottom(&mut self, p: f32) {
        self.cur().padding_bottom(p);
    }
    pub fn width(&mut self, w: f32) {
        self.cur().width(w);
    }
    pub fn height(&mut self, h: f32) {
        self.cur().height(h);
    }
    pub fn expand(&mut self, v: bool) {
        self.cur().expand(v);
    }
    pub fn center(&mut self) {
        self.cur().center();
    }
    pub fn align_items(&mut self, a: FlexAlign) {
        self.cur().align_items(a);
    }
    pub fn justify_content(&mut self, a: FlexAlign) {
        self.cur().justify_content(a);
    }
    pub fn background(&mut self, c: Color) {
        self.cur().background(c);
    }
    pub fn radius(&mut self, r: f32) {
        self.cur().radius(r);
    }
    pub fn border(&mut self, w: f32, c: Color) {
        self.cur().border(w, c);
    }
    pub fn clip(&mut self, v: bool) {
        self.cur().clip(v);
    }
    pub fn key(&mut self, k: impl Into<Key>) {
        self.cur().key(k);
    }
}

// ───────────────────────── 链式句柄 ─────────────────────────

/// 叶子 / 容器的链式样式句柄。
///
/// 持有 `&mut ViewBuf`，所以只能在**一条语句内**链式使用（用完即弃）：
/// ```ignore
/// c.text("hi").font_size(48.0).color(Color::RED);
/// ```
pub struct DescRef<'a> {
    v: &'a mut ViewBuf,
    idx: u32,
}

impl<'a> DescRef<'a> {
    fn n(&mut self) -> &mut DescNode {
        self.v.node_mut(self.idx)
    }

    // ── 通用逃生舱（任意样式字段）──

    pub fn layout(mut self, f: impl FnOnce(&mut FlexStyle)) -> Self {
        f(&mut self.n().layout);
        self
    }

    pub fn paint(mut self, f: impl FnOnce(&mut PaintStyle)) -> Self {
        f(&mut self.n().paint);
        self
    }

    pub fn text_style(mut self, f: impl FnOnce(&mut TextStyle)) -> Self {
        f(&mut self.n().text);
        self
    }

    // ── 布局糖 ──

    pub fn gap(mut self, g: f32) -> Self {
        self.n().layout.item_space = g;
        self
    }

    pub fn padding(mut self, p: f32) -> Self {
        let l = &mut self.n().layout;
        for i in 0..4 {
            l.padding[i] = p;
        }
        self
    }

    pub fn padding_y(mut self, p: f32) -> Self {
        let l = &mut self.n().layout;
        l.padding[CSSDirection::Top as usize] = p;
        l.padding[CSSDirection::Bottom as usize] = p;
        self
    }

    pub fn padding_top(mut self, p: f32) -> Self {
        self.n().layout.padding[CSSDirection::Top as usize] = p;
        self
    }

    pub fn padding_bottom(mut self, p: f32) -> Self {
        self.n().layout.padding[CSSDirection::Bottom as usize] = p;
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.n().layout.dim[Dimension::Width as usize] = w;
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.n().layout.dim[Dimension::Height as usize] = h;
        self
    }

    pub fn expand(mut self, v: bool) -> Self {
        self.n().layout.flex_grow = if v { 1.0 } else { 0.0 };
        self
    }

    pub fn center(mut self) -> Self {
        let l = &mut self.n().layout;
        l.justify_content = FlexAlign::Center;
        l.align_items = FlexAlign::Center;
        self
    }

    pub fn align_items(mut self, a: FlexAlign) -> Self {
        self.n().layout.align_items = a;
        self
    }

    pub fn justify_content(mut self, a: FlexAlign) -> Self {
        self.n().layout.justify_content = a;
        self
    }

    // ── 绘制糖 ──

    pub fn background(mut self, c: Color) -> Self {
        self.n().paint.background_color = Some(c);
        self
    }

    pub fn hover_background(mut self, c: Color) -> Self {
        self.n().paint.hover_background = Some(c);
        self
    }

    pub fn pressed_background(mut self, c: Color) -> Self {
        self.n().paint.pressed_background = Some(c);
        self
    }

    pub fn radius(mut self, r: f32) -> Self {
        self.n().paint.border_radius = r;
        self
    }

    pub fn border(mut self, w: f32, c: Color) -> Self {
        let p = &mut self.n().paint;
        p.border_width = w;
        p.border_color = Some(c);
        self
    }

    /// 输入框的空文本提示语
    pub fn placeholder(mut self, s: &str) -> Self {
        if let KindDesc::Input { placeholder, .. } = &mut self.n().kind {
            *placeholder = s.to_string();
        }
        self
    }

    pub fn shadow(mut self, s: ShadowSpec) -> Self {
        self.n().paint.shadow = Some(s);
        self
    }

    pub fn clip(mut self, v: bool) -> Self {
        self.n().paint.clip_content = v;
        self
    }

    // ── 文本糖 ──

    pub fn font_size(mut self, s: f64) -> Self {
        self.n().text.spec.font_size = s;
        self
    }

    pub fn color(mut self, c: Color) -> Self {
        self.n().text.color = c;
        self
    }

    pub fn hover_color(mut self, c: Color) -> Self {
        self.n().text.hover_color = Some(c);
        self
    }

    pub fn pressed_color(mut self, c: Color) -> Self {
        self.n().text.pressed_color = Some(c);
        self
    }

    pub fn font_family(mut self, f: impl Into<String>) -> Self {
        self.n().text.spec.font_family = f.into();
        self
    }

    pub fn text_align(mut self, a: lieui_text::TextAlign) -> Self {
        self.n().text.spec.text_align = a;
        self
    }

    pub fn wrap(mut self, v: bool) -> Self {
        self.n().text.spec.wrap = v;
        self
    }

    /// **光学对齐**：按墨迹盒（ink bounds）而不是行盒参与尺寸与居中。
    ///
    /// 行盒里 ascent/descent 不对称 ⇒ 可见字形视觉中心偏离行盒中心
    /// （实测：13px 拉丁文字偏 1.05px、CJK 偏 0.39px），而行高系数 1.0 的图标字体
    /// 墨迹**精确居中**于行盒。混排时按墨迹对齐才能让两者视觉中心重合。
    pub fn optical_align(mut self, v: bool) -> Self {
        self.n().text.spec.optical_align = v;
        self
    }

    // ── 可视 / 命中 ──

    pub fn visible(mut self, v: bool) -> Self {
        self.n().visibility = if v { Visibility::Visible } else { Visibility::Collapsed };
        self
    }

    pub fn hidden(mut self) -> Self {
        self.n().visibility = Visibility::Hidden;
        self
    }

    pub fn collapsed(mut self) -> Self {
        self.n().visibility = Visibility::Collapsed;
        self
    }

    pub fn hit_test_visible(mut self, v: bool) -> Self {
        self.n().hit_test_visible = v;
        self
    }

    pub fn enabled(mut self, v: bool) -> Self {
        self.n().enabled = v;
        self
    }

    /// 悬停提示：hover 该节点 [`crate::app::TOOLTIP_DELAY`] 后框架自动浮出 tooltip。
    pub fn tooltip(mut self, s: impl Into<String>) -> Self {
        self.n().tooltip = Some(s.into());
        self
    }

    /// **右键菜单**（对齐 WinUI `ContextFlyout`）：右键该节点时，框架在**光标处**弹出这个菜单。
    ///
    /// 与"自己管状态 + [`Self::popup_at_point`]"那套写法的区别：状态、触发、定位、关闭
    /// 全在框架里，应用**一行状态都不用加**。闭包在弹层被渲染时才跑，所以捕获到的
    /// `Rc` 状态总是当下的值。
    ///
    /// 框架负责的部分（对齐 `ContextFlyout` 的默认行为）：
    ///
    /// - **贴光标**出现（不是锚在节点左边缘），下方放不下自动翻上方、靠边自动平移回视口；
    /// - 命中链里**最深**带菜单的节点胜出（子元素的菜单优先于祖先的）；
    /// - 点菜单外 / 点别的行的右键 ⇒ 收起；**菜单项被点击后也收起**；
    /// - 目标节点被回收（列表滚走、页面关闭、该项不再声明菜单）⇒ 自动收起。
    ///
    /// 键盘调用（菜单键 / `Shift+F10`）与级联子菜单暂不支持 —— 见 [`crate::menu`] 模块文档。
    ///
    /// ```ignore
    /// v.column(|c| {
    ///     for row in rows {
    ///         c.text(&row.title).context_menu(|m| {
    ///             m.item("复制").on_tap_with(act(self, |s| s.copy(row.id)));
    ///             m.separator();
    ///             m.item("删除")
    ///                 .enabled(row.deletable)          // 禁用项不响应点击
    ///                 .accelerator("Del")
    ///                 .on_tap_with(act(self, |s| s.delete(row.id)));
    ///         });
    ///     }
    /// });
    /// ```
    pub fn context_menu(mut self, f: impl Fn(&mut crate::menu::MenuRef<'_>) + 'static) -> Self {
        self.n().context_menu = Some(std::rc::Rc::new(f));
        self
    }

    pub fn opacity(mut self, o: f32) -> Self {
        self.n().paint.opacity = o.clamp(0.0, 1.0);
        self
    }

    pub fn rotate(mut self, deg: f32) -> Self {
        self.n().transform.rotation_deg = deg;
        self
    }

    pub fn translate(mut self, x: f32, y: f32) -> Self {
        self.n().transform.translate = (x, y);
        self
    }

    pub fn clip_rect(mut self, r: Rect) -> Self {
        self.n().clip = Some(r);
        self
    }

    pub fn tab_stop(mut self, v: bool) -> Self {
        self.n().tab_stop = v;
        self
    }

    pub fn tab_index(mut self, i: i32) -> Self {
        self.n().tab_index = i;
        self
    }

    // ── 身份 / 事件 ──

    pub fn key(mut self, k: impl Into<Key>) -> Self {
        self.n().key = Some(k.into());
        self
    }

    /// 注册任意事件的处理器
    pub fn on(mut self, kind: EventKind, f: impl Fn(&mut crate::event::Ctx) + 'static) -> Self {
        self.n().handlers.push(HandlerSlot {
            kind,
            handler: Rc::new(f),
            handled_events_too: false,
        });
        self
    }

    /// 注册"即便已被处理也要调用"的处理器（≈ WinUI `handledEventsToo`）。
    ///
    /// 用户态一般不需要；框架内置行为也**不用**它（见下）。
    ///
    /// ## ★ 内置行为为什么不用这个机制
    ///
    /// 本方法（原`Track::add_builtin_handler` 的替代品）曾被设计成
    /// "框架内置交互在 `align` 之后挂成 `handled_events_too = true` 的 handler"。
    /// **实际实现走了另一条路**：内置交互是 `widgets::handle()` 里的**直接函数分派**
    /// （`src/widgets/mod.rs:51`，按 `KindTag` switch）。
    ///
    /// 原因是 handler 签名拿不到 `&Event` —— 只有 `&EventView`（`Copy` 的定长摘要），
    /// 而 **IME 预编辑/提交带字符串 payload**，`EventView` 装不下。
    /// `handle()` 直接收 `&Event` 才有办法把 payload 转成 `input_set_preedit` /
    /// `input_insert`。
    ///
    /// `Track::add_builtin_handler` 因此是设计变更的残留（长期零调用），
    /// 已于 2026-10-07 删除。**要加内置行为请改 `widgets::handle()`。**
    pub fn on_always(mut self, kind: EventKind, f: impl Fn(&mut crate::event::Ctx) + 'static) -> Self {
        self.n().handlers.push(HandlerSlot {
            kind,
            handler: Rc::new(f),
            handled_events_too: true,
        });
        self
    }

    /// 无参回调 → 包一层（用户态最常用的形态）
    pub fn on_tap(self, f: impl Fn() + 'static) -> Self {
        self.on(EventKind::Tapped, move |_| f())
    }

    /// 挂一个**已经装箱**的处理器（[`crate::event::Handler`] = `Rc<dyn Fn(&mut Ctx)>`）。
    ///
    /// 给**组合构件**用：菜单那种"先把属性攒成规格、闭包结束再统一建树"的写法，
    /// 处理器是先存进规格的（要能被 `clone`/多次转移），没法再走 [`Self::on`] 的
    /// `impl Fn` 入口 —— 那条路只能现场装箱。
    pub fn handler(mut self, kind: EventKind, h: crate::event::Handler) -> Self {
        self.n().handlers.push(HandlerSlot {
            kind,
            handler: h,
            handled_events_too: false,
        });
        self
    }

    /// 需要 `cx`（如请求重绘、开窗）时用这个
    pub fn on_tap_with(self, f: impl Fn(&mut crate::event::Ctx) + 'static) -> Self {
        self.on(EventKind::Tapped, f)
    }
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;
