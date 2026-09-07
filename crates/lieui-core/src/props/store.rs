//! 属性存储 —— 列式 + 槽位索引 + 4 级优先层
//!
//! ## 分层
//! `ANIM > LOCAL > STYLE > DEFAULT`。`ANIM` 独立成层是为了让「用户操作」
//! 与「动画」的语义相反：用户 `set` 要打断动画，动画结束要自动回落到 LOCAL。
//!
//! ## 短路求值（ADR-8）
//! ANIM / LOCAL 命中即返回，**根本不会写入继承缓存**，因此主题切换（epoch 自增）
//! 不需要遍历失效它们——不需要分层 epoch。
//!
//! ## 与设计的差异
//! 设计里 `set_raw(&mut self, node, key, src, value)` 内部持有树引用；
//! Rust 下属性写入需要 `&mut Tree`（向下标脏）而解析需要 `&Tree`，
//! 因此这里把树作为显式参数传入（`set(&mut self, tree, node, key, value)`），
//! 由 `Window` 统一调度，避免长期互借。

use crate::arena::GenerationalArena;
use crate::id::{EffectId, NodeId};
use crate::tree::{NodeFlags, Tree};

use super::key::{PropKey, PropKeyId, PropValueKind, value_of_tag};
use super::value::{Color, Dimension, PropValue, SharedString, TypeTag};

/// 层数：ANIM / LOCAL / STYLE / DEFAULT
const LAYERS: usize = 4;

bitflags::bitflags! {
    /// 值来源层级。`set_raw` 只接受单层（位中恰有 1 个 1）。
    #[derive(Copy, Clone, PartialEq, Eq, Debug)]
    pub struct ValueSource: u8 {
        const ANIM    = 1 << 0;
        const LOCAL   = 1 << 1;
        const STYLE   = 1 << 2;
        const DEFAULT = 1 << 3;
    }
}

impl Default for ValueSource {
    fn default() -> Self {
        Self::empty()
    }
}

impl ValueSource {
    /// 单层 → 层下标。多层时返回 `None`。
    pub fn layer(self) -> Option<usize> {
        let b = self.bits();
        if b.count_ones() != 1 {
            None
        } else {
            Some(b.trailing_zeros() as usize)
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum WriterKind {
    #[default]
    None,
    Effect(EffectId),
    Manual,
}

/// 值在某一列中的位置
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct SlotRef {
    pub tag: TypeTag,
    pub idx: u32,
}

impl Default for SlotRef {
    fn default() -> Self {
        Self {
            tag: TypeTag::None,
            idx: 0,
        }
    }
}

/// 槽位元信息：记录了当前占用了哪些层、最后写入者是谁
#[derive(Copy, Clone, Default, Debug)]
pub struct SlotMeta {
    pub source: ValueSource,
    pub writer: WriterKind,
}

/// 列：值数组 + 空闲槽栈
pub struct Column<T> {
    data: Vec<Option<T>>,
    free: Vec<u32>,
}

impl<T> Default for Column<T> {
    fn default() -> Self {
        Self {
            data: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<T> Column<T> {
    fn alloc(&mut self, v: T) -> u32 {
        match self.free.pop() {
            Some(i) => {
                self.data[i as usize] = Some(v);
                i
            }
            None => {
                let i = self.data.len() as u32;
                self.data.push(Some(v));
                i
            }
        }
    }
    fn release(&mut self, i: u32) {
        if let Some(slot) = self.data.get_mut(i as usize)
            && slot.take().is_some()
        {
            self.free.push(i);
        }
    }
    fn get(&self, i: u32) -> Option<&T> {
        self.data.get(i as usize)?.as_ref()
    }
    fn set(&mut self, i: u32, v: T) {
        if let Some(slot) = self.data.get_mut(i as usize) {
            *slot = Some(v);
        }
    }
    pub fn len(&self) -> usize {
        self.data.iter().filter(|x| x.is_some()).count()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 每类型列的访问器
pub trait PropColumn: PropValueKind + Sized {
    fn col(store: &PropertyStore) -> &Column<Self>;
    fn col_mut(store: &mut PropertyStore) -> &mut Column<Self>;
}

macro_rules! impl_prop_column {
    ($ty:ty, $field:ident) => {
        impl PropColumn for $ty {
            #[inline]
            fn col(store: &PropertyStore) -> &Column<Self> {
                &store.$field
            }
            #[inline]
            fn col_mut(store: &mut PropertyStore) -> &mut Column<Self> {
                &mut store.$field
            }
        }
    };
}
impl_prop_column!(f32, f32s);
impl_prop_column!(i32, i32s);
impl_prop_column!(u32, u32s);
impl_prop_column!(bool, bools);
impl_prop_column!(Color, colors);
impl_prop_column!(SharedString, strings);
impl_prop_column!(Dimension, dims);

/// 继承解析缓存条目
#[derive(Clone, Debug, Default)]
struct InheritSlot {
    resolved: PropValue,
    epoch: u32,
    /// 值来自哪个节点（路径压缩）；None = 兜底默认值或本节点
    source: Option<NodeId>,
    src_gen: u32,
    src_epoch: u32,
    valid: bool,
}

#[derive(Default)]
struct InheritCache {
    slots: Vec<InheritSlot>,
}

#[derive(Default)]
struct NodeSlots {
    /// 索引 = slot * LAYERS + layer
    refs: Vec<Option<SlotRef>>,
    meta: Vec<SlotMeta>,
    live: bool,
}

/// 属性存储
pub struct PropertyStore {
    f32s: Column<f32>,
    i32s: Column<i32>,
    u32s: Column<u32>,
    bools: Column<bool>,
    colors: Column<Color>,
    strings: Column<SharedString>,
    dims: Column<Dimension>,

    /// 按 `NodeId::index()` 索引
    nodes: Vec<NodeSlots>,
    inherit: Vec<InheritCache>,

    /// 主题 epoch（换肤 / 热重载时自增）。M4 主题落地后由 `ThemeRegistry` 驱动。
    epoch: u32,
}

impl Default for PropertyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PropertyStore {
    pub fn new() -> Self {
        Self {
            f32s: Column::default(),
            i32s: Column::default(),
            u32s: Column::default(),
            bools: Column::default(),
            colors: Column::default(),
            strings: Column::default(),
            dims: Column::default(),
            nodes: Vec::new(),
            inherit: Vec::new(),
            epoch: 0,
        }
    }

    #[inline]
    pub fn epoch(&self) -> u32 {
        self.epoch
    }
    /// 换肤 / 样式热重载：自增后所有继承缓存一次性失效（O(1)）
    pub fn bump_epoch(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    // ── 容量 ──

    fn ensure(&mut self, node: NodeId) {
        let i = node.index() as usize;
        if self.nodes.len() <= i {
            self.nodes.resize_with(i + 1, NodeSlots::default);
            self.inherit.resize_with(i + 1, InheritCache::default);
        }
        self.nodes[i].live = true;
    }

    fn node_slots(&self, node: NodeId) -> Option<&NodeSlots> {
        self.nodes.get(node.index() as usize)
    }
    fn node_slots_mut(&mut self, node: NodeId) -> &mut NodeSlots {
        self.ensure(node);
        self.nodes.get_mut(node.index() as usize).unwrap()
    }

    // ── 读 ──

    /// 读某一层的原始值（不解析继承）
    pub fn try_layer(&self, node: NodeId, slot: u16, src: ValueSource) -> Option<PropValue> {
        let layer = src.layer()?;
        let ns = self.node_slots(node)?;
        let r = (*ns.refs.get(slot as usize * LAYERS + layer)?)?;
        self.read_ref(r)
    }

    fn read_ref(&self, r: SlotRef) -> Option<PropValue> {
        match r.tag {
            TypeTag::F32 => self.f32s.get(r.idx).map(|v| PropValue::F32(*v)),
            TypeTag::I32 => self.i32s.get(r.idx).map(|v| PropValue::I32(*v)),
            TypeTag::U32 => self.u32s.get(r.idx).map(|v| PropValue::U32(*v)),
            TypeTag::Bool => self.bools.get(r.idx).map(|v| PropValue::Bool(*v)),
            TypeTag::Color => self.colors.get(r.idx).map(|v| PropValue::Color(*v)),
            TypeTag::Str => self.strings.get(r.idx).map(|v| PropValue::Str(v.clone())),
            TypeTag::Dim => self.dims.get(r.idx).map(|v| PropValue::Dim(*v)),
            TypeTag::None => None,
        }
    }

    fn release_ref(&mut self, r: SlotRef) {
        match r.tag {
            TypeTag::F32 => self.f32s.release(r.idx),
            TypeTag::I32 => self.i32s.release(r.idx),
            TypeTag::U32 => self.u32s.release(r.idx),
            TypeTag::Bool => self.bools.release(r.idx),
            TypeTag::Color => self.colors.release(r.idx),
            TypeTag::Str => self.strings.release(r.idx),
            TypeTag::Dim => self.dims.release(r.idx),
            TypeTag::None => {}
        }
    }

    fn alloc<T: PropColumn>(&mut self, v: T) -> SlotRef {
        let idx = T::col_mut(self).alloc(v);
        SlotRef { tag: T::TAG, idx }
    }

    // ── 写 ──

    /// 内部写入口。`src` 必须是单层。
    pub fn set_raw<T: PropColumn>(
        &mut self,
        tree: &mut Tree,
        node: NodeId,
        key: PropKey<T>,
        src: ValueSource,
        value: T,
        writer: WriterKind,
    ) {
        debug_assert_eq!(src.bits().count_ones(), 1, "set_raw 只接受单层 ValueSource");
        self.write_slot(node, key, src, value, writer);

        // ★ 可继承属性会改变后代的解析结果 → 传播失效
        if key.is_inheritable() {
            self.bump_value_epoch(tree, node);
            self.mark_subtree_inherit_dirty(tree, node);
        }
        self.mark_effect_dirty(tree, node, key.flags());
    }

    /// 外部写入口（Presenter / reconcile / effect）。写 LOCAL 会清 ANIM —— 用户操作打断动画。
    pub fn set<T: PropColumn>(&mut self, tree: &mut Tree, node: NodeId, key: PropKey<T>, value: T) {
        self.clear_layer(node, key.slot(), ValueSource::ANIM);
        self.set_raw(
            tree,
            node,
            key,
            ValueSource::LOCAL,
            value,
            WriterKind::Manual,
        );
    }

    /// effect 专用写入口（记录 writer，供 dev 双写检测）
    pub fn set_by_effect<T: PropColumn>(
        &mut self,
        tree: &mut Tree,
        node: NodeId,
        key: PropKey<T>,
        value: T,
        effect: EffectId,
    ) {
        self.clear_layer(node, key.slot(), ValueSource::ANIM);
        self.set_raw(
            tree,
            node,
            key,
            ValueSource::LOCAL,
            value,
            WriterKind::Effect(effect),
        );
    }

    fn write_slot<T: PropColumn>(
        &mut self,
        node: NodeId,
        key: PropKey<T>,
        src: ValueSource,
        value: T,
        writer: WriterKind,
    ) {
        let layer = src.layer().expect("set_raw 只接受单层 ValueSource");
        let slot = key.slot() as usize;
        let idx = slot * LAYERS + layer;
        {
            let ns = self.node_slots_mut(node);
            if ns.refs.len() <= idx {
                ns.refs.resize(idx + 1, None);
            }
            if ns.meta.len() <= slot {
                ns.meta.resize(slot + 1, SlotMeta::default());
            }
        }
        // 先取出旧 SlotRef，结束对 self.nodes 的借用后再碰列
        let existing = self.nodes[node.index() as usize].refs[idx];
        match existing {
            // 复用同一列槽位，避免每次写入都分配
            Some(r) => T::col_mut(self).set(r.idx, value),
            None => {
                let r = self.alloc(value);
                self.nodes[node.index() as usize].refs[idx] = Some(r);
            }
        }
        let ns = self.node_slots_mut(node);
        let meta = &mut ns.meta[slot];
        meta.source |= src;
        meta.writer = writer;
    }

    /// 清除某一层（动画结束 / 用户打断）
    pub fn clear_layer(&mut self, node: NodeId, slot: u16, src: ValueSource) {
        let Some(layer) = src.layer() else { return };
        let idx = slot as usize * LAYERS + layer;
        // 先取走，结束借用后再释放列槽位
        let taken = self
            .nodes
            .get_mut(node.index() as usize)
            .and_then(|ns| ns.refs.get_mut(idx))
            .and_then(|x| x.take());
        if let Some(r) = taken {
            self.release_ref(r);
        }
        if let Some(ns) = self.nodes.get_mut(node.index() as usize)
            && let Some(meta) = ns.meta.get_mut(slot as usize)
        {
            meta.source &= !src;
        }
    }

    pub fn writer_of(&self, node: NodeId, slot: u16) -> Option<WriterKind> {
        let ns = self.node_slots(node)?;
        ns.meta.get(slot as usize).map(|m| m.writer)
    }

    // ── 解析（短路求值）──

    /// 解析属性的最终值。命中高优先级层即返回，不触碰下层。
    pub fn resolve(&mut self, tree: &Tree, node: NodeId, key: PropKeyId) -> PropValue {
        // ① ANIM —— 最高优先级，不查 epoch
        if let Some(v) = self.try_layer(node, key.slot, ValueSource::ANIM) {
            return v;
        }
        // ② LOCAL —— 同上
        if let Some(v) = self.try_layer(node, key.slot, ValueSource::LOCAL) {
            return v;
        }
        // ③ 以下才走继承 / 主题，才需要 epoch 检查
        if let Some(v) = self.cached_inherit(tree, node, key) {
            return v;
        }
        // ④ 沿树向上 + 兜底
        let v = self.resolve_inherited(tree, node, key);
        self.write_inherit_cache(tree, node, key, v.clone());
        v
    }

    fn cached_inherit(&self, tree: &Tree, node: NodeId, key: PropKeyId) -> Option<PropValue> {
        let cache = self.inherit.get(node.index() as usize)?;
        let entry = cache.slots.get(key.slot as usize)?;
        if !entry.valid || entry.epoch != self.epoch {
            return None;
        }
        if tree.get(node)?.flags.contains(NodeFlags::INHERIT_DIRTY) {
            return None;
        }
        // 路径压缩校验：来源节点存活（generation 相同）且其 value_epoch 未变
        match entry.source {
            Some(src) if src.generation() == entry.src_gen => {
                let src_node = tree.get(src)?;
                if src_node.value_epoch == entry.src_epoch {
                    Some(entry.resolved.clone())
                } else {
                    None
                }
            }
            Some(_) => None, // 来源节点已被销毁并复用 → 缓存失效
            None => Some(entry.resolved.clone()),
        }
    }

    fn resolve_inherited(&self, tree: &Tree, node: NodeId, key: PropKeyId) -> PropValue {
        let slot = key.slot;
        // ① 本节点自己的声明：STYLE / DEFAULT
        //    （本节点的 ANIM / LOCAL 已在 `resolve` 里短路返回）
        for src in [ValueSource::STYLE, ValueSource::DEFAULT] {
            if let Some(v) = self.try_layer(node, slot, src) {
                return v;
            }
        }
        // ② 沿树向上取最近祖先的**生效值** —— 仅限可继承属性。
        //    ★ 祖先的 LOCAL 也要参与：用户/effect 在容器上写 `font_size`
        //      同样应当被子节点继承（等同 CSS 的 computed value 继承）。
        //    ★ 不可继承属性（width / height / margin …）绝不能向上取，
        //      否则子节点的尺寸会被父容器污染（并使边界判定失真）。
        if super::defaults::INHERITABLE_SLOTS.contains(&slot) {
            for anc in tree.ancestors(node).skip(1) {
                for src in [ValueSource::LOCAL, ValueSource::STYLE, ValueSource::DEFAULT] {
                    if let Some(v) = self.try_layer(anc, slot, src) {
                        return v;
                    }
                }
            }
        }
        // ③ 主题变量 —— M4（lieui-theme）接入
        // ④ 控件类型默认值
        super::defaults::default_value(slot)
    }

    fn write_inherit_cache(&mut self, tree: &Tree, node: NodeId, key: PropKeyId, value: PropValue) {
        let i = node.index() as usize;
        if self.inherit.len() <= i {
            self.inherit.resize_with(i + 1, InheritCache::default);
        }
        // 路径压缩：记录值来自哪个祖先节点
        let (source, src_gen, src_epoch) = match self.find_inherit_source(tree, node, key.slot) {
            Some(src) => (
                Some(src),
                src.generation(),
                tree.get(src).map_or(0, |n| n.value_epoch),
            ),
            None => (None, 0, 0),
        };
        let cache = &mut self.inherit[i];
        if cache.slots.len() <= key.slot as usize {
            cache
                .slots
                .resize(key.slot as usize + 1, InheritSlot::default());
        }
        cache.slots[key.slot as usize] = InheritSlot {
            resolved: value,
            epoch: self.epoch,
            source,
            src_gen,
            src_epoch,
            valid: true,
        };
    }

    fn find_inherit_source(&self, tree: &Tree, node: NodeId, slot: u16) -> Option<NodeId> {
        for src in [ValueSource::STYLE, ValueSource::DEFAULT] {
            if self.try_layer(node, slot, src).is_some() {
                return Some(node);
            }
        }
        if super::defaults::INHERITABLE_SLOTS.contains(&slot) {
            for anc in tree.ancestors(node).skip(1) {
                for src in [ValueSource::LOCAL, ValueSource::STYLE, ValueSource::DEFAULT] {
                    if self.try_layer(anc, slot, src).is_some() {
                        return Some(anc);
                    }
                }
            }
        }
        None
    }

    // ── 失效传播 ──

    fn bump_value_epoch(&self, tree: &mut Tree, node: NodeId) {
        if let Some(n) = tree.get_mut(node) {
            n.value_epoch = n.value_epoch.wrapping_add(1);
        }
    }

    /// 只标记到最近的继承边界——显式定义了继承值或主题作用域的节点。
    /// 边界内的后代共享一次 dirty 检查，避免 O(n) 全量标记。
    fn mark_subtree_inherit_dirty(&self, tree: &mut Tree, node: NodeId) {
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            if let Some(nd) = tree.get_mut(n) {
                nd.flags.insert(NodeFlags::INHERIT_DIRTY);
            }
            if self.is_inherit_boundary(tree, n) {
                continue;
            }
            stack.extend(tree.children(n));
        }
    }

    /// 显式定义了任一可继承属性的节点 → 继承失效传播到此为止
    fn is_inherit_boundary(&self, tree: &Tree, node: NodeId) -> bool {
        super::defaults::INHERITABLE_SLOTS.iter().any(|s| {
            [ValueSource::LOCAL, ValueSource::STYLE, ValueSource::DEFAULT]
                .iter()
                .any(|src| self.try_layer(node, *s, *src).is_some())
        }) || tree.get(node).is_some_and(|n| n.parent.is_none())
    }

    fn mark_effect_dirty(&self, tree: &mut Tree, node: NodeId, flags: super::key::PropFlags) {
        let Some(n) = tree.get_mut(node) else { return };
        if flags.contains(super::key::PropFlags::AFFECT_LAYOUT) {
            n.layout_dirty = true;
            n.flags.insert(NodeFlags::LAYOUT_DIRTY);
        }
        if flags.contains(super::key::PropFlags::AFFECT_PAINT) {
            n.flags.insert(NodeFlags::PAINT_DIRTY);
        }
    }

    /// 清除节点的继承缓存（继承失效后由解析路径自然重建）
    pub fn clear_inherit_cache(&mut self, node: NodeId) {
        if let Some(c) = self.inherit.get_mut(node.index() as usize) {
            for s in c.slots.iter_mut() {
                s.valid = false;
            }
        }
    }

    // ── 生命周期 ──

    /// 节点销毁：释放其占用的全部列槽位
    pub fn destroy_node(&mut self, node: NodeId) {
        // 先把 SlotRef 全部取走，结束借用后再逐个释放列槽位
        let taken: Vec<SlotRef> = match self.nodes.get_mut(node.index() as usize) {
            Some(ns) => {
                let v: Vec<SlotRef> = ns.refs.iter_mut().filter_map(|r| r.take()).collect();
                ns.meta.clear();
                ns.live = false;
                v
            }
            None => Vec::new(),
        };
        for r in taken {
            self.release_ref(r);
        }
        self.clear_inherit_cache(node);
    }

    /// 已分配槽位的节点数（诊断用）
    pub fn live_nodes(&self) -> usize {
        self.nodes.iter().filter(|n| n.live).count()
    }

    /// 各列当前占用的值个数（诊断用，用于断言无泄漏）
    pub fn value_counts(&self) -> [usize; 7] {
        [
            self.f32s.len(),
            self.i32s.len(),
            self.u32s.len(),
            self.bools.len(),
            self.colors.len(),
            self.strings.len(),
            self.dims.len(),
        ]
    }
}

/// 取 `PropValue` 中指定 tag 的值（类型不匹配返回 None）
pub fn narrow(value: &PropValue, tag: TypeTag) -> Option<PropValue> {
    value_of_tag(value, tag)
}

/// 仅用于文档引用的占位（保持与 arena 的依赖关系显式）
#[allow(dead_code)]
fn _assert_arena_used(a: &GenerationalArena<u8>) -> usize {
    a.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::props::keys as K;

    fn pair() -> (Tree, NodeId) {
        let mut t = Tree::new();
        let n = t.create(crate::id::ElementTypeId::BOX, None);
        t.append_child(t.root(), n);
        (t, n)
    }

    #[test]
    fn layered_priority() {
        let (mut tree, n) = pair();
        let mut p = PropertyStore::new();
        p.set_raw(
            &mut tree,
            n,
            K::GAP,
            ValueSource::DEFAULT,
            1.0,
            WriterKind::None,
        );
        p.set_raw(
            &mut tree,
            n,
            K::GAP,
            ValueSource::STYLE,
            2.0,
            WriterKind::None,
        );
        assert_eq!(p.resolve(&tree, n, K::GAP.id()), PropValue::F32(2.0));

        p.set(&mut tree, n, K::GAP, 3.0);
        assert_eq!(p.resolve(&tree, n, K::GAP.id()), PropValue::F32(3.0));

        p.set_raw(
            &mut tree,
            n,
            K::GAP,
            ValueSource::ANIM,
            4.0,
            WriterKind::None,
        );
        assert_eq!(p.resolve(&tree, n, K::GAP.id()), PropValue::F32(4.0));

        // 用户 set → 清 ANIM
        p.set(&mut tree, n, K::GAP, 5.0);
        assert_eq!(p.resolve(&tree, n, K::GAP.id()), PropValue::F32(5.0));

        // 动画结束 → 回落 LOCAL
        p.clear_layer(n, K::GAP.slot(), ValueSource::ANIM);
        p.set_raw(
            &mut tree,
            n,
            K::GAP,
            ValueSource::ANIM,
            9.0,
            WriterKind::None,
        );
        assert_eq!(p.resolve(&tree, n, K::GAP.id()), PropValue::F32(9.0));
        p.clear_layer(n, K::GAP.slot(), ValueSource::ANIM);
        assert_eq!(p.resolve(&tree, n, K::GAP.id()), PropValue::F32(5.0));
    }

    #[test]
    fn all_value_types_roundtrip() {
        let (mut tree, n) = pair();
        let mut p = PropertyStore::new();
        p.set(&mut tree, n, K::GAP, 1.5);
        p.set(&mut tree, n, K::FLEX_GROW, 2.5);
        p.set(&mut tree, n, K::WIDTH, Dimension::px(120.0));
        p.set(&mut tree, n, K::TEXT, SharedString::new("hi"));
        p.set(&mut tree, n, K::BG, Color::rgb(1, 2, 3));
        p.set(&mut tree, n, K::FLEX_DIRECTION, K::flex_direction::ROW);

        assert_eq!(p.resolve(&tree, n, K::GAP.id()), PropValue::F32(1.5));
        assert_eq!(
            p.resolve(&tree, n, K::WIDTH.id()),
            PropValue::Dim(Dimension::px(120.0))
        );
        assert_eq!(
            p.resolve(&tree, n, K::TEXT.id()),
            PropValue::Str(SharedString::new("hi"))
        );
        assert_eq!(
            p.resolve(&tree, n, K::BG.id()),
            PropValue::Color(Color::rgb(1, 2, 3))
        );
        assert_eq!(
            p.resolve(&tree, n, K::FLEX_DIRECTION.id()),
            PropValue::U32(0)
        );
    }

    #[test]
    fn inheritance_walks_up_and_invalidates() {
        let mut tree = Tree::new();
        let parent = tree.create(crate::id::ElementTypeId::BOX, None);
        let child = tree.create(crate::id::ElementTypeId::TEXT, None);
        tree.append_child(tree.root(), parent);
        tree.append_child(parent, child);

        let mut p = PropertyStore::new();
        p.set(&mut tree, parent, K::FONT_SIZE, 20.0);
        assert_eq!(
            p.resolve(&tree, child, K::FONT_SIZE.id()),
            PropValue::F32(20.0)
        );

        // 父节点改值 → 子节点的继承缓存必须失效
        p.set(&mut tree, parent, K::FONT_SIZE, 30.0);
        assert_eq!(
            p.resolve(&tree, child, K::FONT_SIZE.id()),
            PropValue::F32(30.0)
        );
    }

    /// 不可继承属性绝不能沿树向上取——否则子节点的尺寸会被父容器污染，
    /// 并让「宽高已确定」的边界判定失真（曾经的真实 bug）。
    #[test]
    fn non_inheritable_props_do_not_walk_up() {
        let mut tree = Tree::new();
        let parent = tree.create(crate::id::ElementTypeId::BOX, None);
        let child = tree.create(crate::id::ElementTypeId::TEXT, None);
        tree.append_child(tree.root(), parent);
        tree.append_child(parent, child);

        let mut p = PropertyStore::new();
        p.set(&mut tree, parent, K::WIDTH, Dimension::px(400.0));
        p.set(&mut tree, parent, K::HEIGHT, Dimension::px(600.0));
        p.set(&mut tree, parent, K::FONT_SIZE, 20.0);

        assert_eq!(
            p.resolve(&tree, child, K::WIDTH.id()),
            PropValue::Dim(Dimension::Auto)
        );
        assert_eq!(
            p.resolve(&tree, child, K::HEIGHT.id()),
            PropValue::Dim(Dimension::Auto)
        );
        // 可继承属性照常继承
        assert_eq!(
            p.resolve(&tree, child, K::FONT_SIZE.id()),
            PropValue::F32(20.0)
        );
    }

    #[test]
    fn destroy_node_releases_slots() {
        let (mut tree, n) = pair();
        let mut p = PropertyStore::new();
        p.set(&mut tree, n, K::GAP, 1.0);
        p.set(&mut tree, n, K::TEXT, SharedString::new("abc"));
        assert_eq!(p.value_counts()[0], 1);
        p.destroy_node(n);
        assert_eq!(p.value_counts(), [0; 7]);
    }
}
