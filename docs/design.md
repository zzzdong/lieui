# lieui 详细设计文档

**版本**：v1.0（对应架构设计 v4.2）
**定位**：Rust 桌面 UI 框架 — Retained Node Tree + 信号响应式 + 原生 Rust DSL
**渲染**：vello_cpu（当前）→ vello_hybrid（M7）
**本文性质**：架构设计 v4.2 的**实现规格**。论证过程见架构文档，本文只给可直接编码的规格、签名与算法。

---

# 第一部分：项目总览

## 1.1 目标与非目标

**目标**

| 目标 | 说明 |
|---|---|
| 桌面端原生 UI | Windows / macOS / Linux，自绘控件 |
| 细粒度响应式 | 信号驱动属性槽，无 VDOM、无 diff |
| 原生 Rust DSL | builder API 为主，无脚本引擎 |
| 渲染后端可替换 | DisplayList 隔离，vello_cpu / vello_hybrid / Record（测试） |
| 样式热重载 | RON 外置 + `style_epoch` O(1) 失效 |

**非目标**（明确不做）

不支持 Web / 移动端 · 不内建脚本引擎 · 不提供 DOM 兼容层 · 不支持 CSS 全量 · 不内建富文本编辑器 · 不做原生外观（native look-and-feel）· 不做 STYLE 层自动信号追踪 · 不做 dylib 热重载 · 不做信号图多线程。

## 1.2 workspace 与 crate 划分

```
lieui/                              workspace 根
├── Cargo.toml                      workspace 清单（含 Cargo.lock，纳入版本管理）
├── Cargo.lock                      ★ 必须纳入版本管理（依赖锁定实际生效处）
├── rust-toolchain.toml             固定工具链版本
├── crates/
│   ├── lieui-core/                 内核：节点树 · 属性 · 响应式 · reconcile · 帧管线
│   ├── lieui-layout/               布局：Taitank 风格 Flex 引擎（树无关，移植自 main 分支）
│   ├── lieui-text/                 文本：parley 集成
│   ├── lieui-render/               渲染抽象：DisplayList · RenderBackend · BackendCaps
│   ├── lieui-render-vello/         后端实现：vello_cpu · vello_hybrid
│   ├── lieui-platform/             平台：winit · IME · 剪贴板 · 文件对话框
│   ├── lieui-a11y/                 无障碍：accesskit 集成
│   ├── lieui-theme/                主题：RON schema · ThemeRegistry · StyleWatcher
│   ├── lieui-widgets/              内置控件集
│   ├── lieui-macros/               view! 宏（可选，M3 按需启用）
│   └── lieui/                      门面：重导出 + 预lude + App 启动入口
├── examples/                       示例应用（同时作为 DSL 人体工学试验田）
├── tests/                          集成测试与快照基线
└── benches/                        性能基准
```

**依赖方向（严格单向，禁止反向依赖）**

```
lieui (门面)
   │
   ├─► lieui-widgets ─┬─► lieui-core
   │                  └─► lieui-theme
   ├─► lieui-platform ─► lieui-core
   ├─► lieui-a11y     ─► lieui-core
   └─► lieui-core ─┬─► lieui-layout  ─► lieui-text
                   ├─► lieui-render  （仅依赖 DisplayList 等抽象类型，不含后端）
                   └─► lieui-theme

lieui-render-vello ─► lieui-render    （后端实现，独立 crate，主工程不直接依赖）
```

> **关键约束**：`lieui-core` **不依赖任何具体后端**。`lieui-render-vello` 在应用层装配时注入。这让 vello API 重设计的影响面收敛到一个 crate。

### 1.2.1 Cargo.toml 关键配置

```toml
# 根 Cargo.toml
[workspace]
members = ["crates/*"]
resolver = "2"

[workspace.dependencies]
# 内核
slotmap          = "1.0"
smallvec         = "1.13"
bitflags         = "2.6"
# 渲染
vello_cpu        = "0.2"
vello_hybrid     = "0.1"   # M7 启用
vello_common     = "0.2"
peniko           = "0.4"
kurbo            = "0.11"
# 文本 / 布局
parley           = "0.11"
# 平台 / 无障碍
winit            = "0.30"
accesskit        = "0.17"
accesskit_winit  = "0.23"
# 主题
ron              = "0.8"
notify           = "6.1"
# 工具
insta            = "1.40"
criterion        = "0.5"

[profile.dev]
opt-level = 1          # 兼顾增量编译速度与运行速度
debug = "line-tables-only"

[profile.release]
lto = "thin"
codegen-units = 16
```

**链接器**（M1 引入，目标是把结构改动的重编译压到 5s 内）：

```toml
# .cargo/config.toml
[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
# Windows: 默认 link.exe（stable 工具链的 -Clinker-features=+lld 仍为 unstable，无法启用 rust-lld）；
#          如需启用 lld-link 见文件内注释
# macOS: lld（需 -C link-arg=-fuse-ld=/path/to/ld64.lld）
```

> 实测（本机 x86_64-pc-windows-msvc，i7）：noop 168ms / 结构改动 1.2s / 叶子改动 1.5s / cold 56s，
> 均满足「样式 ≤200ms、结构 ≤5s」目标。完整数据见 `docs/m1-build-times.md`。

### 1.2.2 Feature flags

```toml
[features]
default = ["a11y", "theme-hot-reload"]

# 无障碍：关闭后 P5 整体跳过，M0–M4 期间可关闭
a11y = ["dep:lieui-a11y"]

# 样式热重载：release 可裁剪
theme-hot-reload = ["dep:notify"]

# 不变式严格模式：release 下也让 RefCell 借用失败 panic（默认优雅降级）
strict-invariants = []

# 调试树：默认随 debug_assertions 开启，此 flag 允许在 release 下也开启
debug-tree = []

# 后端选择（互斥）
backend-vello-cpu = ["dep:lieui-render-vello"]
backend-vello-hybrid = ["dep:lieui-render-vello", "lieui-render-vello/hybrid"]
```

## 1.3 模块树（lieui-core）

```
lieui-core/src/
├── lib.rs
├── id.rs                   NodeId / SignalId / MemoId / EffectId / OwnerId / WindowId
├── arena.rs                GenerationalArena<T>（所有 ID 的底层存储）
│
├── tree/
│   ├── mod.rs              Tree · Node · 遍历 · 命中测试
│   ├── diff.rs             Reconciler
│   ├── key.rs              ItemKey · KeyTable
│   └── pool.rs             节点回收池（虚拟化列表用）
│
├── props/
│   ├── mod.rs
│   ├── value.rs            PropValue · TypeTag · 转换
│   ├── key.rs              PropKey<T> · PropKeyId · PropFlags
│   ├── store.rs            PropertyStore · Column
│   ├── resolve.rs          短路求值 · 继承解析 · 路径压缩
│   └── inherit.rs          InheritSlot · 继承边界
│
├── reactive/
│   ├── mod.rs
│   ├── graph.rs            ReactiveGraph
│   ├── signal.rs           Signal<T> · SignalId · 读写 API
│   ├── memo.rs             Memo<T> · 拓扑序重算
│   ├── effect.rs           Effect · EffectCtx
│   ├── owner.rs            OwnerNode · dispose · remove_effect_raw
│   ├── queue.rs            dirty / pending_initial / dirty_memos · 位标记去重
│   └── ctx.rs              BuildCtx · MemoCtx
│
├── view/
│   ├── mod.rs              View trait · BuildCtx 扩展
│   ├── builder.rs          通用 builder 基础设施
│   ├── control.rs          Show / For / Match
│   └── component.rs        组件 trait · 模板
│
├── frame/
│   ├── mod.rs              App::frame() 主循环
│   ├── phase.rs            P0–P5 各阶段实现
│   ├── damage.rs           DamageRegion · 重排边界
│   └── batch.rs            BatchCtx · 事务性变更
│
├── event/
│   ├── mod.rs              事件定义
│   ├── route.rs            Tunnel / Target / Bubble 路由
│   ├── listener.rs         ListenerTable
│   ├── focus.rs            焦点管理 · Tab 序
│   └── hit.rs              命中测试
│
├── anim.rs                 AnimationClock · clear_anim
├── app.rs                  App · Window · split_borrows · close_window
└── error.rs                LieuiError · 错误策略
```

---

# 第二部分：核心数据结构

## 2.1 标识系统与代际索引

所有 ID 都是 `Copy + 'static` 的整数句柄，底层统一走 `GenerationalArena`。

```rust
// id.rs
/// 所有 ID 的内部表示：低 32 位 index，高 32 位 generation
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct NodeId(u64);

impl NodeId {
    #[inline] pub const fn index(self) -> u32 { self.0 as u32 }
    #[inline] pub const fn generation(self) -> u32 { (self.0 >> 32) as u32 }
    #[inline] pub const fn to_u64(self) -> u64 { self.0 }
    #[inline] pub const fn from_u64(v: u64) -> Self { Self(v) }
}

// 其余同构，仅 PhantomData 不同以阻止混用
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct SignalId(u32);
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct MemoId(u32);
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct EffectId(u32);
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct OwnerId(u32);
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct WindowId(u32);
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct ListenerId(u32);
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct AnimId(u32);
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct TemplateId(u32);
```

> **为什么 NodeId 用 u64 而其余用 u32**：节点数量在长列表 + 虚拟化场景下可能突破 u32 的 generation 循环速度，且 `NodeId` 需要跨后端 / debug 工具 / 未来 FFI 传递，统一 64 位更省心。信号等 ID 生命周期短、数量级小，u32 足够，且能减少 arena 元数据内存。

### 2.1.1 GenerationalArena

```rust
// arena.rs
pub struct GenerationalArena<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,              // 空闲 index 栈
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

impl<T> GenerationalArena<T> {
    pub fn insert(&mut self, value: T) -> (u32, u32) {          // (index, generation)
        match self.free.pop() {
            Some(idx) => {
                let slot = &mut self.slots[idx as usize];
                slot.value = Some(value);
                (idx, slot.generation)
            }
            None => {
                let idx = self.slots.len() as u32;
                self.slots.push(Slot { generation: 0, value: Some(value) });
                (idx, 0)
            }
        }
    }

    #[inline]
    pub fn get(&self, index: u32, generation: u32) -> Option<&T> {
        match self.slots.get(index as usize) {
            Some(slot) if slot.generation == generation => slot.value.as_ref(),
            _ => None,                                          // generation 不匹配 = 悬垂
        }
    }

    pub fn remove(&mut self, index: u32, generation: u32) -> Option<T> {
        let slot = self.slots.get_mut(index as usize)?;
        if slot.generation != generation { return None; }
        let v = slot.value.take()?;
        slot.generation = slot.generation.wrapping_add(1);      // ★ 使旧句柄永久失效
        self.free.push(index);
        Some(v)
    }
}
```

> **关键**：`remove` 时 `generation += 1`。这保证**任何持有旧 ID 的代码在 `get` 时静默得到 `None`**，而不是读到被复用的新值——这是整个框架安全性的基石。

## 2.2 节点树

```rust
// tree/mod.rs
pub struct Tree {
    nodes: GenerationalArena<Node>,
    root: NodeId,
}

pub struct Node {
    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
    pub prev_sibling: Option<NodeId>,
    pub next_sibling: Option<NodeId>,

    pub type_id: ElementTypeId,        // 控件类型（u16）
    pub owner: OwnerId,                // 所属响应式 Owner
    pub key: Option<ItemKey>,          // reconcile 用的 key
    pub child_count: u32,

    // 位标志：常用状态集中放，减少缓存行占用
    pub flags: NodeFlags,
    // 每节点 value_epoch（可继承属性写入时自增，供路径压缩比对）
    pub value_epoch: u32,
    // 伪状态（hover / active / focus / disabled ...），供样式 states 查表
    pub pseudo: PseudoClassSet,
    // 布局脏标记与重排边界
    pub layout_dirty: bool,
    pub is_layout_boundary: bool,
}

bitflags! {
    pub struct NodeFlags: u32 {
        const VISIBLE        = 1 << 0;
        const INHERIT_DIRTY  = 1 << 1;   // 继承缓存需重解析
        const LAYOUT_DIRTY   = 1 << 2;
        const PAINT_DIRTY    = 1 << 3;
        const NEEDS_A11Y     = 1 << 4;
        const FOCUSABLE      = 1 << 5;
        const CLIPS          = 1 << 6;
        const RECYCLED       = 1 << 7;   // 处于回收池中
    }
}
```

**遍历**：提供 `Ancestors`、`Descendants`、`Children` 三个迭代器，全部基于 sibling 指针，零分配。

```rust
impl Tree {
    pub fn children(&self, node: NodeId) -> ChildIter<'_>;
    pub fn ancestors(&self, node: NodeId) -> AncestorIter<'_>;      // 含自身
    pub fn descendants(&self, node: NodeId) -> DescendantIter<'_>;  // 深度优先

    /// 不提供 query_selector —— 禁止字符串全局查询
    /// 调试/测试遍历走 #[cfg(debug_assertions)] 的 DebugTree
}
```

### 2.2.1 节点回收池（虚拟化列表）

```rust
// tree/pool.rs
pub struct NodePool {
    free: HashMap<ElementTypeId, Vec<NodeId>>,   // 按控件类型分池
}

impl NodePool {
    /// 取出或新建：优先复用同类型节点，避免重新分配属性槽
    pub fn take(&mut self, tree: &mut Tree, props: &mut PropertyStore,
                ty: ElementTypeId, owner: OwnerId) -> NodeId;
    /// 归还：清除数据与监听，但保留属性槽容量
    pub fn give_back(&mut self, tree: &mut Tree, node: NodeId);
}
```

## 2.3 属性系统

### 2.3.1 PropValue 与 TypeTag

```rust
// props/value.rs
#[derive(Clone, Debug, PartialEq)]
pub enum PropValue {
    F32(f32),
    I32(i32),
    U32(u32),
    Bool(bool),
    Color(Color),
    Str(SharedString),           // 内部 Arc<str>，小字符串内联
    Dim(Dimension),              // 布局尺寸：Auto / Px / Percent
    Any(Box<dyn AnyPropValue>),  // 冷门类型兜底
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum TypeTag { F32, I32, U32, Bool, Color, Str, Dim, Any, None }
```

> **`PropValue` 无 `Object` / `Array` 变体**——这是刻意的边界收窄：复合数据必须拆成多个细粒度信号或属性，避免把"半个 ViewModel"塞进属性表。

### 2.3.2 PropKey

```rust
// props/key.rs
pub struct PropKey<T> {
    slot: u16,
    tag: TypeTag,          // 决定存到哪一列
    flags: PropFlags,
    _ty: PhantomData<T>,
}

impl<T: PropValueKind> PropKey<T> {
    /// 编译期常量构造。所有内置属性的 key 都是 `pub const`。
    pub const fn new(slot: u16, flags: PropFlags) -> Self {
        Self { slot, tag: T::TAG, flags, _ty: PhantomData }
    }
    #[inline] pub const fn slot(&self) -> u16 { self.slot }
    #[inline] pub const fn flags(&self) -> PropFlags { self.flags }
    #[inline] pub const fn is_inheritable(&self) -> bool {
        self.flags.contains(PropFlags::INHERITABLE)
    }
}

// 无类型版本：effect / meta 表 / 注册表用
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct PropKeyId { slot: u16, tag: TypeTag }

bitflags! {
    pub struct PropFlags: u8 {
        const INHERITABLE   = 1 << 0;   // 沿树继承（字号、前景色、字体族…）
        const AFFECT_LAYOUT = 1 << 1;   // 变化需触发重排
        const AFFECT_PAINT  = 1 << 2;   // 变化需触发重绘
    }
}

/// 类型 → TypeTag 的编译期映射
pub trait PropValueKind: Clone + 'static { const TAG: TypeTag; }
impl PropValueKind for f32 { const TAG: TypeTag = TypeTag::F32; }
// … 其余同理
```

### 2.3.3 PropertyStore（列式 + 槽位索引）

```rust
// props/store.rs
pub struct PropertyStore {
    // ── 按类型分列 ──
    f32s:    Column<f32>,
    i32s:    Column<i32>,
    u32s:    Column<u32>,
    bools:   Column<bool>,
    colors:  Column<Color>,
    strings: Column<SharedString>,
    dims:    Column<Dimension>,
    anys:    Column<Box<dyn AnyPropValue>>,

    // ── 每节点的槽位表 ──
    // slot_refs[node][slot] → 指向某列的 (TypeTag, index)；None 表示未设置
    slot_refs: GenerationalArena<Box<[SlotRef]>>,
    // slot_meta[node][slot] → 优先级位与写入者
    slot_meta: GenerationalArena<Box<[SlotMeta]>>,
}

#[derive(Copy, Clone, Default)]
pub struct SlotRef { tag: TypeTag, idx: u32 }

#[derive(Copy, Clone, Default)]
pub struct SlotMeta {
    pub source: ValueSource,       // DEFAULT / STYLE / LOCAL / ANIM 位掩码
    pub writer: WriterKind,
}

bitflags! {
    pub struct ValueSource: u8 {
        const DEFAULT = 1 << 0;
        const STYLE   = 1 << 1;
        const LOCAL   = 1 << 2;
        const ANIM    = 1 << 3;
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum WriterKind { None, Effect(EffectId), Manual }

/// 列：值数组 + 空闲槽栈
pub struct Column<T> {
    data: Vec<Option<T>>,
    free: Vec<u32>,
}
```

**写入路径**（唯一入口）：

```rust
impl PropertyStore {
    /// src 是单层（不是位掩码），一次只写一层的原始值
    pub(crate) fn set_raw<T: PropValueKind>(
        &mut self, node: NodeId, key: PropKey<T>, src: ValueSource, value: T,
    ) {
        debug_assert!(src.bits().count_ones() == 1, "set_raw 只接受单层 ValueSource");

        self.write_slot(node, key, src, value);

        // ★ 只有可继承属性才触发失效传播
        if key.is_inheritable() {
            self.bump_value_epoch(node);
            self.mark_subtree_inherit_dirty(node);
        } else if key.flags().contains(PropFlags::AFFECT_LAYOUT) {
            self.mark_layout_dirty(node);
        } else if key.flags().contains(PropFlags::AFFECT_PAINT) {
            self.mark_paint_dirty(node);
        }
    }

    /// 外部写入口（Presenter / reconcile 用），自动处理优先级清空
    pub fn set<T: PropValueKind>(&mut self, node: NodeId, key: PropKey<T>, value: T) {
        // 写 LOCAL 会清 ANIM —— 用户操作打断动画
        self.clear_layer(node, key.slot(), ValueSource::ANIM);
        self.set_raw(node, key, ValueSource::LOCAL, value);
    }
}
```

**读取路径（短路求值）**：

```rust
// props/resolve.rs
impl PropertyStore {
    /// 解析属性的最终值。命中高优先级层即返回，不触碰下层。
    pub fn resolve(
        &mut self, tree: &Tree, theme: &ThemeRegistry, node: NodeId, slot: u16,
    ) -> PropValue {
        // ① ANIM —— 最高优先级，不查 epoch
        if let Some(v) = self.try_layer(node, slot, ValueSource::ANIM) {
            return v;
        }
        // ② LOCAL —— 不查 epoch
        if let Some(v) = self.try_layer(node, slot, ValueSource::LOCAL) {
            return v;
        }
        // ③ 以下才走继承 / 主题，才需要 epoch 检查
        let inherit = &self.inherit[node];
        if inherit.epoch == theme.epoch
            && !tree.get(node).flags.contains(NodeFlags::INHERIT_DIRTY)
            && inherit.is_valid(tree)
        {
            return inherit.resolved.clone();
        }
        // ④ 沿树向上 + theme scope 解析
        let v = self.resolve_inherited(tree, theme, node, slot);
        self.write_inherit_cache(node, slot, v.clone(), theme.epoch);
        v
    }
}
```

> **短路求值的关键收益**（架构 12.2）：ANIM / LOCAL 命中的属性**根本不会被写入 inherit 缓存**，因此主题切换（epoch 自增）不会影响它们——**不需要分层 epoch，也不需要遍历失效**。

### 2.3.4 继承解析与路径压缩

```rust
// props/inherit.rs
pub struct InheritSlot {
    pub resolved: PropValue,
    pub epoch: u32,
    pub source: Option<NodeId>,   // 值来自哪个节点（路径压缩）
    pub src_gen: u32,             // 来源节点的 generation
    pub src_epoch: u32,           // 来源节点写入时的 value_epoch
}

impl PropertyStore {
    fn resolve_inherited(
        &self, tree: &Tree, theme: &ThemeRegistry, node: NodeId, slot: u16,
    ) -> PropValue {
        // ① 本节点有显式 STYLE / DEFAULT
        if let Some(v) = self.try_layer(node, slot, ValueSource::STYLE) {
            return v;
        }
        // ② 沿树向上找最近显式 STYLE / DEFAULT
        for anc in tree.ancestors(node).skip(1) {
            if let Some(v) = self.try_layer(anc, slot, ValueSource::STYLE) {
                return v;
            }
        }
        // ③ 查 theme scope（子树局部主题），再查全局变量
        if let Some(v) = theme.resolve_var(tree, node, slot) {
            return v;
        }
        // ④ 控件类型默认值
        DEFAULT_TABLE.get(type_id, slot).clone()
    }

    /// 路径压缩校验：来源节点存活 + 其 value_epoch 未变 → 复用缓存
    fn inherit_cache_valid(&self, tree: &Tree, slot: &InheritSlot) -> bool {
        match slot.source {
            Some(src) => tree.is_alive(src, slot.src_gen)
                         && tree.get(src).value_epoch == slot.src_epoch,
            None => true,
        }
    }
}
```

**失效传播（继承边界）**：

```rust
impl PropertyStore {
    fn mark_subtree_inherit_dirty(&mut self, node: NodeId) {
        // 只标记到最近的"继承边界"——即显式定义了继承值或 theme scope 的节点
        // 边界内的后代共享一次 dirty 检查，避免 O(n) 全量标记
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            self.tree.get_mut(n).flags.insert(NodeFlags::INHERIT_DIRTY);
            if self.is_inherit_boundary(n) { continue; }   // 不再向下传播
            stack.extend(self.tree.children(n));
        }
    }
}
```

> **为什么用继承边界**：主题变量与继承值的覆盖点通常是稀疏的。边界内的节点在解析时会命中同一个边界，因此不需要逐个标记。

## 2.4 响应式系统

### 2.4.1 ReactiveGraph

```rust
// reactive/graph.rs
pub struct ReactiveGraph {
    // ── 求值阶段：共享借用（RefCell） ──
    signals: RefCell<GenerationalArena<SignalSlot>>,
    memos:   RefCell<GenerationalArena<MemoSlot>>,
    effects: RefCell<GenerationalArena<EffectSlot>>,
    subs:    RefCell<HashMap<RxId, HashSet<RxId>>>,      // 源 → 观察者集合
    deps:    RefCell<HashMap<RxId, HashSet<RxId>>>,      // 观察者 → 源集合
    observer_stack: RefCell<Vec<RxId>>,
    computing:      RefCell<HashSet<MemoId>>,
    dirty:          RefCell<Vec<EffectId>>,
    pending_initial: RefCell<Vec<EffectId>>,
    dirty_memos:    RefCell<Vec<MemoId>>,

    // ── 结构阶段：独占借用（&mut self） ──
    owners: GenerationalArena<OwnerNode>,
}

// ★ 编译期强制 UI 线程亲和
// impl !Sync for ReactiveGraph {}
// impl !Send for ReactiveGraph {}

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub enum RxId { Signal(SignalId), Memo(MemoId) }
```

> **内部可变性分层**：值层与依赖层用 `RefCell`（求值阶段 `&self` 即可读写），结构层（`owners`）用 `&mut self` 独占。两者时间上不重叠——**结构变更只在 reconcile 阶段发生，那时没有 effect 在运行**。

### 2.4.2 信号

```rust
// reactive/signal.rs
pub struct SignalSlot {
    pub value: Box<dyn AnyClone>,     // 类型擦除的具体值
    pub owner: OwnerId,
    pub epsilon: Option<f32>,         // signal_f32 的容差，存于 slot
}

/// 用户持有的句柄：Copy + 'static
#[derive(Copy, Clone)]
pub struct Signal<T> { id: SignalId, _ty: PhantomData<T> }

impl<T: Clone + 'static> Signal<T> {
    /// 读：登记依赖 + 返回值。dispose 后 **panic**（快速失败）
    pub fn get(&self) -> T {
        with_graph(|g| g.read_signal::<T>(self.id))
    }
    /// 容错读：dispose 后返回 None
    pub fn try_get(&self) -> Option<T> {
        with_graph(|g| g.try_read_signal::<T>(self.id))
    }
    /// 写：写入即传播，不检查相等
    pub fn set(&self, v: T) { with_graph(|g| g.write_signal(self.id, v)) }
    /// 写：相等则跳过（高频更新用）
    pub fn set_if_changed(&self, v: T) where T: PartialEq {
        with_graph(|g| g.write_signal_if_changed(self.id, v))
    }
    /// 原地更新（避免一次 clone）
    pub fn update(&self, f: impl FnOnce(&mut T)) { /* … */ }

    pub fn id(&self) -> SignalId { self.id }
}
```

**图侧实现**（guard 不跨调用边界）：

```rust
impl ReactiveGraph {
    pub fn read_signal<T: Clone + 'static>(&self, id: SignalId) -> T {
        self.track(RxId::Signal(id));
        let signals = self.signals.borrow();            // ← guard 作用域限定
        match signals.get(id.index(), id.generation()) {
            Some(slot) => slot.value.downcast_ref::<T>().cloned()
                .expect("Signal<T> 类型不匹配"),
            None => panic!("读取已 dispose 的信号 {id:?}；请改用 try_read_signal"),
        }
    }                                                    // ← guard 在此释放

    pub fn write_signal<T: 'static>(&self, id: SignalId, v: T) {
        {
            let mut signals = self.signals.borrow_mut(); // ← 独立作用域
            if let Some(slot) = signals.get_mut(id.index(), id.generation()) {
                slot.value = Box::new(v);
            } else { return; }                            // 已 dispose，静默忽略
        }                                                 // ← 释放后才 mark_dirty
        self.mark_dirty_from(RxId::Signal(id));           // ← 新的 borrow
    }

    pub fn write_signal_if_changed<T: PartialEq + 'static>(&self, id: SignalId, v: T) {
        let changed = {
            let signals = self.signals.borrow();
            match signals.get(id.index(), id.generation()) {
                Some(slot) => !slot.value.downcast_ref::<T>()
                                 .map_or(false, |old| old == &v),
                None => false,
            }
        };
        if changed { self.write_signal(id, v); }
    }
}
```

> **铁律**：`borrow()` / `borrow_mut()` 必须在同一函数内取得并释放，**绝不跨越任何用户回调或外部调用边界**。这条规则使分层设计的所有潜在借用冲突一次性消失。

### 2.4.3 Memo

```rust
// reactive/memo.rs
pub struct MemoSlot {
    pub value: Box<dyn AnyClone>,
    pub compute: MemoFn,          // fn(&ReactiveGraph) -> Box<dyn AnyClone>
    pub owner: OwnerId,
    pub stale: bool,
    pub depth: u32,               // 依赖深度，用于拓扑序排序
}

#[derive(Copy, Clone)]
pub struct Memo<T> { id: MemoId, _ty: PhantomData<T> }

impl<T: Clone + PartialEq + 'static> Memo<T> {
    pub fn get(&self) -> T { with_graph(|g| g.read_memo::<T>(self.id)) }
    pub fn try_get(&self) -> Option<T> { with_graph(|g| g.try_read_memo::<T>(self.id)) }
}
```

**重算 + RAII 守卫**：

```rust
impl ReactiveGraph {
    pub fn read_memo<T: Clone + PartialEq + 'static>(&self, id: MemoId) -> T {
        self.track(RxId::Memo(id));
        if self.memos.borrow().get(id.index(), id.generation())
               .map_or(false, |m| m.stale) {
            self.recompute_memo(id);
        }
        let memos = self.memos.borrow();
        match memos.get(id.index(), id.generation()) {
            Some(m) => m.value.downcast_ref::<T>().cloned().unwrap(),
            None => panic!("读取已 dispose 的 memo {id:?}"),
        }
    }

    fn recompute_memo(&self, id: MemoId) {
        assert!(!self.computing.borrow().contains(&id),
            "递归 memo：memo {id:?} 在其自身求值中被读取。\
             Memo 必须是纯计算且不可自引用——需要循环状态请用 Signal + Effect。");

        // ★ RAII 守卫：panic 时无条件清理，杜绝 computing 残留误报
        let _guard = ComputingGuard::new(self, id);

        self.observer_stack.borrow_mut().push(RxId::Memo(id));
        self.deps.borrow_mut()
            .entry(RxId::Memo(id)).or_default().clear();   // 每次重收集依赖

        let compute = self.memos.borrow()
            .get(id.index(), id.generation()).unwrap().compute;
        let value = compute(self);                          // 可能 panic

        self.observer_stack.borrow_mut().pop();
        let mut memos = self.memos.borrow_mut();
        if let Some(m) = memos.get_mut(id.index(), id.generation()) {
            m.value = value;
            m.stale = false;
        }
    }
}

struct ComputingGuard<'a> { graph: &'a ReactiveGraph, id: MemoId }
impl Drop for ComputingGuard<'_> {
    fn drop(&mut self) {
        self.graph.computing.borrow_mut().remove(&self.id);
        let mut stack = self.graph.observer_stack.borrow_mut();
        if stack.last() == Some(&RxId::Memo(self.id)) { stack.pop(); }
    }
}
```

> **为什么必须是 RAII**：若用"插入 → 计算 → 移除"的线性写法，`compute` 闭包 panic 会让 `computing` 残留，**后续所有对该 memo 的访问都会误报递归**。`Drop` 在栈展开时无条件执行，同时顺带修复 `observer_stack` 的同类残留。

### 2.4.4 Effect

```rust
// reactive/effect.rs
pub struct EffectSlot {
    pub f: EffectFn,             // fn(&mut EffectCtx)
    pub owner: OwnerId,
    pub node: NodeId,
    pub window: WindowId,        // ★ 由框架从 node 推导填充
    pub key: PropKeyId,
    pub flags: Cell<EffectFlags>,
}

bitflags! {
    pub struct EffectFlags: u8 {
        const IN_INITIAL     = 1 << 0;
        const IN_DIRTY       = 1 << 1;
        const RAN_THIS_FRAME = 1 << 2;   // 帧首统一重置
    }
}

pub struct EffectCtx<'a> {
    graph:  &'a ReactiveGraph,
    window: &'a mut Window,       // ★ 不是 &mut App：字段级拆分借用避开冲突
    slot:   EffectId,
}

impl EffectCtx<'_> {
    /// ★ effect 内唯一的属性写入口：节点失效时返回 false 并跳过
    pub fn set_prop<T: PropValueKind>(
        &mut self, node: NodeId, key: PropKey<T>, v: T,
    ) -> bool {
        if !self.window.tree.is_alive_by_id(node) { return false; }
        self.window.props.set_raw(node, key, ValueSource::LOCAL, v);
        true
    }

    pub fn try_read<T: Clone + 'static>(&self, id: SignalId) -> Option<T> {
        self.graph.try_read_signal(id)
    }
    pub fn read_or<T: Clone + 'static>(&self, id: SignalId, default: T) -> T {
        self.try_read(id).unwrap_or(default)
    }
    pub fn window(&self) -> WindowId { self.window.id }
}
```

**运行器（字段级拆分借用）**：

```rust
// app.rs
impl App {
    /// Rust 允许对同一结构体的不同字段分别借用
    fn split_borrows(&mut self) -> (&ReactiveGraph, &mut Vec<Window>) {
        (&self.rx, &mut self.windows)
    }

    pub(crate) fn run_effects(&mut self, ids: Vec<EffectId>) {
        let (rx, windows) = self.split_borrows();
        for id in ids {
            let w_idx = rx.window_index_of(id);          // 从 EffectSlot.window 读
            let window = &mut windows[w_idx];
            rx.run_effect(id, EffectCtx { graph: rx, window, slot: id });
        }
    }
}
```

### 2.4.5 Owner 树与 dispose

```rust
// reactive/owner.rs
pub struct OwnerNode {
    pub parent: Option<OwnerId>,
    pub children: Vec<OwnerId>,
    pub window: Option<WindowId>,   // Some = Window Owner；None = App Owner
    pub signals: Vec<SignalId>,
    pub memos: Vec<MemoId>,
    pub effects: Vec<EffectId>,
}

impl ReactiveGraph {
    /// 结构阶段：&mut self
    pub fn dispose_owner(&mut self, id: OwnerId) {
        let owned: Vec<OwnerId> = self.collect_subtree(id);   // 深度优先收集
        for o in owned.into_iter().rev() {                      // 子先于父释放
            let node = self.owners.remove(o.index(), o.generation());
            for e in node.effects { self.remove_effect_raw(e); }
            for m in node.memos   { self.memos.get_mut().remove(m.index(), m.generation()); }
            for s in node.signals { self.signals.get_mut().remove(s.index(), s.generation()); }
        }
    }

    /// ★ 三条路径（dispose_owner / remove_effect_preserving_owner / 窗口关闭）
    ///   必须共用这一个函数——任一条自己实现清理都会导致不一致的泄漏
    fn remove_effect_raw(&mut self, e: EffectId) {
        // ① 出队
        self.dirty.get_mut().retain(|x| *x != e);
        self.pending_initial.get_mut().retain(|x| *x != e);
        // ② 清依赖边（双向）
        if let Some(srcs) = self.deps.get_mut().remove(&RxId::Effect(e)) {
            for s in srcs {
                if let Some(subs) = self.subs.get_mut().get_mut(&s) { subs.remove(&RxId::Effect(e)); }
            }
        }
        // ③ 从所属 Owner 的清单摘除 + 释放槽位（generation 自增 → 句柄失效）
        if let Some(owner) = self.effects.get(e.index(), e.generation()).map(|s| s.owner) {
            if let Some(o) = self.owners.get_mut(owner.index(), owner.generation()) {
                o.effects.retain(|x| *x != e);
            }
        }
        self.effects.get_mut().remove(e.index(), e.generation());
    }

    /// 仅摘除 effect，不销毁 Owner（窗口关闭用）
    pub fn remove_effect_preserving_owner(&mut self, e: EffectId) {
        self.remove_effect_raw(e);
    }
}
```

### 2.4.6 build 上下文

```rust
// reactive/ctx.rs
pub struct BuildCtx<'a> {
    pub tree:  &'a mut Tree,
    pub rx:    &'a mut ReactiveGraph,     // 结构阶段：可创建原语
    pub props: &'a mut PropertyStore,
    pub theme: &'a ThemeRegistry,
    pub owner: OwnerId,
    pub window: WindowId,
    pub anim:  &'a mut AnimationClock,
}

impl BuildCtx<'_> {
    pub fn create_signal<T: Clone + 'static>(&mut self, v: T) -> Signal<T>;
    pub fn create_memo<T>(&mut self, f: impl Fn() -> T + 'static) -> Memo<T>;
    pub fn create_effect(&mut self, node: NodeId, key: PropKeyId, f: EffectFn) -> EffectId;
    pub fn child_owner(&mut self) -> OwnerId;
    pub fn handle_of(&self, node: NodeId) -> ViewHandle;
}
```

---

# 第三部分：子系统详细设计

## 3.1 帧管线

```rust
// frame/phase.rs
impl App {
    pub fn frame(&mut self, events: Vec<PlatformEvent>) {
        self.p0_input(events);
        self.p1_update();
        self.p2_layout();
        self.p3_paint();
        self.p4_composite();
        #[cfg(feature = "a11y")]
        self.p5_a11y();
    }
}
```

### P0 INPUT

```rust
fn p0_input(&mut self, events: Vec<PlatformEvent>) {
    // ① 重置 RAN_THIS_FRAME（★ 必须在冻结 dirty 快照之前）
    self.rx.reset_frame_flags();

    // ② 排空 Worker 消息通道（单帧上限，防积压拖慢帧首）
    const MAX_WORKER_MSGS: usize = 256;
    let mut n = 0;
    while let Ok(msg) = self.worker_rx.try_recv() {
        self.apply_worker_message(msg);
        n += 1;
        if n >= MAX_WORKER_MSGS {
            dev_warn!("Worker 消息单帧超过 {MAX_WORKER_MSGS} 条，剩余留到下帧");
            break;
        }
    }

    // ③ 事件路由：命中测试 → Tunnel → Target → Bubble
    for ev in events {
        self.route_event(ev);
    }

    // ④ 样式热重载检查
    #[cfg(feature = "theme-hot-reload")]
    self.poll_style_reload();
}
```

> `RAN_THIS_FRAME` 遗漏重置会导致"某些更新莫名丢失"——症状隐蔽、极难诊断，因此写进帧首第一步并在 M2 用断言覆盖。

### P1 UPDATE

```rust
fn p1_update(&mut self) {
    // 前置：执行事件回调 → 应用变更批次（事务性）
    self.flush_callbacks();
    self.apply_pending_batches();

    // ① 冻结 dirty 快照
    let dirty_snapshot = self.rx.take_dirty();
    let initial_snapshot = self.rx.take_pending_initial();

    // ② Reconcile
    self.reconcile();
    //    a. 创建 / 移动 / 销毁节点
    //    b. 跑组件函数 → 新 effect 进入 pending_initial（深度优先拓扑序入队）
    //    c. dispose 被移除组件的 Owner

    // ③ 清理 dirty 队列中已销毁 Owner 的 effect
    //    （已在 remove_effect_raw 内处理，此处做一致性断言）
    debug_assert!(self.rx.no_dangling_in(&dirty_snapshot));

    // ④ 拓扑序重算 dirty memos
    //    依赖闭包 = pending_initial ∪ dirty 两者的依赖并集，按 depth 升序
    let memo_closure = self.rx.memo_closure_of(&initial_snapshot, &dirty_snapshot);
    self.rx.recompute_memos_topological(memo_closure);

    // ⑤ 运行 pending_initial（深度优先拓扑序，父先于子）
    self.run_effects(initial_snapshot.clone());

    // ⑥ 运行 dirty effects，跳过已在 ⑤ 运行过的（位标记差集去重）
    let remaining: Vec<_> = dirty_snapshot.into_iter()
        .filter(|e| !self.rx.has_flag(*e, EffectFlags::RAN_THIS_FRAME))
        .collect();
    self.run_effects(remaining);

    // ⑦ 文本编辑（IME / 输入）
    self.apply_text_edits();
}
```

> **为什么 memo 重算必须在两类 effect 之前**：保证 ⑤⑥ 读到的 memo 全是最新值，effect 执行时间可预测、调用栈深度恒定，**不存在"effect 触发同步 memo 重算"的路径**。

### P2 LAYOUT

```rust
fn p2_layout(&mut self) {
    for window in &mut self.windows {
        if !window.layout_dirty { continue; }

        // ① parley 两段式测度（只测度脏节点，命中缓存直接返回）
        for node in window.dirty_text_nodes() {
            self.text.measure(window.id, node, &window.props);
        }

        // ② Taitank Flex 求解：从重排边界开始，不整窗重算
        //    LayoutHost 通过 LayoutTree trait 向引擎提供 style / 子节点 / 文本测度，
        //    最多跑两轮（第 2 轮修正首帧无历史 avail 的百分比尺寸）。
        for boundary in window.layout_boundaries() {
            window.run_layout(&mut self.text);
        }

        window.layout_dirty = false;
    }
}
```

> **全程原生 f32**，不做任何量化（量化只在 P3）。

### P3 PAINT

```rust
fn p3_paint(&mut self) {
    for window in &mut self.windows {
        if !window.paint_dirty { continue; }

        window.dl.clear();
        let mut ctx = PaintCtx {
            dl: &mut window.dl,
            layout: &window.layout,
            props: &window.props,
            text: &self.text,
            theme: &self.theme,
            damage: &window.damage,
        };

        // 遍历渲染对象树，按 z 序产出 DrawOp
        for (node, ro) in window.paint_order() {
            self.registry.paint(ro.type_id, &mut ctx, node);
        }

        // ★ 量化只在生成 DrawOp 时进行（DisplayList::push 内部完成）
        window.paint_dirty = false;
    }
}
```

### P4 COMPOSITE / P5 A11Y

```rust
fn p4_composite(&mut self) {
    for window in &mut self.windows {
        self.backend.render(&window.dl, &self.render_resources,
                            &window.damage, self.quality);
        self.backend.present(window.surface.as_mut());
        window.damage.clear();
    }
}

fn p5_a11y(&mut self) {
    for window in &mut self.windows {
        if !window.a11y_dirty { continue; }
        let update = self.a11y.build_update(&window.tree, &window.props, &window.layout);
        window.a11y_adapter.update(update);
        window.a11y_dirty = false;
    }
}
```

## 3.2 Reconcile

```rust
// tree/diff.rs
pub struct Reconciler<'a> {
    tree:  &'a mut Tree,
    props: &'a mut PropertyStore,
    rx:    &'a mut ReactiveGraph,
    pool:  &'a mut NodePool,
    owner: OwnerId,
    window: WindowId,
}
```

### 3.2.1 Key 策略

```rust
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum ItemKey { Str(SharedString), I64(i64), U64(u64) }

impl Reconciler<'_> {
    fn diff_children(&mut self, parent: NodeId, new: &[ElementDesc], old: &[NodeId]) {
        // ① 建旧表：key → NodeId
        let mut old_by_key: HashMap<ItemKey, NodeId> = HashMap::new();
        let mut old_keyless: Vec<NodeId> = Vec::new();
        for &n in old {
            match self.tree.get(n).key.clone() {
                Some(k) => { old_by_key.insert(k, n); }
                None    => { old_keyless.push(n); }
            }
        }

        // ② 遍历新列表，决定复用 / 移动 / 新建
        let mut used: HashSet<NodeId> = HashSet::new();
        let mut result: Vec<NodeId> = Vec::with_capacity(new.len());

        for (i, desc) in new.iter().enumerate() {
            let matched: Option<NodeId> = match &desc.key {
                Some(k) => old_by_key.get(k).copied().filter(|n| !used.contains(n)),
                // 无 key：按位置降级
                None => old_keyless.get(i).copied().filter(|n| !used.contains(n)),
            };

            let node = match matched {
                Some(n) if self.tree.get(n).type_id == desc.type_id => {
                    used.insert(n);
                    self.reuse(n, desc);      // 移动 + 更新属性，★ 保留 NodeId
                    n
                }
                _ => self.create(desc),       // 类型不同或 key 未匹配 → 新建
            };
            result.push(node);
        }

        // ③ 未匹配的旧节点 → 销毁（并 dispose 对应 Owner）
        for &n in old {
            if !used.contains(&n) { self.destroy(n); }
        }

        // ④ 重排 children 链接（未变化的子树整体跳过）
        self.tree.set_children(parent, &result);
    }
}
```

**关键语义**：

| 场景 | 行为 |
|---|---|
| key 相同 + 类型相同 | **移动**：保留 `NodeId`，身份 / 焦点 / 动画状态全部保留 |
| key 相同 + 类型不同 | 销毁重建 |
| 无 key | 按位置降级匹配 + **dev 警告** |
| key 重复 | **dev 报错** |

### 3.2.2 移动后的继承失效

```rust
fn reuse(&mut self, node: NodeId, desc: &ElementDesc) {
    if self.tree.get(node).parent != Some(desc.parent) {
        // 层级变化 → 继承与 theme scope 解析结果可能变化
        self.props.mark_subtree_inherit_dirty(node);
    }
    self.update_props(node, desc);
}
```

### 3.2.3 与响应式的交互顺序

严格遵守 P1 的 ①②③④⑤⑥⑦。特别地：

- **新 effect 不立即执行**，进入 `pending_initial` 在 ⑤ 统一运行——立即执行会在组件函数内递归创建 effect，且执行时树尚未稳定
- **dispose 时会清理 dirty 队列**（`remove_effect_raw` 内完成），杜绝访问已销毁节点

## 3.3 布局（lieui-layout）

> **M1 变更**：原计划用 taffy，改为移植本地 `main` 分支的 Taitank 风格 Flex 引擎（见 §7.2 M1 任务说明）。
> 引擎**树无关**：通过 `LayoutTree` trait 向宿主获取样式/子节点/文本测度，宿主自行缓存。

```rust
// lieui-layout/src/lib.rs
pub struct LayoutEngine;   // 纯函数式：build_flex + 递归 layout，无每窗口状态

/// 引擎回调宿主（实现方持有 props 解析缓存与文本测度缓存）
pub trait LayoutTree {
    fn style_of(&mut self, node: u64) -> FlexStyle;
    fn collect_children(&mut self, node: u64, out: &mut Vec<u64>);
    fn measure(&mut self, node: u64, max_width: Option<f32>) -> Option<(f32, f32)>;
    fn is_text(&mut self, node: u64) -> bool;
    fn scroll_offset(&mut self, node: u64) -> (f32, f32);
}

pub fn compute_layout(host: &mut impl LayoutTree, root: u64,
                      avail: (f32, f32), origin: (f32, f32)) -> LayoutOutput;
```

```rust
// lieui-core/src/layout.rs —— 宿主实现
struct LayoutHost<'a> {
    tree: &'a Tree,
    props: &'a mut PropertyStore,
    text: &'a mut TextService,
    last: &'a LayoutStore,   // 上一帧结果（用于百分比修正 / 重排边界复现）
    viewport: (f32, f32),
}

impl LayoutTree for LayoutHost<'_> {
    fn style_of(&mut self, n) -> FlexStyle { /* 解析 4 层属性 + 本地缓存 */ }
    fn collect_children(&mut self, n, out) { /* 填充 out，避免回调重入冲突 */ }
    fn measure(&mut self, n, max_width) -> Option<(f32,f32)> {
        // 构造 TextSpec 调用 parley 两段式：break_all_lines 缓存 + align 缓存
    }
}
```

**重排边界**：布局相关属性变化时，脏标记只冒泡到**最近的布局边界**（宽高均确定的节点 / 滚动容器 / 窗口根），P2 只重算边界内子树；因 `ComputedLayout` 回显了 `local_x/local_y` 与 `avail_w/avail_h`，边界子树可在不重算祖先的前提下复现完全相同的输入。

**百分比尺寸**：首帧无历史 avail 时退化为 Auto，并触发至多第 2 轮 pass 修正（`unresolved_percent`）。

```rust
pub fn mark_layout_dirty(&mut self, node: NodeId) {
    let mut n = Some(node);
    while let Some(cur) = n {
        if self.tree.get(cur).is_layout_boundary {
            self.boundaries.insert(cur);
            return;
        }
        self.tree.get_mut(cur).layout_dirty = true;
        n = self.tree.get(cur).parent;
    }
}
```

## 3.4 文本（lieui-text）

```rust
// lieui-text/src/lib.rs
pub struct TextService {
    font_cx: FontContext,        // ★ App 级单例，长期持有
    layout_cx: LayoutContext,    // ★ 同上
    buffers: HashMap<NodeId, TextBuffer>,
    layouts: HashMap<NodeId, parley::Layout<Brush>>,
    generations: HashMap<NodeId, u64>,
}

/// parley 三重 &mut 冲突的解法：所有权隔离
///   - 文本内容（buffer）   → 归 TextBuffers 所有
///   - 排版资源（font/layout ctx）→ 归 TextService 所有
///   - 编辑 pass 独占两者，且完全不碰节点树，只按 NodeId 索引
impl TextService {
    /// 纯文本编辑（单行 / 多行输入），走 PlainEditorDriver
    pub fn edit(&mut self, node: NodeId, op: EditOp) {
        let buffer = self.buffers.get_mut(&node).unwrap();
        let mut driver = buffer.editor.driver(&mut self.font_cx, &mut self.layout_cx);
        match op { /* 光标移动、插入、删除、选区 … */ }
    }

    /// 富文本：不用 PlainEditor（它只处理单一样式纯文本）
    /// 走「buffer + 属性区间 + 派生 Layout」
    pub fn layout_rich(&mut self, node: NodeId, spans: &[Span]) { /* … */ }

    /// 重绘判定
    pub fn needs_repaint(&self, node: NodeId) -> bool {
        self.generations[&node] != self.buffers[&node].editor.generation()
    }
}
```

**缓存键**：`text_hash + style_hash + wrap_mode + max_width`。

## 3.5 渲染（lieui-render）

### 3.5.1 DisplayList

```rust
// lieui-render/src/display_list.rs
pub struct DisplayList {
    ops: Vec<DrawOp>,
    regions: Vec<Region>,        // 供像素快照分区容差使用
    paths: Vec<BezPath>,         // 路径池，DrawOp 只存索引
    text_layouts: Vec<TextLayoutId>,
    bounds: RectI,
}

/// ★ 所有几何量以 1/256 像素为单位存储（i32），保证跨平台确定性
pub type Coord256 = i32;

#[derive(Copy, Clone)]
pub struct RectI { x: Coord256, y: Coord256, w: Coord256, h: Coord256 }

pub enum DrawOp {
    // ── 基础 ──
    FillRect     { rect: RectI, radius: Coord256, color: Color },
    StrokeRect   { rect: RectI, radius: Coord256, width: Coord256, color: Color },
    FillPath     { path: u32, transform: [f32; 6], paint: PaintRef },
    StrokePath   { path: u32, transform: [f32; 6], width: Coord256, paint: PaintRef },

    // ── 文本 ──
    FillText     { layout: u32, origin: PointI, color: Color },
    FillTextRun  { layout: u32, run: u32, origin: PointI, color: Color },

    // ── 图像 ──
    DrawImage    { image: u32, rect: RectI, sampling: Sampling },

    // ── 图层与裁剪 ──
    PushClip     { rect: RectI, radius: Coord256 },
    PopClip,
    PushLayer    { rect: RectI, alpha: u8, blend: BlendMode, filter: Option<FilterRef> },
    PopLayer,

    // ── 阴影（后端能力降级用）──
    FillShadow   { rect: RectI, radius: Coord256, blur: Coord256, color: Color },
}

/// 区域类型：像素快照按此分别取容差
#[derive(Copy, Clone)]
pub enum RegionKind { Solid, Geometry, Text, Gradient, Blur }

pub struct Region { rect: RectI, kind: RegionKind }
```

**量化时机**（架构 17.3）：

```rust
impl DisplayList {
    pub fn push(&mut self, op: DrawOp) {
        // ★ 唯一量化点。P0–P2 全程保持原生 f32，
        //   若在动画插值中间量化，高 DPI 下 hairline 与贝塞尔曲线会产生视觉抖动
        self.ops.push(op.quantized());
        if let Some(region) = Region::from(&op) { self.regions.push(region); }
    }
}
```

### 3.5.2 BackendCaps 与降级

```rust
// lieui-render/src/caps.rs
pub struct BackendCaps {
    pub mask_layers: bool,
    pub filter_graph: bool,               // false → 只支持单 primitive
    pub blend_modes: BitSet<BlendMode>,
    pub max_image_atlas: u32,
    pub max_gradient_ramp: u32,
    pub max_path_complexity: u32,
    pub supports_text_blur: bool,
}

/// 降级行为必须预定义——不允许运行时 panic
impl BackendCaps {
    pub fn degrade(&self, op: &DrawOp) -> DegradeAction {
        match op {
            DrawOp::PushLayer { filter: Some(..), .. } if !self.filter_graph =>
                DegradeAction::PrerenderAsImage,        // 预渲染为图像
            DrawOp::PushLayer { .. } if !self.mask_layers =>
                DegradeAction::ReplaceWithClip,         // 降级为 clip
            DrawOp::PushLayer { blend, .. } if !self.blend_modes.contains(*blend) =>
                DegradeAction::ReplaceBlend(BlendMode::SrcOver),
            _ => DegradeAction::Keep,
        }
    }
}
```

### 3.5.3 RenderBackend

```rust
// lieui-render/src/backend.rs
pub trait RenderBackend {
    fn caps(&self) -> &BackendCaps;
    fn resize(&mut self, width: u32, height: u32);
    fn prepare(&mut self, resources: &mut RenderResources);
    fn render(
        &mut self,
        dl: &DisplayList,
        resources: &RenderResources,
        damage: &DamageRegion,
        quality: QualityMode,
    );
    fn present(&mut self, surface: &mut dyn Surface);
    fn debug_name(&self) -> &'static str;
}

#[derive(Copy, Clone)]
pub enum QualityMode { Fast, Balanced, High }

/// 测试后端：只记录 DisplayList，不产生像素
pub struct RecordBackend { log: Vec<DisplayList> }
```

> **为什么必须有自己的 DisplayList**：linebender 已明确预告要重设计 vello API。若 `RenderContext` 调用散落在渲染代码里，那次重设计的成本就是整个渲染层；有 DisplayList 隔离，成本收敛到 `lieui-render-vello` 一个 crate 的适配层。

## 3.6 事件系统

```rust
// event/mod.rs
pub enum EventKind {
    // 指针
    PointerDown, PointerUp, PointerMove, PointerEnter, PointerLeave, Wheel,
    // 键盘
    KeyDown, KeyUp, Ime(ImeEvent),
    // 焦点
    FocusIn, FocusOut,
    // 语义
    Click, DoubleClick, ContextMenu,
    // 自定义（控件私有）
    Custom(u16),
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum RoutePhase { Capture, Target, Bubble }

pub struct EventCtx<'a> {
    pub kind: EventKind,
    pub target: NodeId,
    pub phase: RoutePhase,
    handled: bool,
    app: &'a mut App,
}

impl EventCtx<'_> {
    pub fn stop_propagation(&mut self) { self.handled = true; }
    pub fn is_handled(&self) -> bool { self.handled }
    pub fn signal<T: Clone + 'static>(&self, id: SignalId) -> Option<T> {
        self.app.rx.try_read_signal(id)     // 回调阶段可安全读
    }
}
```

**路由**：

```rust
// event/route.rs
impl App {
    fn route_event(&mut self, ev: PlatformEvent) {
        let target = self.hit_test(ev.position());

        // Tunnel：祖先 → 目标（可拦截）
        let chain: Vec<NodeId> = self.tree.ancestors(target).collect();
        for &node in chain.iter().rev() {
            self.dispatch(node, &ev, RoutePhase::Capture);
            if self.handled { return; }
        }
        // Target
        self.dispatch(target, &ev, RoutePhase::Target);
        // Bubble：目标 → 祖先
        for &node in &chain {
            if self.handled { break; }
            self.dispatch(node, &ev, RoutePhase::Bubble);
        }
    }
}
```

**监听表**：

```rust
// event/listener.rs
pub struct ListenerTable {
    by_node: HashMap<NodeId, Vec<ListenerEntry>>,
}
struct ListenerEntry {
    kind: EventKind,
    phase: RoutePhase,
    f: Box<dyn FnMut(&mut EventCtx)>,
}
```

> **回调不重入管线是硬规则**：回调内只能写信号或入队，返回后才推进下一阶段。这同时解决 WPF `Dispatcher.BeginInvoke` 语义、Rust 别名冲突与重入崩溃。

## 3.7 焦点

```rust
// event/focus.rs
pub struct FocusManager {
    focused: Option<NodeId>,
    tab_order: Vec<NodeId>,      // 按树序 + tab_index 排序，脏时重算
    tab_order_dirty: bool,
}

impl FocusManager {
    pub fn focus(&mut self, node: NodeId);
    pub fn blur(&mut self);
    pub fn next(&mut self, backward: bool) -> Option<NodeId>;
    /// 焦点节点被销毁时自动转移到下一个可聚焦节点
    pub fn on_node_destroyed(&mut self, node: NodeId);
}
```

## 3.8 动画

```rust
// anim.rs
pub struct AnimationClock {
    anims: GenerationalArena<AnimSlot>,
}

pub struct AnimSlot {
    target: (NodeId, PropKeyId),
    from: PropValue, to: PropValue,
    start: Instant, duration: Duration,
    easing: EasingFn,
    on_finish: Option<Box<dyn FnOnce(&mut App)>>,
}

impl AnimationClock {
    /// 每帧 P1 前置调用：推进所有动画
    pub fn tick(&mut self, now: Instant, props: &mut PropertyStore) {
        let mut finished = Vec::new();
        for (id, slot) in self.anims.iter_mut() {
            let t = ((now - slot.start).as_secs_f32() / slot.duration.as_secs_f32())
                    .clamp(0.0, 1.0);
            let eased = (slot.easing)(t);
            // ★ 插值全程原生 f32，量化只发生在 P3
            let v = slot.from.lerp(&slot.to, eased);
            self.set_anim_raw(props, slot.target.0, slot.target.1, v);
            if t >= 1.0 { finished.push(id); }
        }
        for id in finished { self.finish(id, props); }
    }

    /// ★ ANIM 位的唯一写入者（私有）
    fn set_anim_raw(&self, props: &mut PropertyStore,
                    node: NodeId, key: PropKeyId, v: PropValue) {
        props.set_raw_erased(node, key, ValueSource::ANIM, v);
    }

    /// ★ ANIM 位的唯一清除入口：动画自然结束
    pub fn finish(&mut self, id: AnimId, props: &mut PropertyStore) {
        let slot = self.anims.remove(id.index(), id.generation()).unwrap();
        self.clear_anim(props, slot.target.0, slot.target.1);
        if let Some(cb) = slot.on_finish { cb(self.app) }
    }
    /// 取消：同样清理，但不写终值
    pub fn cancel(&mut self, id: AnimId, props: &mut PropertyStore) { /* … */ }
    /// 被用户操作打断（写 LOCAL 会清 ANIM 位，此处同步移除动画记录）
    fn interrupt(&mut self, id: AnimId, props: &mut PropertyStore) { /* … */ }

    fn clear_anim(&self, props: &mut PropertyStore, node: NodeId, key: PropKeyId) {
        props.clear_layer(node, key.slot, ValueSource::ANIM);
        // 短路求值自动回落到 LOCAL / STYLE —— 无需显式恢复
    }
}
```

## 3.9 主题与样式热重载（lieui-theme）

```rust
// lieui-theme/src/lib.rs
pub struct ThemeRegistry {
    scopes: HashMap<ScopeId, ThemeScope>,   // 子树局部主题
    rules: HashMap<ElementTypeId, StyleRule>,
    states: HashMap<ElementTypeId, Vec<(PseudoClassSet, StyleRule)>>,
    epoch: u32,                              // ★ 换肤 / 热重载 = epoch += 1
}

pub struct ThemeScope {
    vars: HashMap<VarKey, PropValue>,
    inherits: Option<ScopeId>,
}

impl ThemeRegistry {
    /// 沿树向上找最近的 scope 定义 —— 树形作用域，不是线性优先级
    pub fn resolve_var(&self, tree: &Tree, node: NodeId, slot: u16) -> Option<PropValue> {
        let mut scope = None;
        for anc in tree.ancestors(node) {
            if let Some(s) = self.scope_of(anc) { scope = Some(s); break; }
        }
        // … 沿 scope 链查找变量
    }
}
```

**StyleWatcher（错误保留语义）**：

```rust
// lieui-theme/src/watcher.rs
pub struct StyleWatcher { path: PathBuf, last_mtime: SystemTime }

impl StyleWatcher {
    pub fn try_reload(&mut self, reg: &mut ThemeRegistry) -> Result<(), StyleError> {
        let mtime = fs::metadata(&self.path)?.modified()?;
        if mtime <= self.last_mtime { return Ok(()); }
        self.last_mtime = mtime;

        let text = fs::read_to_string(&self.path)?;
        let parsed = ron::from_str::<ThemeSet>(&text)
            .map_err(|e| StyleError::Parse(e))?;
        parsed.validate()                                  // $var 未定义等语义错误
            .map_err(|e| StyleError::Validate(e))?;

        // ★ 仅在解析与校验完全成功后才替换并 bump epoch
        *reg = ThemeRegistry::from(parsed);
        reg.epoch += 1;
        Ok(())
    }
}

/// 调用侧：任何 Err → 记录日志 + 保留旧主题 + 等待下一次变化
impl App {
    fn poll_style_reload(&mut self) {
        if let Err(e) = self.style_watcher.try_reload(&mut self.theme) {
            dev_error!("样式重载失败，保留上一个可用主题：{e}");
            // ★ 绝不 bump epoch —— 用户保存半成品文件时 UI 不退化
        }
    }
}
```

**RON schema**（架构 18.1）：`vars` → root scope；`rules[K]` → 按 `ElementTypeId` 索引；`states[S]` → 按伪状态位掩码索引；`"$var"` 引用变量。

## 3.10 无障碍（lieui-a11y）

```rust
// lieui-a11y/src/lib.rs
pub struct A11yAdapter {
    adapter: accesskit_winit::Adapter,
    node_ids: HashMap<NodeId, accesskit::NodeId>,
}

impl A11yAdapter {
    /// 增量更新：只推送 dirty 节点
    pub fn build_update(
        &mut self, tree: &Tree, props: &PropertyStore, layout: &LayoutStore,
    ) -> TreeUpdate {
        // 与节点树同源 —— 不维护独立的无障碍树副本
        // …
    }
}

/// 控件通过 Painter::a11y 声明语义
pub trait Painter {
    fn a11y(&self, ctx: &A11yCtx, b: &mut accesskit::NodeBuilder);
}
```

> **身份同源是关键优势**：Retained Node Tree 的稳定 `NodeId` 直接映射为 `accesskit::NodeId`，不需要像 immediate-mode 那样每帧重建无障碍树。

## 3.11 平台层（lieui-platform）

```rust
// lieui-platform/src/lib.rs
pub struct Platform {
    event_loop: EventLoop<UserEvent>,
}

/// winit ApplicationHandler 实现
impl ApplicationHandler<UserEvent> for LieuiHandler {
    fn window_event(&mut self, id: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::Ime(ime) => {
                // ★ M0 就要验证：parley PlainEditor + winit IME 事件对接
                self.app.pending_text_edits.push(ime.into());
            }
            WindowEvent::Resized(size)     => self.app.resize_window(id, size),
            WindowEvent::CloseRequested    => self.app.close_window(id),
            WindowEvent::RedrawRequested   => self.app.redraw(id),
            _ => self.app.push_event(id, ev.into()),
        }
    }
}
```

**IME 是公认难点**，M0 必须做技术验证：组字（preedit）显示的文本属性、候选窗位置、确认提交时序。

## 3.12 多窗口

```rust
// app.rs
pub struct App {
    pub rx: ReactiveGraph,
    pub theme: ThemeRegistry,
    pub text: TextService,
    pub images: ImageCache,
    pub style_watcher: StyleWatcher,
    pub windows: Vec<Window>,
    pub workers: WorkerPool,
    pub backend: Box<dyn RenderBackend>,
    pub registry: PainterRegistry,
    pub quality: QualityMode,
}

pub struct Window {
    pub id: WindowId,
    pub tree: Tree,
    pub props: PropertyStore,
    pub states: StateStore,
    pub buffers: TextBuffers,
    pub layout: LayoutStore,
    pub dl: DisplayList,
    pub listeners: ListenerTable,
    pub damage: DamageRegion,
    pub focus: FocusManager,
    pub root_owner: OwnerId,           // ★ 窗口根 Owner
    pub surface: Box<dyn Surface>,
    pub a11y_dirty: bool,
    pub layout_dirty: bool,
    pub paint_dirty: bool,
}

impl App {
    /// ★ 窗口关闭四步（顺序不可调换）
    pub fn close_window(&mut self, wid: WindowId) {
        let w_idx = self.window_index(wid);

        // ① 跨 Owner 扫描：找出所有 window == wid 的 effect
        //    （这些 effect 可能属于 App Owner，不会被 ③ 覆盖）
        let orphans = self.rx.effects_with_window(wid);

        // ② 只摘除 effect，不销毁其 Owner（Owner 可能要继续存活）
        for e in orphans { self.rx.remove_effect_preserving_owner(e); }

        // ③ dispose 窗口根 Owner → 级联销毁窗口内组件及其信号/备忘/effect
        let root = self.windows[w_idx].root_owner;
        self.rx.dispose_owner(root);

        // ④ 销毁节点树与 Window 结构
        self.windows.remove(w_idx);
    }

    /// 跨窗口共享信号的显式入口（挂在 App Owner 下）
    pub fn create_shared_signal<T: Clone + 'static>(&mut self, v: T) -> Signal<T> {
        self.rx.create_signal(self.app_owner, v)
    }
}
```

**Owner 分类**：

| 类型 | `OwnerNode::window` | 生命周期 | 示例 |
|---|---|---|---|
| **Window Owner** | `Some(wid)` | 窗口关闭即销毁 | 窗口根组件及其子树 |
| **App Owner** | `None` | 应用退出才销毁 | 业务 ViewModel、全局设置、跨窗口共享状态 |

**dev 跨窗口订阅检查**（仅在创建 effect 时、仅跨 Owner 边界告警）：

```rust
#[cfg(debug_assertions)]
fn check_cross_window_subscription(&self, effect: EffectId, deps: &[RxId]) {
    let e_owner = self.effects[effect].owner;
    for src in deps {
        let s_owner = self.owner_of(*src);
        if let (Some(w1), Some(w2)) = (self.owner_window(e_owner), self.owner_window(s_owner)) {
            if w1 != w2 {
                dev_warn!("信号 {src:?} 属于窗口 {w2:?} 的 Owner，但被窗口 {w1:?} 的 effect 订阅。\
                           窗口关闭时该信号会被销毁，导致悬垂。\
                           请改用 App::create_shared_signal()。");
            }
        }
    }
}
```

---

# 第四部分：DSL 与控件

## 4.1 View trait 与 builder

```rust
// view/mod.rs
pub trait View {
    fn build(self, cx: &mut BuildCtx) -> NodeId;
}

/// 所有控件的 builder 基础设施
pub struct ElementBuilder<T: Widget> {
    props: T::Props,
    children: Vec<Box<dyn FnOnce(&mut BuildCtx) -> NodeId>>,
    key: Option<ItemKey>,
}

impl<T: Widget> ElementBuilder<T> {
    pub fn key(mut self, k: impl Into<ItemKey>) -> Self { self.key = Some(k.into()); self }
    pub fn child(mut self, v: impl View + 'static) -> Self {
        self.children.push(Box::new(move |cx| v.build(cx))); self
    }
    pub fn when(self, cond: bool, f: impl FnOnce(Self) -> Self) -> Self {
        if cond { f(self) } else { self }
    }
    pub fn push_some<V: View + 'static>(self, opt: Option<V>,
                                        f: impl FnOnce(Self, V) -> Self) -> Self {
        match opt { Some(v) => f(self, v), None => self }
    }
}

impl<T: Widget> View for ElementBuilder<T> {
    fn build(self, cx: &mut BuildCtx) -> NodeId {
        let node = cx.tree.create(T::type_id(), cx.owner, self.key);
        T::write_props(&self.props, node, cx);
        // 子组件在独立 Owner 中构建（深度优先 → pending_initial 天然拓扑序）
        let child_owner = cx.child_owner();
        for c in self.children {
            let child = c(&mut cx.with_owner(child_owner));
            cx.tree.append_child(node, child);
        }
        node
    }
}
```

### 4.1.1 结构控制组件

```rust
// view/control.rs
pub struct Show<C> { cond: Signal<bool>, child: C }
impl<C: View> View for Show<C> {
    fn build(self, cx: &mut BuildCtx) -> NodeId {
        // 条件变化 → reconcile 该子树（销毁重建或复用）
        let holder = cx.tree.create(TYPE_HOLDER, cx.owner, None);
        cx.create_effect(holder, /* … */);   // 订阅 cond
        self.child.build(cx)                  // 由 effect 决定是否挂载
    }
}

pub struct For<I, K, V> { items: Signal<Vec<I>>, key: fn(&I) -> K, view: fn(&I) -> V }
pub struct Match<S> { state: Signal<S>, arms: Vec<(S, Box<dyn FnOnce(&mut BuildCtx) -> NodeId>)> }
```

**虚拟化列表**：`For` 在元素数超过阈值时自动切换为虚拟化模式，只 reconcile 可视区 + 上下 buffer，滚出的节点进 `NodePool` 复用。

## 4.2 Widget trait 与 Painter 注册表

```rust
// lieui-widgets/src/lib.rs
pub trait Widget: Sized {
    type Props: Default;
    const TYPE_ID: ElementTypeId;
    fn write_props(props: &Self::Props, node: NodeId, cx: &mut BuildCtx);
}

/// 渲染层按 ElementTypeId 分发
pub trait Painter {
    fn measure(&self, ctx: &MeasureCtx, node: NodeId) -> Size<f32>;
    fn paint(&self, ctx: &mut PaintCtx, node: NodeId);
    fn a11y(&self, ctx: &A11yCtx, b: &mut accesskit::NodeBuilder);
    fn hit_test(&self, ctx: &HitCtx, node: NodeId, p: Point) -> bool { true }
}

pub struct PainterRegistry {
    entries: Vec<Box<dyn Painter>>,      // 按 ElementTypeId 索引
    fallback: Box<dyn Painter>,          // 未知类型：什么都不画 + dev 警告
}
```

**控件定义示例**：

```rust
// lieui-widgets/src/button.rs
pub struct Button;

#[derive(Default)]
pub struct ButtonProps {
    pub label: Option<SharedString>,
    pub variant: ButtonVariant,
    pub disabled: bool,
    pub on_click: Option<Box<dyn Fn(&mut EventCtx)>>,
}

impl Widget for Button {
    type Props = ButtonProps;
    const TYPE_ID: ElementTypeId = TYPE_BUTTON;
    fn write_props(p: &Self::Props, node: NodeId, cx: &mut BuildCtx) {
        cx.props.set(node, LABEL, p.label.clone().unwrap_or_default());
        cx.props.set(node, VARIANT, p.variant);
        cx.props.set(node, DISABLED, p.disabled);
        if let Some(f) = &p.on_click {
            cx.listeners.attach(node, EventKind::Click, RoutePhase::Target, f);
        }
    }
}

/// builder 便捷方法
impl ElementBuilder<Button> {
    pub fn label(mut self, s: impl Into<SharedString>) -> Self { /* … */ self }
    pub fn on_click(mut self, f: impl Fn(&mut EventCtx) + 'static) -> Self { /* … */ self }
    pub fn disabled(mut self, d: bool) -> Self { /* … */ self }
}

pub fn Button_new() -> ElementBuilder<Button> { ElementBuilder::default() }
```

## 4.3 绑定

```rust
// ① 直接绑定信号
Text::new().text(&name)

// ② 计算绑定：闭包内读信号，依赖自动收集
Text::new().text_fn(move || format!("{} {}", first.get(), last.get()))

// ③ Memo 形式（跨多属性复用时用）
let label = cx.create_memo(move || format!("{} {}", first.get(), last.get()));
Text::new().text(&label)
```

**`text_fn` 的实现**：

```rust
impl ElementBuilder<Text> {
    pub fn text_fn<F>(self, f: F) -> Self
    where F: Fn() -> SharedString + 'static
    {
        // 延迟到 build 时创建 effect —— effect 只在结构阶段创建
        self.deferred(move |cx, node| {
            cx.create_effect(node, TEXT.into(), move |ectx| {
                let v = f();                       // 闭包内读信号 → 自动登记依赖
                ectx.set_prop(node, TEXT, v);      // ★ 唯一写入口，自动存活检查
            });
        })
    }
}
```

## 4.4 受控组件：严格 Signal 双向绑定

```rust
// ✅ 正确：单向环路
Input::new()
    .value(&vm.text)                        // effect 写 LOCAL
    .on_input(move |s, ctx| vm.text.set(s)) // 输入 → 写 Signal → effect 重写 UI

// ❌ 错误：Presenter 直写，会触发 dev 双写报错
Input::new()
    .value(&vm.text)
    .on_input(move |s, ctx| handle.set_prop(TEXT, s))
```

**心智模型**：受控组件的"当前值"唯一真相是 Signal。用户输入 → 写 Signal → effect 写回 UI。**这是一条单向环路，不是两条并行路径。**

**Presenter 的适用场景**（`ViewHandle`）：

```rust
let h: ViewHandle = cx.handle_of(container);
h.set_prop(PADDING, 24.0);
h.find("status_label").set_prop(TEXT, "已保存");
```

仅限**非数据驱动的纯命令式交互**：画布绘制、拖拽过程中的临时位置、编辑器光标闪烁、滚动容器的瞬时偏移。共同点是**不进入业务数据流**。

**dev 双写检测**：

```rust
// props/store.rs
fn check_writer(&self, node: NodeId, slot: u16, new: WriterKind) {
    #[cfg(debug_assertions)]
    if let Some(meta) = self.slot_meta(node, slot) {
        match (meta.writer, new) {
            (WriterKind::Effect(_), WriterKind::Manual) | (WriterKind::Manual, WriterKind::Effect(_)) =>
                dev_error!("属性槽 {slot} 存在双写入者（effect 与 Presenter 争抢）。\
                            受控组件请改用 Signal 双向绑定。"),
            _ => {}
        }
    }
}
```

## 4.5 内置控件清单

| 里程碑 | 控件 | 备注 |
|---|---|---|
| **M0–M2** | `Box` `Text` `Button` `Input`(单行) `Scroll` `List`(虚拟化) | 最小可用集 |
| **M3** | `Checkbox` `Radio` `Slider` `Dropdown` `Modal` | |
| **M5** | `TextArea`(多行+IME) `Tab` `Menu` `Tree` `Table` `Tooltip` | |

**公共属性**（定义在 `lieui-widgets/src/props.rs`，均为 `pub const PropKey<T>`）：

```rust
// 布局
pub const PADDING:      PropKey<f32>        = PropKey::new(0, PropFlags::AFFECT_LAYOUT);
pub const GAP:          PropKey<f32>        = PropKey::new(1, PropFlags::AFFECT_LAYOUT);
pub const WIDTH:        PropKey<Dimension>  = PropKey::new(2, PropFlags::AFFECT_LAYOUT);
pub const HEIGHT:       PropKey<Dimension>  = PropKey::new(3, PropFlags::AFFECT_LAYOUT);
// 外观
pub const BG:           PropKey<Color>      = PropKey::new(10, PropFlags::AFFECT_PAINT);
pub const FG:           PropKey<Color>      = PropKey::new(11, PropFlags::AFFECT_PAINT | PropFlags::INHERITABLE);
pub const RADIUS:       PropKey<f32>        = PropKey::new(12, PropFlags::AFFECT_PAINT);
pub const BORDER_W:     PropKey<f32>        = PropKey::new(13, PropFlags::AFFECT_PAINT);
pub const SHADOW:       PropKey<ShadowSpec> = PropKey::new(14, PropFlags::AFFECT_PAINT);
// 文本
pub const FONT_SIZE:    PropKey<f32>        = PropKey::new(20, PropFlags::AFFECT_LAYOUT | PropFlags::INHERITABLE);
pub const FONT_FAMILY:  PropKey<SharedString> = PropKey::new(21, PropFlags::AFFECT_LAYOUT | PropFlags::INHERITABLE);
pub const TEXT:         PropKey<SharedString> = PropKey::new(22, PropFlags::AFFECT_LAYOUT);
// 状态
pub const VISIBLE:      PropKey<bool>       = PropKey::new(30, PropFlags::AFFECT_LAYOUT);
pub const OPACITY:      PropKey<f32>        = PropKey::new(31, PropFlags::AFFECT_PAINT);
pub const DISABLED:     PropKey<bool>       = PropKey::new(32, PropFlags::empty());
```

---

# 第五部分：错误策略与诊断

## 5.1 错误策略总原则

> **框架内部不变式违规 → panic（快速失败）；外部输入错误 → `Result`（可恢复）。**

| 类别 | 处理 | 理由 |
|---|---|---|
| 悬垂句柄读取（`read_signal`） | **panic** | 是 bug，静默错值比崩溃更难查 |
| 悬垂句柄读取（`try_read`） | `None` | 显式容错 |
| effect 运行期间结构变更 | RefCell panic（debug）/ 跳过（release） | 框架不变式违规 |
| 递归 memo | panic | 设计错误，附修复指引 |
| dispose 后写属性 | 返回 `false`（`EffectCtx::set_prop`） | 节点销毁时跳过是正常行为 |
| RON 样式解析失败 | `Result` + 保留旧主题 | 用户输入，可恢复 |
| 窗口创建失败 | `Result` | 平台错误 |
| 后端能力不足 | **降级** + dev 警告 | 不允许 panic |
| Worker 任务失败 | `Result` 传给发起方 | 业务错误 |

## 5.2 错误类型

```rust
// error.rs
#[derive(Debug, thiserror::Error)]
pub enum LieuiError {
    #[error("平台错误：{0}")]
    Platform(#[from] PlatformError),
    #[error("样式解析失败：{0}")]
    Style(#[from] StyleError),
    #[error("窗口不存在：{0:?}")]
    NoSuchWindow(WindowId),
    #[error("Worker 任务失败：{0}")]
    Worker(String),
}

#[derive(Debug, thiserror::Error)]
pub enum StyleError {
    #[error("RON 语法错误：{0}")]
    Parse(ron::error::SpannedError),
    #[error("主题校验失败：{0}")]
    Validate(String),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("窗口创建失败：{0}")]
    WindowCreation(String),
    #[error("事件循环错误：{0}")]
    EventLoop(String),
}
```

## 5.3 诊断宏

```rust
// 所有 dev_* 宏在 release 下编译为 no-op
macro_rules! dev_warn {
    ($($arg:tt)*) => { #[cfg(debug_assertions)] eprintln!("[lieui warn] {}", format!($($arg)*)) };
}
macro_rules! dev_error { /* 同上 */ }
```

**dev 期强制检查清单**：

| 检查 | 触发 |
|---|---|
| 属性槽双写入者 | effect 绑定 / Presenter 写入时 |
| 跨窗口悬垂订阅 | 创建 effect 时 |
| 列表无 key | reconcile 时 |
| key 重复 | reconcile 时 |
| 用户回调超时 | 回调返回时（60Hz 8ms / 120Hz 4ms） |
| Worker 消息积压 | 单帧超 256 条 |
| 后端能力降级 | 生成 DrawOp 时 |
| dispose 后访问 | `try_read` 命中悬垂时（release 下也记录） |

---

# 第六部分：测试策略

## 6.1 分层

| 层级 | 内容 | 工具 |
|---|---|---|
| **单元测试** | 各模块内部逻辑（arena、列式存储、diff） | `#[test]` |
| **集成测试** | 帧管线不变量（dirty 计数、effect 运行次数、内存回落） | `tests/` |
| **快照测试** | IR（字节级严格）+ 像素（分区容差） | `insta` |
| **压力测试** | 10 万节点销毁、多窗口快速开关、1000 信号组件 dispose | `tests/stress.rs` |
| **基准** | 帧时间、布局时间、重绘面积 | `criterion` |

## 6.2 关键不变量断言

```rust
// tests/integration/frame_invariants.rs

#[test]
fn effect_runs_exactly_once_when_in_both_queues() {
    // 构造：新 effect 创建后，其依赖信号在同一帧被写入
    // 断言：该 effect 本帧只运行一次
}

#[test]
fn memo_recomputed_before_pending_initial() {
    // 断言：新 effect 首次求值时读到的 memo 已是最新值
}

#[test]
fn disposing_owner_clears_dirty_queue() {
    // 断言：dispose 后 dirty 队列中不含已销毁的 effect
}

#[test]
fn dangling_handle_returns_none() {
    // 销毁含 1000 个信号的组件 → try_read 全部返回 None 且无泄漏
}

#[test]
fn multi_window_close_cleans_cross_owner_effects() {
    // 两窗口共享 App 信号 → 快速关闭其一
    // 断言：另一窗口 effect 完好、被关闭窗口 effect 全清理、无泄漏
}

#[test]
fn single_text_change_does_not_relayout_window() {
    // 断言：改单个文本节点不触发整窗重排（重排边界生效）
}
```

## 6.3 快照测试

```rust
// tests/snapshot/ir.rs
#[test]
fn ir_snapshot_is_deterministic() {
    let app = build_test_app();
    app.frame(vec![]);
    insta::assert_debug_snapshot!(app.windows[0].dl);   // ★ 字节级严格
}
```

**确定性三规范**（架构 19.1）：

1. 几何量在 P3 生成 DrawOp 时量化到 1/256 px
2. 所有集合遍历用稳定排序 + 按 `NodeId` 升序的显式 tie-break，不依赖 `HashMap` 迭代顺序
3. 哈希使用确定性哈希器（非 SipHash）

> **M0 必须在三平台（macOS / Windows / Linux）× 两优化级别（debug / release）验证一致。**

```rust
// tests/snapshot/pixel.rs
#[test]
fn pixel_snapshot_with_region_tolerance() {
    // 从 DisplayList 导出 .region 元数据（每个 DrawOp 天然知道自己的类型）
    // 按区域分别取阈值：几何 99.9% / 文字 99.0% / 渐变·模糊 99.5%
}
```

基线更新：`LOOM_SNAPSHOT_UPDATE=1` → 改为 `LIEUI_SNAPSHOT_UPDATE=1`，走独立 CI workflow，需人工 review。

## 6.4 压力测试

```rust
// tests/stress.rs
#[test] fn destroy_100k_nodes_reclaims_memory();
#[test] fn dispose_1000_signals_no_leak();
#[test] fn rapid_window_open_close_100_times();
#[test] fn virtualized_list_scroll_1m_items();
```

---

# 第七部分：实施

## 7.1 目录结构（完整）

```
lieui/
├── Cargo.toml
├── Cargo.lock                     ★ 纳入版本管理
├── rust-toolchain.toml
├── .cargo/config.toml             mold / lld 链接器
├── README.md
├── CONTRIBUTING.md                ★ 含附录 C 编码规范
├── crates/
│   ├── lieui-core/src/            （见 1.3 模块树）
│   ├── lieui-layout/src/lib.rs
│   ├── lieui-text/src/lib.rs
│   ├── lieui-render/src/
│   │   ├── lib.rs
│   │   ├── display_list.rs
│   │   ├── caps.rs
│   │   ├── backend.rs
│   │   ├── paint.rs               Color / PaintRef / BlendMode / FilterRef
│   │   └── record.rs              测试后端
│   ├── lieui-render-vello/src/
│   │   ├── lib.rs                 后端装配与选择
│   │   ├── cpu.rs                 vello_cpu 适配
│   │   ├── hybrid.rs              vello_hybrid 适配（M7）
│   │   └── convert.rs             DisplayList → vello Scene（★ 唯一接触 vello API 的地方）
│   ├── lieui-platform/src/{lib.rs, ime.rs, clipboard.rs, dialog.rs}
│   ├── lieui-a11y/src/lib.rs
│   ├── lieui-theme/src/{lib.rs, schema.rs, watcher.rs}
│   ├── lieui-widgets/src/
│   │   ├── lib.rs                 Widget trait · PainterRegistry
│   │   ├── props.rs               公共 PropKey 常量表
│   │   ├── box.rs text.rs button.rs input.rs scroll.rs list.rs ...
│   ├── lieui-macros/src/lib.rs    view! 宏（M3 按需）
│   └── lieui/src/lib.rs           门面 + prelude
├── examples/
│   ├── counter.rs                 M2 最小示例
│   ├── todo_list.rs               M3 列表 + 虚拟化
│   ├── editor.rs                  M5 文本编辑 + IME
│   └── showcase.rs                M4 全套控件 + 主题切换
├── tests/
│   ├── integration/
│   ├── snapshot/
│   └── stress.rs
├── benches/
└── docs/
    ├── 架构设计_v4.2.md
    ├── 编码规范.md                ★ 附录 C 独立成文
    └── 属性参考.md                自动从 PropKey 常量表生成
```

## 7.2 里程碑与任务分解

### M0 骨架（★ 可立即启动）

| # | 任务 | 产出 | 验收 |
|---|---|---|---|
| 1 | workspace 骨架 + CI | 13 个 crate 空壳 + 编译通过 | `cargo build` 全绿 |
| 2 | `GenerationalArena` + ID 体系 | `arena.rs` `id.rs` | 单测：generation 失效、free list 复用 |
| 3 | `Tree` + 遍历 + 命中测试 | `tree/` | 单测：增删改查、深度优先序 |
| 4 | `PropertyStore` 列式存储 | `props/store.rs` | 单测：8 种类型读写、free list |
| 5 | **raw/resolved + 继承存储定稿** | `props/resolve.rs` `inherit.rs` | 单测：继承链、路径压缩、INHERITABLE 过滤 |
| 6 | 阶段化 pass 骨架 + winit | `frame/` `lieui-platform` | 窗口能开、能关、能收事件 |
| 7 | `BackendCaps` + 降级策略 | `lieui-render` | 单测：每种降级路径有对应用例 |
| 8 | vello_cpu 后端适配 | `lieui-render-vello` | **1 万矩形 + 文本帧时间达标** |
| 9 | **IME 技术验证** | `lieui-platform/ime.rs` | **组字、候选窗、提交完整可用** |
| 10 | 快照基线流程 | `tests/snapshot` | **IR 快照三平台 × 两优化级别一致** |
| 11 | **并行：leptos 源码调研 + 最小信号原型** | `docs/m2-prep.md` | **三方案对比（采用/自研/混合）就绪** |

> 任务 11 与任务 9 的等待期并行——IME 验证需要接 winit 事件循环，期间可做信号系统调研。**这不是提前做 M2 的活，而是把调研成本移出 M2 时间窗。**

### M1 布局

| # | 任务 | 验收 | 状态 |
|---|---|---|---|
| 1 | 布局引擎集成 + 重排边界 | 改单文本节点不触发整窗重排（计数器断言） | ✅ |
| 2 | 文本两段式测度 | 测度缓存命中率 > 90% | ✅ |
| 3 | 文本布局缓存 | 相同 (text, style, wrap) 不重复测度 | ✅ |
| 4 | **量编译时间** | **样式改动 ≤200ms / 结构改动 ≤5s 有数据** | ✅ |
| 5 | mold / lld 接入 | 链接时间下降可量化 | ⚠️ 部分（见下） |

> **M1 与原计划不一致的点**（详见 `docs/M1-changelog.md`）：
> - **布局引擎选型变更**：原计划用 `taffy`，改为移植本地 `main` 分支的 Taitank 风格 Flex 引擎（`crates/lieui-layout`，零依赖、树无关）。原因：用户临时决策 + 更贴合「重排边界 / 两段式测度」的可控实现。
> - **parley 版本**：`0.7` → `0.11`（`Brush` 为空结构体 `()`，颜色由绘制层取；API 名称与原计划 `0.7` 不同）。
> - **重排边界实现**：不依赖 taffy 的 partial tree，而是 `ComputedLayout` 回显 `local_x/local_y` + `avail_w/avail_h`，使边界子树能在不重算祖先的前提下复现相同输入；百分比尺寸用「至多 2 轮 pass」修正首帧无历史 avail 的情况。
> - **Windows 链接器**：stable 工具链的 `-Clinker-features=+lld` 仍为 unstable，且本机无 `lld-link`，故 Windows 段暂不启用 rust-lld，仅配置 Linux mold + macOS lld。
> - **`Window.taffy` 字段已移除**：布局状态函数式化（`LayoutEngine` 无每窗口状态）。

### M2 信号响应式（★ 第一周做 leptos 决策）

| # | 任务 | 验收 |
|---|---|---|
| 0 | **第一周：8 项硬标准评估 + 5 个极端用例** | **结论冻结：采用 / 自研** |
| 1 | `ReactiveGraph` 内部可变性分层 | guard 不跨边界铁律有单测覆盖 |
| 2 | 观察者栈 + RAII `ComputingGuard` | 递归 memo panic 后 `computing` 无残留 |
| 3 | 每次重收集依赖 | 分支依赖自动正确（单测） |
| 4 | 拓扑序 memo 重算 | 深度依赖链只算一次 |
| 5 | 队列去重（位标记） | 同帧不重复运行（用例 5） |
| 6 | Owner 分类 + 统一 `remove_effect_raw` | 三条路径共用（代码审查） |
| 7 | dispose 三件事 + `try_read` | 用例 3 通过 |
| 8 | **M2 结束前 2 周冻结信号公共 API** | API 文档冻结 |

### M3 DSL + Reconcile

| # | 任务 | 验收 |
|---|---|---|
| 1 | builder API + `Show`/`For`/`Match` | 示例应用可写 |
| 2 | key 策略 + 双端 diff | 移动保留 `NodeId`（单测） |
| 3 | `pending_initial` 拓扑序 | 首帧无闪烁（像素快照） |
| 4 | `inherit_dirty` + 虚拟化列表 | 100 万项滚动流畅 |
| 5 | **示例应用 + 人体工学归档** | 痛点清单归档；**宏回退触发条件已检查** |

### M4 样式与打磨

| # | 任务 | 验收 |
|---|---|---|
| 1 | RON schema + `ThemeRegistry` | 主题切换全量生效 |
| 2 | `StyleWatcher` + 错误保留语义 | **改 RON → 200ms 内更新；语法错误时 UI 保持不变** |
| 3 | 伪状态（hover/active/disabled） | 状态切换正确 |
| 4 | DSL 打磨 + 可选 `view!` 宏 | 按 M3 结论决定 |

### M5 交互完善

| # | 任务 | 验收 |
|---|---|---|
| 1 | 文本编辑 / IME 完整 | 输入法组字正确 |
| 2 | 焦点 + Tab 序 | 键盘可遍历全部控件 |
| 3 | 路由事件（Tunnel/Target/Bubble） | 冒泡与拦截正确 |
| 4 | accesskit 集成 | 屏幕阅读器可读 |
| 5 | **多窗口** | **快速开关压力测试无泄漏** |
| 6 | **受控组件 + 动画 ANIM 清理** | 无双写告警；动画结束后回落正确 |

### M6 性能 / M7 混合渲染

| # | 任务 | 验收 |
|---|---|---|
| M6 | 剖析、图层缓存、脏矩形、质量档、渲染多线程 | 基准可复现；回归护栏进 CI |
| M7 | 按 ≥30% 判据接入 vello_hybrid | 双后端像素一致；降级正确 |

## 7.3 依赖与升级流程

| 依赖 | 基线 | 关注点 |
|---|---|---|
| vello_cpu | 0.2.x | `render`/`render_with`、`Resources`、filter 支持度；**预期被重设计** |
| vello_hybrid | 0.1.x（M7） | Mask/filter/blend 限制 |
| parley | 0.7.x | `editing`/`cursor` 模块路径、`Alignment` 变体名 |
| taffy | —（已弃用） | 原计划用于布局，M1 改为移植 main 分支的 Taitank 风格 Flex 引擎，见 §3.3 |
| winit | 0.30.x | `ApplicationHandler`、IME 接口 |
| accesskit | 0.17.x | 节点模型、`TreeUpdate` |
| slotmap / smallvec / bitflags | 稳定 | — |
| RON / notify | 稳定 | M4 |
| insta / criterion | 稳定 | 测试 |

**升级流程**（Cargo.lock 对库依赖不生效，实际生效在 workspace 根）：

1. workspace 根 `Cargo.lock` 纳入版本管理
2. CI 对每个依赖变更运行：编译 → 单测 → **IR 快照 → 像素快照**
3. 维护 `BackendCaps` 与 vello 版本的适配表
4. 版本升级必跑全链路快照

---

# 附录 A：M2 前置决策 —— `leptos_reactive` 评估清单

> **M2 第一周必须给出结论并冻结。8 项全过才采用，部分满足 = 自研。**

| # | 评估项 | 不通过则自研 |
|---|---|---|
| 1 | **Owner 语义**：能否表达 Window Owner / App Owner 两级分类 | 只能扁平 Owner |
| 2 | **嵌套依赖追踪**：观察者**栈**还是全局 `current_observer`？后者不支持嵌套 memo | 仅全局单例观察者 |
| 3 | **类型级约束**：能否让 `EffectCtx` 只写属性槽、禁止写信号 | API 暴露写信号能力 |
| 4 | **dispose 语义**：`try_read`、悬垂静默失效、dirty 队列联动清理 | dispose 后 panic 且无 try 变体 |
| 5 | **内部可变性**：依赖追踪方法必须是 `&self`（`ReadSignal::get(&self)` 内部完成订阅登记） | 需要 `&mut self` 才能读 |
| 6 | **每次重收集**：effect/memo 每次运行能否清空依赖集重新收集 | 依赖集只增不减 |
| 7 | **定制扩展**：容差比较、`write_if_changed`、`pending_initial` 队列能否接入 | 核心 API sealed |
| 8 | **依赖重量**：是否引入 web/wasm 相关依赖 | 拖入大量非桌面目标依赖 |

**为什么"部分满足 = 不通过"**：第 5、6 项是**整个借用模型与依赖精确性的地基**。适配它们等于重写其运行时，那时"用它的收益"只剩 arena 存储，而代价是长期受其演进约束——**自研反而更可控**。

**五个极端用例**（采用与否都要跑）：

1. 深层嵌套 Memo（A → B → Signal C）
2. 列表 Reconcile × Signal 并发
3. 悬垂句柄静默失效（1000 信号）
4. 多窗口关闭清理（跨 Owner effect）
5. `pending_initial` 与 `dirty` 去重

---

# 附录 B：编码规范（强制）

> 违反会导致 panic、abort 或静默错误。写入 `CONTRIBUTING.md` 并在 code review 中检查。

| # | 规范 | 违反后果 |
|---|---|---|
| 1 | **`Drop` 实现中禁用 `read_signal` / `read_memo`，只能用 `try_*`** | double panic → `abort`，无崩溃日志 |
| 2 | **effect 闭包内禁止创建 Signal / Memo / Effect** | 编译期拒绝（结构层 `&mut` 独占） |
| 3 | **effect 内禁止写信号**，只能写属性槽 | 类型系统阻止：`EffectCtx` 无写信号方法 |
| 4 | **effect 内只用 `EffectCtx::set_prop`**，不碰裸 `Window::set_prop` | 绕过存活检查 → 悬垂节点写入 |
| 5 | **effect 不得依赖继承属性**，只依赖显式 Signal / Memo | 依赖追踪不精确 → 漏更新 |
| 6 | **`RefCell` guard 不得跨调用边界**，同一函数内取得并释放 | 借用冲突 panic |
| 7 | **一个属性槽单一写入者**；受控组件必须 Signal 双向绑定 | dev 双写报错 |
| 8 | **ANIM 位只能通过 `AnimationClock` 清理** | 动画结束后 UI 卡在终值 |
| 9 | **浮点量化只在 P3 生成 DrawOp 时进行** | 动画期间 hairline / 曲线视觉抖动 |
| 10 | **样式解析失败时不得 bump epoch** | UI 因临时编辑错误退化到默认样式 |
| 11 | **跨窗口共享信号必须用 `App::create_shared_signal()`** | 窗口关闭导致另一窗口 effect 悬垂 |
| 12 | **effect 清理必须走 `remove_effect_raw`**，不得自行实现 | 依赖边泄漏 |
| 13 | **后端能力不足必须降级，不得 panic** | 生产环境崩溃 |

---

# 附录 C：架构决策索引（ADR 摘要）

| # | 决策 | 理由摘要 |
|---|---|---|
| ADR-1 | Retained Node Tree，禁字符串全局查询 | 查询能力摧毁静态优化；无查询则无 VDOM / diff |
| ADR-2 | 信号响应式替代 WPF 绑定引擎 | Rust 无反射 / GC，绑定引擎成本极高 |
| ADR-3 | 单向数据流（Effect 不得写信号） | 循环依赖结构上不可能；免迭代求解与深度限制 |
| ADR-4 | 每次运行重收集依赖 | 分支依赖自动正确；消灭依赖超集与自适应收紧 |
| ADR-5 | 内部可变性分层（值/依赖 RefCell，结构 `&mut`） | 阶段化，两阶段时间不重叠；避免全局 RefCell |
| ADR-6 | `EffectCtx` 持 `&mut Window`（字段级拆分借用） | 编译器拒绝同时借 `&rx` 与 `&mut App` |
| ADR-7 | 属性 4 级（ANIM 独立） | 用户 set 后：动画该停、signal 该覆盖，语义相反 |
| ADR-8 | 短路求值而非分层 epoch | ANIM/LOCAL 命中即返回，不触及 STYLE，问题消失 |
| ADR-9 | 主题用 theme scope（树形）不加优先级层 | 主题是作用域概念，不是线性优先级 |
| ADR-10 | `style_epoch` O(1) 失效 | 换肤低频；epoch 版本号天然覆盖跨窗口 |
| ADR-11 | 递归 memo 检测 release 也保留 | 不接受 release 里静默栈溢出 |
| ADR-12 | `pending_initial` 队列而非创建即执行 | 立即执行会递归创建且树未稳；队列保序仍在本帧 |
| ADR-13 | memo 重算前置于两类 effect | 否则新 effect 首帧读脏值并破坏拓扑序 |
| ADR-14 | 窗口关闭四步（跨 Owner 扫描 → 摘 effect → dispose → 销毁） | effect 的 window 与 owner 可能不同属 |
| ADR-15 | 格式串 / 格式化函数 / `expr()` 合一为 Rust 闭包 | 依赖自动收集，编译期检查，复用 `format!` |
| ADR-16 | builder API 为主（含宏回退条款） | 组件只跑一次，冗长可接受；>5s 则回退宏 |
| ADR-17 | 不做 Rust 抢占中断 | Rust 无安全抢占点；用 dev 警告 + Worker 引导 |
| ADR-18 | 信号图单线程（逃生口：按窗口分片） | 图操作是 O(变更量)；重活在 P2–P4 已可并行 |
| ADR-19 | 不做 dylib 热重载 | 与代际索引、Owner 树直接冲突，是深水区 |
| ADR-20 | 不做 STYLE 层自动信号追踪 | 为 1% 用例付 100% 成本 |
| ADR-21 | IR 快照量化到 1/256 px + 稳定排序 + 确定性哈希 | 跨平台确定性；量化只在 P3 防动画抖动 |

---

# 附录 D：启动命令

```bash
# 1. 创建工作区
cargo new --lib lieui-core && cd lieui-core

# 2. 按 1.2 节的 workspace 结构扩展

# 3. M0 第一批提交目标（按依赖顺序）
#    lieui-core: id.rs → arena.rs → tree/ → props/ → frame/
#    lieui-render: display_list.rs → caps.rs → backend.rs
#    lieui-render-vello: convert.rs → cpu.rs
#    lieui-platform: lib.rs → ime.rs
#    lieui: lib.rs（门面）

# 4. 验收命令
cargo test --workspace                     # 单测 + 集成 + 快照
cargo test --workspace --release           # ★ release 下也要跑（不变式降级路径）
LIEUI_SNAPSHOT_UPDATE=1 cargo test         # 更新快照基线（需人工 review）
cargo bench                                # 性能基准
```

---

**文档状态**：详细设计 v1.0，对应架构设计 v4.2。
**下一步**：`cargo new lieui-core` —— 从 `id.rs` 与 `arena.rs` 开始。
