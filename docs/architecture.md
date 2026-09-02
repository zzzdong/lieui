# LieUI UI 架构设计 v4.2

**Retained Node Tree + 信号响应式 + 原生 Rust DSL｜vello_cpu → vello_hybrid｜桌面端**

> **v4.2 = 架构冻结终版。** 本轮无架构变更，收入第六轮评审的 10 条实施级指南：可继承属性过滤、Drop 双 panic、受控组件心智模型、量化时机、ANIM 清理、帧标记重置、热重载错误处理、Worker 限流、回调阶段约束、M2 自研预案。
> **【v4.2】** = 本次新增；**[决策]** = 本设计选择；**[待验]** = 需实测。
> 本版为 M0 实施的唯一基准。

---

# 第一部分：论证

## 1. 跨界消失后，v2 的复杂度彻底死亡

这不是"少了一层"，而是**一整类问题的消失**。v2 那一整章（双通道求值、Recording/Steady、依赖超集、自适应收紧）的存在理由：

> **JS 侧的读取无法被 Rust 追踪，所以要在运行时猜出依赖集。**

纯 Rust 后，effect 闭包直接读信号，观察者栈**天然**收集依赖，且每次运行前重收集——**分支依赖自动正确，依赖超集与自适应收紧彻底不需要**。

同一条逻辑消灭的还有：影子表、快照原子性、乐观更新、QuickJS 中断策略、`expr()` 依赖协议、转译管线、Bridge 层、`Persistent` 生命周期。粗估减少 30–35% 工作量。

## 2. 不是 DOM，是 Retained Node Tree

DOM 的致命伤是 `querySelector`——任何代码可随时摸到任何节点，摧毁所有静态优化可能：React 为此引入 VDOM + diff，Svelte 为此放弃查询能力。我们**不提供字符串全局选择器**（会引入路径解析与全树遍历），因此既不需要 VDOM 也不需要 diff。

节点身份由 `NodeId`（代际索引）承载，`Copy + 'static`，跨模块、跨阶段只需传 `u64`。

## 3. 保留 MVVM 分层，抛弃绑定引擎

MVVM 的普适价值是 View/ViewModel 分离。麻烦的是绑定引擎——WPF 式绑定靠 CLR 三件套（反射读属性路径、GC 管订阅、装箱），Rust 一样都没有。**照搬它的机制，是用别人的解药治自己的病。**

## 4. Rust 的自然形态是信号

> **[事实]** Leptos 信号是 arena 索引：`Copy + 'static` 的整数句柄，生命周期由 ownership tree 管理而非 Rust 生命周期或 `Rc<>`，官方文档称之为"让我们能作为 Rust UI 框架存在的创新"[citation:1][citation:4]。

> **[事实]** SolidJS 证明其性能形态：组件函数只运行一次建立依赖图，之后只有受影响的绑定重执行，无 VDOM、无 diff[citation:11]。

**信号响应式 = 自动化的 MVP**：MVP 手动调 `view.set_xxx()`，信号自动化这一步——保留类型安全与显式数据流，消除样板代码。比 WPF Binding 轻一个数量级。

## 5. 单向数据流（核心定位语）

```
Signal ──▶ Memo ──▶ Effect ──▶ 属性槽 ──▶ 渲染
  源        纯计算      终点       终点
```

用 Rust 类型系统在编译期约束：`MemoCtx` 无写信号能力，`EffectCtx` 只读信号且只能写属性槽。**循环依赖在结构上不可能**。

## 6. builder API 而非 RSX 宏（有条件的判断）

> **[决策] DSL 以 builder API 为主 + 轻量宏为辅，不采用 RSX proc-macro 作为主形态。**

proc-macro 的编译时间与错误信息是真实痛点，而它的收益——"声明式结构的书写简洁性"——在我们的模型里被稀释：**组件函数只跑一次**，信号细粒度更新意味着你写一次结构，之后调的是 binding。

> **[v4.1 回退条款] 若 M1 量得"改一行 DSL 结构代码 → 重编译 → 看到效果"超过 5 秒，则 M3 必须引入轻量 `view!` 宏**，即使付出 proc-macro 的编译代价。见 13.6。

## 7. 模式选型

| 模式 | Rust 适配 | 增量更新 | 身份/无障碍 | 结论 |
|---|---|---|---|---|
| Immediate mode（egui） | 极好 | ❌ 全量 | ❌ 无稳定身份 | ❌ 布局悖论[citation:3] |
| DOM + WPF 绑定 | 差 | ✅ | ✅ | ❌ |
| Hybrid widget tree + diff | 好 | ✅ | ✅ | 可行但有 diff 开销[citation:6] |
| **Retained Tree + 信号 + 原生 DSL** | **好** | **✅ 最细** | **✅** | **✅ 采用** |

## 8. 非目标

不提供 DOM/Web API 兼容层 · 不内建脚本引擎（JS 以外部插件形式后置）· 不支持 CSS 全量 · 不内建富文本编辑器 · 不做原生外观 · 不做移动端/Web 目标 · **不做 STYLE 层自动信号追踪** · **不采用 RSX proc-macro 作为主 DSL 形态（M3 可回退）** · **不做信号图多线程（见 16.3 已知限制）** · **不做 dylib 热重载（见 18.4）**。

---

# 第二部分：架构

## 9. 总体分层

```
L5 应用层      Rust 业务代码（Model / ViewModel）+ Rust DSL 视图代码
L4 响应式      ReactiveGraph（!Sync+!Send，内部可变性分层）· Owner 树
L3 节点树      NodeTree + Reconciler · PropertyStore · LayoutStore · DisplayList
L2 服务        App 级：Text(parley) · 字体 · 图像 · 主题 · 样式热重载
               Window 级：Layout(taffy) · A11y(accesskit)
L1 后端        RenderBackend + BackendCaps（vello_cpu / vello_hybrid / Record）
L0 平台        winit · IME · 剪贴板 · 对话框
```

信号图独立于节点树：effect 持有 `NodeId` + `PropKey` + `WindowId`，绑定不是节点的字段，而是信号图里的一条边。

---

## 10. 响应式系统

### 10.1 内部可变性分层

| 层 | 字段 | 可变性 | 理由 |
|---|---|---|---|
| **值层** | `signals` / `memos` 的值 | `RefCell` | 读写贯穿求值阶段 |
| **依赖层** | `subs` / `deps` / `observer_stack` / 各队列 | `RefCell` | `read` 时登记订阅 |
| **结构层** | `owners` 树、槽位创建/销毁 | `&mut self` **独占** | 只在 reconcile 阶段操作，此时无 effect 运行 |

```rust
pub struct ReactiveGraph {
    signals: RefCell<SlotMap<SignalId, SignalSlot>>,
    memos:   RefCell<SlotMap<MemoId,   MemoSlot>>,
    subs:    RefCell<SecondaryMap<RxId, HashSet<EffectId>>>,
    deps:    RefCell<SecondaryMap<EffectId, HashSet<RxId>>>,
    observer_stack:  RefCell<Vec<RxId>>,
    computing:       RefCell<HashSet<MemoId>>,     // 递归检测（release 也生效）
    dirty:           RefCell<Vec<EffectId>>,
    pending_initial: RefCell<Vec<EffectId>>,
    dirty_memos:     RefCell<Vec<MemoId>>,
    owners: SlotMap<OwnerId, OwnerNode>,           // 仅 &mut 访问
}
// !Sync + !Send：编译期强制 UI 线程亲和
```

### 10.2 铁律：RefCell guard 不得跨调用边界

> **[决策] 所有 `borrow()` / `borrow_mut()` 必须在同一函数内取得并释放，**绝不跨越任何用户回调或外部调用边界**。

```rust
pub fn write_signal<T>(&self, id: SignalId, v: T) {
    {
        let mut signals = self.signals.borrow_mut();   // guard 作用域限定
        signals[id].value = v;
    }                                                   // ← 释放后才做下一步
    self.mark_dirty_of(RxId::Signal(id));               // 独立的 borrow
}
```

`flush_memo` 逃生口同样遵守：先释放所有 guard，再进入 `recompute_memo`。

这条规则加上之后，10.1 分层的所有潜在借用冲突一次性消失。**这与全文"状态外挂 + 阶段化"哲学同源**——借用是短暂的、局部的、不跨越边界的。

### 10.3 铁律：求值阶段禁止创建响应式原语

结构层是 `&mut self` 独占，求值阶段（只有 `&self`）无法创建 Signal/Memo/Effect——**编译期拒绝**。

> **[决策] 响应式原语的创建属于结构变更，只能在组件函数执行期间（Reconcile 阶段，持有 `&mut ReactiveGraph`）进行。Effect 闭包内部严禁创建新的响应式原语。**

这不是限制，是架构约束（SolidJS 同样建议）：它使依赖图规模与组件树同构，而非随执行历史无界增长。

### 10.4 `EffectCtx` 的借用拆分

Effect 本质是"读全局信号图，写特定窗口的属性槽"，因此持有**路由后的 `&mut Window`**，通过**字段级拆分借用**构造：

```rust
impl App {
    fn split_borrows(&mut self) -> (&ReactiveGraph, &mut Vec<Window>) {
        (&self.rx, &mut self.windows)      // 字段级拆分，Rust 允许
    }
}
fn run_effects(app: &mut App) {
    let (rx, windows) = app.split_borrows();
    for id in rx.take_dirty() {
        let w = &mut windows[rx.window_of(id)];
        rx.run_effect(id, EffectCtx { graph: rx, window: w });
    }
}

pub struct EffectCtx<'a> {
    graph:  &'a ReactiveGraph,
    window: &'a mut Window,        // 不是 &mut App：避开借用冲突
}
```

### 10.5 依赖收集：观察者栈 + 递归检测 + RAII 守卫

```rust
impl ReactiveGraph {
    fn track(&self, source: RxId) {
        if let Some(observer) = self.observer_stack.borrow().last() {
            self.subs.borrow_mut()[source].insert(*observer);
            self.deps.borrow_mut()[*observer].insert(source);
        }
    }

    pub fn read_signal<T: Clone>(&self, id: SignalId) -> T {
        self.track(RxId::Signal(id));
        self.signals.borrow()[id].value.clone()
    }

    pub fn read_memo<T: Clone + PartialEq>(&self, id: MemoId) -> T {
        self.track(RxId::Memo(id));
        if self.memos.borrow()[id].stale { self.recompute_memo(id); }
        self.memos.borrow()[id].value.clone()
    }

    fn recompute_memo(&self, id: MemoId) {
        assert!(!self.computing.borrow().contains(&id),
            "递归 memo：memo {id:?} 在其自身求值中被读取。\
             Memo 必须是纯计算且不可自引用——需要循环状态请用 Signal + Effect。");

        // RAII 守卫：panic 时自动清理，杜绝 computing 残留误报
        let _guard = ComputingGuard::new(self, id);
        self.observer_stack.borrow_mut().push(RxId::Memo(id));
        self.deps.borrow_mut()[RxId::Memo(id)].clear();     // ← 每次重收集
        let compute = self.memos.borrow()[id].compute;
        let v = compute(self);                              // 可能 panic
        self.observer_stack.borrow_mut().pop();
        let mut memos = self.memos.borrow_mut();
        memos[id].value = v;
        memos[id].stale = false;
    }
}

/// Drop 时无条件从 computing 与 observer_stack 移除
struct ComputingGuard<'a> { graph: &'a ReactiveGraph, id: MemoId }
impl Drop for ComputingGuard<'_> {
    fn drop(&mut self) {
        self.graph.computing.borrow_mut().remove(&self.id);
        if self.graph.observer_stack.borrow().last() == Some(&RxId::Memo(self.id)) {
            self.graph.observer_stack.borrow_mut().pop();
        }
    }
}
```

**每次运行前重收集依赖**（`deps.clear()`）让**分支依赖自动正确**——本次走 A 分支就只订阅 A 分支的信号。这是消灭"依赖超集策略 + 自适应收紧"的机制基础。

递归检测在 release 也保留（`computing` 是 O(1) HashSet）——不接受"release 里静默栈溢出"。

### 10.6 相等性策略

| API | 语义 | 适用场景 |
|---|---|---|
| `write_signal(v)` | **写入即传播**，不检查 | 默认。事件、计数器、命令式更新 |
| `write_signal_if_changed(v)` | `T: PartialEq`，相等则跳过 | **高频**：鼠标坐标、滚动偏移、动画进度 |
| `write_signal_unchecked(v)` | 强制传播（绕过容差） | 需穿透浮点容差 |
| `signal_f32(v, epsilon)` | **独立信号类型**，epsilon 存于 slot | 浮点几何量，配合 `if_changed` |

> **[决策] `signal_f32` 是独立信号类型而非写入辅助函数**——容差是信号固有属性，应随信号存储。

**API 文档必须写明**：

```
鼠标移动 / 拖拽 / 滚动 / 动画进度  → write_signal_if_changed
按钮点击 / 命令执行 / 状态机切换    → write_signal
浮点几何量（位置、缩放、透明度）    → signal_f32 + write_signal_if_changed
```

### 10.7 `EffectCtx` 安全 API

> **[决策] `EffectCtx` 只暴露一个 `set_prop`，内部自动做存活检查并返回 `bool`。effect 路径上不存在裸写。**

```rust
impl EffectCtx<'_> {
    /// 唯一写入口：节点失效或信号已 dispose 时返回 false 并跳过
    pub fn set_prop<T>(&mut self, node: NodeId, key: PropKey<T>, v: T) -> bool;
    pub fn try_read<T: Clone>(&self, id: SignalId) -> Option<T>;   // dispose 后 None
    pub fn read_or<T: Clone>(&self, id: SignalId, default: T) -> T;
}
```

裸 `Window::set_prop` 仍存在（供 reconcile 与 Presenter 使用），但**不在 `EffectCtx` 上暴露**。

### 10.8 dispose 后访问的明确语义

| API | dispose 后行为 | 用途 |
|---|---|---|
| `read_signal` / `read_memo` | **panic**（debug 与 release 一致） | 快速失败，暴露 bug |
| `try_read_signal` / `try_read_memo` | 返回 `None` | **Drop 实现与防御性读取** |
| `EffectCtx::try_read` | 返回 `None` | effect 内推荐写法 |
| `EffectCtx::read_or` | 返回默认值 | effect 内推荐写法 |

> **[决策] 悬垂句柄通过代际索引检测——`SignalId` 的 generation 与槽位不匹配即视为已 dispose。** 常规 `read` 一律 panic（不做静默降级，静默错值比崩溃更难查）；需要容错的场景显式用 `try_*`。

### 10.9 铁律：Drop 中禁用 panic 式读取【v4.2 新增】

> **[决策] 任何 `Drop` 实现中严禁调用 `read_signal` / `read_memo`，必须且只能使用 `try_read_signal` / `try_read_memo`。**

**理由**（评审 2.2 的关键洞见）：组件销毁时栈可能正在展开（例如 Owner 销毁引发级联 Drop）。若此时 `Drop` 内部调用 `read_signal` 而信号已被前置销毁，panic 会变成 **double panic → 直接 `abort`**，连崩溃日志都留不下。

由于 10.8 规定 `read_signal` 在 dispose 后 panic，这条铁律是它的**必然推论**而非额外约束。写入项目编码规范，并在 code review 中检查。

```rust
impl Drop for MyComponent {
    fn drop(&mut self) {
        // ❌ let v = self.sig.read();          // 可能 double panic → abort
        if let Some(v) = self.sig.try_read() {  // ✅
            cleanup(v);
        }
    }
}
```

### 10.10 铁律：Effect 不得依赖继承属性

> **[决策] Effect 只能依赖显式 Signal / Memo。** 继承属性与 theme scope 变量是**渲染期解析**的（P2/P3），不是响应式源，不参与依赖追踪。

若某派生逻辑确实依赖继承值（如"字号 = 继承字号 × 1.2"），必须把该继承值提升为显式 Signal（在样式解析时写入），而非让 effect 直接读继承属性。

### 10.11 dispose 与 effect 移除的统一实现

```rust
pub fn dispose_owner(&mut self, id: OwnerId) {   // &mut：结构阶段
    // ① 从 dirty / pending_initial 队列移除属于该 Owner 的 effect
    // ② 移除依赖边（subs / deps 双向清理）
    // ③ 释放槽位（generation 自增，悬垂句柄静默失效）
}
pub fn remove_effect_preserving_owner(&mut self, e: EffectId);  // 窗口关闭用
```

> **[强制] 三条路径（`dispose_owner` / `remove_effect_preserving_owner` / 窗口关闭）必须共用同一个 `remove_effect_raw(e)` 私有函数**：出队（dirty + pending_initial）→ 清 `deps[e]` → 从每个源的 `subs` 中摘除 → 释放槽位。任一条路径自己实现清理逻辑都会导致不一致的泄漏。

### 10.12 RefCell panic 的降级

"求值阶段无结构变更"是框架不变式，因此 borrow 失败是**框架 bug**。但生产环境不该因一个 bug 崩掉应用：

| 构建 | 行为 |
|---|---|
| **debug** | **panic**——立即暴露，附完整上下文 |
| **release（默认）** | `try_borrow` 失败 → 记录诊断 → **跳过本次 effect 并标记下帧重试** |
| **release + `strict-invariants`** | 强制 panic |

对 `stale` 位、dirty 计数等简单标量用 `Cell` 而非 `RefCell`，减少运行时检查。

---

## 11. Reconcile 策略

### 11.1 组件实例 ↔ Owner 映射

```
组件实例 ⟺ Owner 节点 ⟺ 节点子树根
```

| 事件 | 动作 |
|---|---|
| 挂载 | 创建 Owner → 在 Owner 上下文跑组件函数 → 节点记录 `owner` |
| 卸载 | `dispose_owner` → 级联释放 → 移除子树 |
| 更新 | 复用同一 Owner，**不重建信号**（只重跑函数产出新描述） |

### 11.2 Key 策略

| 场景 | 策略 |
|---|---|
| key 相同 + 类型相同 | **移动**：保留 `NodeId`，身份/焦点/动画状态全保留 |
| key 相同 + 类型不同 | 销毁重建 |
| 无 key | 按位置降级 + **dev 警告** |
| key 重复 | **dev 报错** |

### 11.3 帧内求值顺序与队列去重

```
① 重置所有 effect 的 RAN_THIS_FRAME 位     ← 【v4.2】帧首统一清除
   冻结 dirty 快照
② Reconcile
   a. 创建 / 移动 / 销毁节点
   b. 跑组件函数 → 新 effect 进入 pending_initial（按挂载拓扑序入队）
   c. dispose 被移除组件的 Owner
③ 从 dirty 队列移除已销毁 Owner 的 effect
④ 拓扑序重算 dirty memos
   （依赖闭包 = pending_initial ∪ dirty 两者的依赖并集，深度升序）
⑤ 运行 pending_initial（深度优先拓扑序，父先于子）
⑥ 运行 dirty effects \ pending_initial（位标记去重）
⑦ 应用文本编辑（IME / 输入）
```

> **[v4.2 补充] `RAN_THIS_FRAME` 在 ① 帧首统一重置**，在冻结 dirty 快照之前。这防止"上帧残留的标记导致本帧 effect 被误跳过"——一个实现时极易遗漏、且症状（某些更新莫名丢失）极难诊断的陷阱。

**去重实现**用位标记而非集合运算，避免每帧分配：

```rust
bitflags! { struct EffectFlags: u8 {
    const IN_INITIAL = 1<<0;
    const IN_DIRTY   = 1<<1;
    const RAN_THIS_FRAME = 1<<2;
}}
// 每个 effect 槽位持有一个 flags: Cell<EffectFlags>
// ⑤ 运行完置 RAN_THIS_FRAME；⑥ 跳过已置位的
```

### 11.4 节点移动后的 `inherit_dirty`

移动节点 → 层级变化 → 继承属性与 theme scope 解析结果可能变化。

```
移动节点 → 标记该子树 inherit_dirty
        → 属性解析时：清 resolved 缓存，重新沿树求值
```

### 11.5 虚拟化列表

作为 P1-② reconcile 的一部分更新，不独立处理——天然继承 11.3 的顺序保证。

```
Viewport = 可视区 + 上下 buffer
  进入范围 → 回收池取节点或新建 → 绑定数据（不重建信号）
  离开范围 → 解除绑定 → 节点进回收池（不销毁）
  数据源变更 → ListSignal 产出最小 move/insert/remove（key 比对）
```

---

## 12. 属性系统

### 12.1 四级优先级

```rust
bitflags! { pub struct ValueSource: u8 {
    const DEFAULT = 1<<0;   // 控件类型默认值
    const STYLE   = 1<<1;   // 主题 / 静态样式 / 模板 setter
    const LOCAL   = 1<<2;   // 直接写 / 信号 effect 写入
    const ANIM    = 1<<3;   // 动画驱动
}}
```

**ANIM 必须独立**：用户手动 set 后两种"动态"来源行为**相反**——动画应停止（写 LOCAL 清 ANIM），信号 effect 下次求值应覆盖（数据流覆盖用户输入是期望行为）。

### 12.2 短路求值

```rust
fn resolve(node: NodeId, key: PropKeyId) -> PropValue {
    // 优先级从高到低短路：命中即返回，不再解析下层
    if let Some(v) = raw_slot(node, key, ANIM)   { return v; }   // 不查 epoch
    if let Some(v) = raw_slot(node, key, LOCAL)  { return v; }   // 不查 epoch
    // ── 以下才涉及继承与主题，才需要 epoch 检查 ──
    let cached = resolved_slot(node, key);
    if cached.epoch == theme.epoch && !inherit_dirty(node) { return cached.value; }
    let v = resolve_inherited_or_themed(node, key);   // 沿树向上 + theme scope
    write_resolved_slot(node, key, v, theme.epoch);
    v
}
```

**关键洞察**：最终值若来自 ANIM 或 LOCAL，解析时**根本不会触及 STYLE/DEFAULT 层**，因此不受 epoch 影响——不需要为它们维护 epoch，也不需要在主题切换时失效它们。**resolved 缓存只为"走继承/主题路径"的属性建立。**

### 12.3 可继承属性的失效传播【v4.2 补充】

```rust
pub struct InheritSlot {
    resolved: PropValue,
    epoch:    u32,           // 主题 epoch
    source:   Option<NodeId>, // 继承来源节点（路径压缩用）
    src_gen:  u32,            // 来源节点的 generation
}
```

**解析算法**：

```
1. 本节点有显式 STYLE/DEFAULT 值 → 直接返回（source = self）
2. 否则沿树向上找最近的显式值 → 返回（source = 那个节点）
3. 缓存 (value, source, src_gen)
```

**路径压缩**：下次解析时若 `cached.source` 仍存活（generation 匹配）且该来源的 `value_epoch` 未变 → 直接复用，不重新沿树向上。

> **[v4.2 修订] `value_epoch += 1` 与子树冒泡仅在写入**可继承属性**时触发。**

```rust
impl PropertyStore {
    fn set_raw<T>(&mut self, node: NodeId, key: PropKey<T>, src: ValueSource, v: T) {
        write_slot(node, key, src, v);
        // 【v4.2】仅可继承属性才触发失效传播
        if key.is_inheritable() {
            self.value_epoch[node] += 1;
            self.mark_subtree_inherit_dirty(node);   // 冒泡至最近继承边界
        }
    }
}
// PropKey 的 INHERITABLE 位是编译期常量，判定零成本
```

**为什么必须过滤**（评审 2.1）：`padding`、`width`、`background` 这类**不可继承**属性的写入若也触发 `value_epoch += 1` 与子树冒泡，会导致大量无谓的继承缓存失效。而这类写入在 UI 运行中远比可继承属性（字号、前景色、字体族）频繁。

**已评估但不做**：按层细分 `value_epoch`（区分 LOCAL/STYLE 写入）。评审 4 指出父节点 LOCAL 写入会让继承 STYLE 的子节点多余失效——**成本仅为一次节点查找**，属于过度设计，明确不做。

### 12.4 主题：`style_epoch` O(1) 失效

```rust
pub struct ThemeRegistry { scopes: HashMap<ScopeId, ThemeScope>, epoch: u32 }
// 换肤 / 样式热重载 = epoch += 1
```

配合 12.2 的短路求值，epoch 只影响真正走了 STYLE/DEFAULT 路径的属性，**O(1) 触发、O(受影响属性) 重解析**。

| 场景 | 机制 | 频率 |
|---|---|---|
| **整体换肤**（浅↔深、切换主题包） | `epoch += 1` + 惰性重解析 | 低频，O(1) 触发 |
| **单变量响应式**（跟随系统强调色） | 该变量是信号 → 显式 effect 写 LOCAL | 每变量一个 effect |
| **局部主题覆盖** | theme scope 沿树向上查找最近定义 | — |

> **[决策] 不提供"STYLE 层自动订阅信号"**——让每个属性槽背负主题订阅开销，是为 1% 用例付 100% 成本。

### 12.5 动画层的清理【v4.2 补充】

12.1 规定了"写 LOCAL 清 ANIM 位"（用户手动打断动画），但**动画自然结束**时同样需要清理：

```rust
impl AnimationClock {
    /// 动画自然结束：移除 ANIM 位，短路求值自动回落到 LOCAL / STYLE
    pub fn finish(&mut self, anim: AnimId) {
        let (node, key) = self.target_of(anim);
        self.dom.clear_anim(node, key);      // 清 ANIM 位 + 原始值
        self.remove(anim);
    }
    /// 动画取消：同样清理，但不写入终值
    pub fn cancel(&mut self, anim: AnimId) { /* 同上，不写终值 */ }
}
```

> **[决策] `clear_anim` 是唯一合法的 ANIM 位清除入口**，由 `AnimationClock` 在 `finish` / `cancel` / `interrupt` 三条路径调用。手写 ANIM slot 会绕过这个不变量——因此 `set_raw(.., ANIM, ..)` 是 `AnimationClock` 的私有方法，对外不暴露。

**语义**：ANIM 位清除后，短路求值自动回落到 LOCAL（有绑定/用户输入）或 STYLE/DEFAULT（主题值）——无需任何显式恢复操作。

### 12.6 列式存储

```rust
pub struct PropertyStore {
    f32s: Column<f32>, u32s: Column<u32>,
    colors: Column<Color>, strings: Column<SharedString>,
    any: Column<Box<dyn AnyVal>>,      // 冷门类型兜底
    meta: MetaTable,                   // ValueSource + writer + value_epoch
    inherit: Column<InheritSlot>,      // 继承/主题解析缓存
}
pub struct PropKey<T> {
    slot: u16,
    flags: PropFlags,                  // 【v4.2】含 INHERITABLE 位，编译期常量
    _ty: PhantomData<T>,
}
```

`PropValue` 枚举保留为列式存储的类型擦除边界，不承载序列化语义。

---

## 13. Rust DSL

### 13.1 基本形态

```rust
Column::new()
    .padding(12)
    .gap(8)
    .child(Text::new().text(&name).font_size(18))
    .child(Button::new()
        .label("点我")
        .on_click(move |cx| count.update(|n| *n += 1)))
    .when(show_footer, |c| c.child(Footer::new()))
```

### 13.2 绑定：三条合一于闭包

```rust
Text::new().text(&signal)                                          // 直接绑定
Text::new().text_fn(move || format!("{} {}", first.get(), last.get()))  // 计算绑定
Text::new().text(&cx.memo(move || format!("{} {}", first.get(), last.get())))  // Memo 复用
```

> **[决策] v3.3 的三条绑定路径（格式串 / Rust 格式化函数 / `expr()`）在 v4.0 起合一为 Rust 闭包。** 依赖自动收集且精确，无漏声明风险，编译期类型检查，复用 `format!` 生态。

### 13.3 结构控制：运行时组件

```rust
Show::when(&cond).child(|| Text::new().text(&label))
For::each(&items).key(|it| it.id).view(|it| Row::new(it))
Match::on(&state)
    .arm(State::Idle,  || Text::new("待机"))
    .arm(State::Busy,  || Spinner::new())
```

### 13.4 组件与 ViewModel

```rust
pub fn counter_view(cx: &mut BuildCtx, vm: &CounterVm) -> impl View {
    Column::new()
        .child(Text::new().text_fn({
            let n = vm.count;                    // SignalId 是 Copy + 'static
            move || format!("点了 {} 次", n.get())
        }))
        .child(Button::new().label("+1")
            .on_click({
                let n = vm.count;
                move |_| n.update(|v| *v + 1)
            }))
}
```

**信号是 `Copy + 'static`**，捕获进闭包无需 clone、无需 `Rc<RefCell>`。嵌套闭包（`For::view` 里再套 `on_click`）偶尔需要显式生命周期标注，这是已知摩擦点。

### 13.5 单一写入者原则

> **[决策] 一个属性槽在任一时刻只能有一个写入者。** Presenter 与信号绑定是互斥的，不是叠加的。

| 场景 | 写入者 | 说明 |
|---|---|---|
| 数据展示、表单、**受控组件** | **effect**（绑定信号） | 响应式自动更新 |
| 画布、拖拽、编辑器光标 | **Presenter**（`ViewHandle::set_prop`） | 命令式直接写 LOCAL |
| 同一属性两者都写 | **禁止** | dev 模式检测并报错 |

```rust
// Presenter 侧（仅用于非数据驱动场景）
let h: ViewHandle = cx.handle_of(container);
h.set_prop(PADDING, 24.0);
h.find("status_label").set_prop(TEXT, "已保存");
```

**检测机制**：属性槽的 meta 中记录 `writer: WriterKind::{None, Effect(EffectId), Manual}`。effect 绑定时若发现 `Manual` 则 dev 报错；Presenter 写入时若发现 `Effect` 则 dev 报错。release 静默（后写覆盖），但 dev 期必然暴露。

### 13.6 受控组件：必须走 Signal 双向绑定【v4.2 新增】

评审 2.3 指出一个极易踩的坑：实现 `Input` / `Slider` 等**受控组件**时，开发者会习惯性地在 `on_input` 回调里用 Presenter 直写 `LOCAL` 更新 UI——这会与外部的 Signal 绑定冲突，触发 13.5 的双写报错。

> **[决策] 受控组件必须通过 Signal 双向绑定实现，严禁在受控组件中混用 Presenter 直写。**

```rust
// ✅ 正确：Signal 双向绑定
Input::new()
    .value(&vm.text)                                        // effect 写 LOCAL
    .on_input(move |s| vm.text.set(s))                      // 用户输入写回 signal
                                                            // → signal 变化 → effect 重写 LOCAL

// ❌ 错误：Presenter 直写（触发 dev 双写报错）
Input::new()
    .value(&vm.text)                                        // effect 持有 writer
    .on_input(move |s| handle.set_prop(TEXT, s))             // Presenter 争抢 writer
```

**心智模型**：受控组件的"当前值"唯一真相是 Signal。用户输入 → 写 Signal → effect 写回 UI。**这是一条单向环路，不是两条并行路径。**

**Presenter 的适用场景仅限"非数据驱动"的纯命令式交互**：画布绘制、拖拽过程中的临时位置、编辑器光标闪烁、滚动容器的瞬时偏移。这些状态的共同点是**不进入业务数据流**。

### 13.7 宏的角色与可回退条款

```rust
view! {
    Column { padding: 12, gap: 8 } {
        Text { text: label, font_size: 18 }
        Button { label: "点我", on_click: handler }
    }
}
```

宏是**可选糖，展开为 builder 调用**，不做 RSX 式完整语法变换——保持编译速度、错误信息可读、IDE 可分析。

> **[可回退条款] 第 6 节"builder 优于 RSX"的判断必须在 M3 用示例应用验证。**
>
> **触发条件**：若 M1 量得"改一行 DSL 结构代码 → 重编译 → 看到效果"**超过 5 秒**，则 M3 必须引入轻量 `view!` 宏（而非 builder 为主），即使付出 proc-macro 的编译代价。
>
> 理由：5 秒是"保持心流"的上限。结构改动频率在 UI 开发中极高，若反馈循环过长，DSL 的表达力优势会被开发节奏的损耗吃掉。

### 13.8 变更批次

> **[决策] 删除跨语言序列化形式，保留批处理语义。**

```rust
impl App {
    pub fn batch(&mut self, f: impl FnOnce(&mut BatchCtx)) {
        let _tx = self.begin_batch();     // 失败自动回滚到批次前
        f(&mut BatchCtx { .. });
        self.commit_batch();              // 一次性 reconcile + 脏标记
    }
}
```

原子性（要么全应用要么全回滚）对 reconcile 正确性有价值，与是否有 JS 无关。删除的是 `Vec<u32>` 编解码。

---

## 14. 帧管线

```
P0 INPUT     ① 重置 RAN_THIS_FRAME 位
             ② 排空 Worker 消息通道（单帧上限 N 条）
             ③ 原始事件 → 命中测试 → 路由（Tunnel → Target → Bubble）
             ④ 检查 StyleWatcher（文件变化 → 解析 → epoch += 1）

P1 UPDATE    前置：执行事件回调 → 应用变更批次（事务性）
             ① 冻结 dirty 快照
             ② Reconcile（建/移/删 · 跑组件函数 → pending_initial · dispose）
             ③ 清理 dirty 队列中已销毁的 effect
             ④ 拓扑序重算 dirty memos（闭包 = pending_initial ∪ dirty）
             ⑤ 运行 pending_initial（深度优先拓扑序）
             ⑥ 运行 dirty effects \ pending_initial（位标记去重）
             ⑦ 应用文本编辑（IME / 输入）

P2 LAYOUT    parley 测度 → taffy 求解 → LayoutStore        ← 全程原生 f32
P3 PAINT     RenderObject 遍历 → DisplayList → 量化 1/256px ← 量化只在这里
P4 COMPOSITE DisplayList → RenderBackend → 呈现
P5 A11Y      accesskit 增量更新
```

### 14.1 Worker 消息的单帧上限【v4.2 补充】

Worker 结果在 P0-② 统一排空保证了确定性，但大量积压会延迟整帧开始。

> **[决策] 单帧处理上限 N 条（默认 256，可配置），剩余留到下帧。** 超限时 dev 模式告警——持续积压意味着 Worker 产出速率超过 UI 消费能力，是需要关注的信号而非应被静默吸收的常态。

### 14.2 回调阶段约束（含 `flush_memo` 安全性）【v4.2 补充】

> **[决策] 用户回调只能在 P0-③ 与 P1 前置阶段触发。P1-④⑤⑥ 的 effect 执行期间不存在任何用户回调路径。**

这保证了 `flush_memo` 逃生口的安全性：effect 执行期间 `observer_stack` 可能非空，若此时用户回调调用 `flush_memo` 会撞上活跃的追踪上下文（甚至误把该 memo 登记为某个 effect 的依赖）。阶段约束把这条路径**从架构上消除**，而不是靠运行时检查。

### 14.3 回调期间时序语义

| 场景 | 语义 |
|---|---|
| 回调写信号后立即读该信号 | **立即生效**（直读 slot） |
| 回调写信号后立即读依赖它的 Memo | **旧值**——Memo 延迟到 P1-④ |
| 需要 Memo 新值 | `flush_memo(id)` 逃生口（**仅在回调阶段可用**） |
| 回调写信号触发另一个回调 | **不允许**——单向数据流禁止 effect 写信号 |

**回调不重入管线是硬规则**：回调只写信号/入队，返回后才推进。

---

## 15. 多窗口

```
App（进程单例，!Send）
  ├─ ReactiveGraph（全局一张信号图）
  ├─ ThemeRegistry / TextService / ImageCache / StyleWatcher / WorkerPool
  └─ Vec<Window>
       └─ Window { Tree, PropertyStore, LayoutStore, DisplayList,
                   root_owner: OwnerId, surface }
```

### 15.1 Effect 路由

```rust
pub struct EffectSlot {
    owner:  OwnerId,
    node:   NodeId,
    window: WindowId,     // 由框架在创建时从 NodeId 推导并填充
    key:    PropKeyId,
}
```

### 15.2 Owner 分类与跨窗口订阅检查

| 类型 | 生命周期 | 归属 | 示例 |
|---|---|---|---|
| **Window Owner** | 窗口关闭即销毁 | `Window::root_owner` | 窗口根组件及其子树 |
| **App Owner** | 应用退出才销毁 | `App` | 业务 ViewModel、全局设置、跨窗口共享状态 |

> **[决策] 跨窗口共享的信号必须显式创建在 App Owner 下**（`App::create_shared_signal()`）。

**dev 检测规则**：

```
创建 effect 时（结构阶段，可精确判定，零运行时成本）：
  若 effect 订阅了信号 S，且 S 的 Owner 在窗口 W 的 root_owner 子树下，
  而 effect 自身不在 W 的 root_owner 子树下
  → dev 警告："信号 S 属于窗口 W 的 Owner，但被窗口外的 effect 订阅。
              窗口关闭时 S 会被销毁，导致悬垂。请改用 App::create_shared_signal()。"
```

**只在跨 Owner 边界时告警**：同一窗口内的 Window Owner 信号被同窗口 effect 订阅完全合理，不告警。

### 15.3 窗口关闭流程

```rust
impl App {
    pub fn close_window(&mut self, wid: WindowId) {
        // ① 跨 Owner 扫描：找出所有 window == wid 的 effect
        let orphans = self.rx.effects_with_window(wid);
        // ② 只摘除 effect，不销毁其 Owner（Owner 可能要继续存活）
        for e in orphans { self.rx.remove_effect_preserving_owner(e); }
        // ③ dispose 窗口根 Owner → 级联销毁窗口内组件及其信号/memo/effect
        self.rx.dispose_owner(self.windows[wid].root_owner);
        // ④ 销毁节点树与 Window 结构
        self.windows.remove(wid);
    }
}
```

**关键**：`remove_effect_preserving_owner` 与 `dispose_owner` 是两个不同操作——混用会导致要么泄漏（只做 ③）要么误杀（只做 ②）。两者内部共用 10.11 的 `remove_effect_raw`。

### 15.4 其余决策

- 文本服务 **App 级单例**（parley 要求 `FontContext`/`LayoutContext` 粗粒度共享）
- 线程：**单 UI 线程 + 多窗口**（winit 支持，跨窗口拖拽简单）
- **样式热重载天然覆盖所有窗口**——`style_epoch` 是 App 级，各窗口 `PropertyStore` 的 resolved 缓存都带 epoch，全部自然失效

---

## 16. 线程模型

v3.3 的 QuickJS 中断策略整章删除——没有脚本引擎，就没有"脚本阻塞 UI"的问题。

```
┌──────────── UI 线程（App 实例，!Send）────────────┐
│  winit 事件循环（多窗口）                           │
│  P0（含排空 Worker 通道）→ P1 → P2 → P3 → P4 → P5   │
│              ▲                                     │
│              │ 消息（无锁 ring buffer）             │
├──────────────┼─────────────────────────────────────┤
│ Worker 池（Rust 原生）：解码 · 字体 · IO · 网络      │
└────────────────────────────────────────────────────┘
```

### 16.1 硬规则

1. **信号读写严格限定 UI 线程**——`ReactiveGraph: !Sync + !Send` 编译期强制
2. Worker 结果通过 channel 回 UI 线程，在 **P0-② 统一排空**（单帧上限 N 条）后写入
3. 长耗时 Rust 计算**必须**下放 Worker 池

### 16.2 用户代码阻塞的兜底

- **dev 模式**：对超过预算（60Hz 8ms / 120Hz 4ms）的回调输出警告 + 调用栈
- **release**：不做强制中断（Rust 没有安全抢占点，强行中断破坏内存安全）
- 真需要长任务 → `cx.spawn()` 下放 Worker，返回后回 UI 线程写信号

### 16.3 已知限制：信号图单线程

> **[决策] 明确记录：信号图是单线程的，这是设计约束而非临时状态。**

**为什么可接受**：桌面 UI 的信号图操作是 O(变更量) 而非 O(树规模)——一次状态更新只触及依赖链上的少量节点。真正的重活（布局、绘制、文本排版、图像解码）**已经在架构上可并行**：P2–P4 天然可以多线程，M6 就做这个。

**若未来确实需要并行信号处理**，逃生口是**按窗口分片**（每个窗口一张子图 + 跨窗口信号走消息传递），**不是**给现有全局图加锁。理由：加锁会让每次 signal 读写付出同步成本，而分片只在跨窗口边界付出成本——后者罕见，前者频繁。

**注意**：分片逃生口会破坏"App Owner 跨窗口共享信号"的直接性，届时需要在"共享便利性"与"并行度"之间重新权衡。**当前不预设这个权衡，等实测数据说话。**

---

## 17. 布局、文本、渲染

### 17.1 布局与文本

taffy 求解 + parley 两段式测度；measure 查 `LayoutStore` 缓存（键 `text_hash + style_hash + wrap_mode + max_width`）；重排边界只处理脏子树。

文本：`FontContext`/`LayoutContext` App 级单例长期持有；重绘判定用 `PlainEditor::generation()`；富文本走「buffer + 属性区间 + 派生 Layout」而非 `PlainEditor`；parley 三重 `&mut` 靠所有权隔离化解（缓冲在 `TextBuffers`，资源在 `TextService`）。

### 17.2 渲染：DisplayList + 能力降级

```rust
pub struct BackendCaps {
    pub mask_layers: bool, pub filter_graph: bool,
    pub blend_modes: BitSet<BlendMode>,
    pub max_image_atlas: u32, pub max_gradient_ramp: u32,
}
```

| 不支持 | 降级 |
|---|---|
| Mask layers | clip + 预渲染图层 |
| Filter graph | 单 primitive 或预渲染为图像 |
| 特定 blend mode | SrcOver + dev 警告 |

**为什么必须有自己的 DisplayList**：linebender 明确预告要重设计 vello API，`sparse_strips/` 自述 "not yet suitable for production use"，`vello_common` 声明无稳定 API 表面。直接散落 `RenderContext` 调用会让那次重设计的成本变成整个渲染层。

### 17.3 浮点量化的精确时机【v4.2 补充】

19.1 规定 DisplayList 坐标量化到 1/256 px 以保证 IR 快照确定性。但量化时机必须精确：

> **[决策] 量化仅在 P3 PAINT 阶段生成最终 `DrawOp` 时进行。P0–P2 全程保持原生 `f32` 精度。**

**理由**（评审 2.4）：若在动画插值的中间过程就量化，高 DPI 屏幕下的极细线条（hairline）与复杂贝塞尔曲线会在动画期间产生**视觉抖动（jitter）**——因为截断误差在逐帧累积并与插值叠加。

```
P1 动画插值        → 原生 f32
P2 布局求解        → 原生 f32
P3 生成 DrawOp     → 量化到 1/256 px  ← 唯一量化点
P3 排序 + 哈希     → 基于量化后的值，保证确定性
```

### 17.4 依赖锁定

Cargo.lock 对库依赖不生效。实际流程：

1. **workspace 根 `Cargo.lock` 纳入版本管理**
2. CI 对每个依赖变更运行编译测试 + IR/像素快照比对
3. 维护 `BackendCaps` 与 vello 版本的适配表
4. 升级必跑：IR 快照 → 像素快照

### 17.5 vello_hybrid 切换判据

| 判据 | 阈值 |
|---|---|
| 目标场景（图像/渐变/filter 密集）帧时间改善 | **≥ 30%** |
| 双后端像素一致性 | 参考图集 100% 通过 |
| 不支持能力 | 降级后视觉可接受 |

---

## 18. 样式热重载

纯 Rust 路径唯一真正的痛点是**失去热重载**。自绘控件 + 主题系统意味着大量视觉微调（圆角、阴影、间距、配色、hover 态）。

### 18.1 样式文件 schema

```ron
// theme.ron
ThemeSet {
    default_theme: "light",
    themes: {
        "light": Theme {
            // ① 全局变量 → 填充到 root ThemeScope
            vars: {
                "color.bg":     "#ffffff",
                "color.fg":     "#1a1a1a",
                "color.accent": "#0066cc",
                "space.sm":     8.0,
                "space.md":     12.0,
                "radius.md":    6.0,
                "font.body":    14.0,
            },
            // ② 按控件类型覆盖 → 按 ElementTypeId 索引的样式表
            rules: {
                "Button": Style {
                    padding: [8.0, 16.0],
                    bg:     "$color.accent",      // $ 引用变量
                    fg:     "$color.bg",
                    radius: "$radius.md",
                    // ③ 伪状态覆盖 → 按 PseudoClassSet 位掩码索引
                    states: {
                        "hover":  Style { bg: "#0055aa" },
                        "active": Style { bg: "#004499" },
                        "disabled": Style { fg: "#999999" },
                    },
                },
                "Text": Style { font_size: "$font.body", fg: "$color.fg" },
            },
        },
        "dark": Theme { /* 同构 */ },
    },
}
```

**到 `ThemeRegistry` 的映射**：

| RON 段 | 映射到 | 作用 |
|---|---|---|
| `vars` | `ThemeRegistry::scopes[ROOT]` | 全局 theme scope，供沿树向上查找 |
| `rules[K]` | 按 `ElementTypeId` 索引的样式表 | 控件默认样式（STYLE 层） |
| `states[S]` | 按 `PseudoClassSet` 位掩码索引 | hover/active/disabled 等状态覆盖 |
| `"$var"` | 解析时查 scope 变量 | 引用而非复制，改一处全生效 |

**热重载流程**：

```
文件 mtime 变化 → 重新解析 RON → 替换 ThemeRegistry 内容 → epoch += 1
→ 各窗口 resolved 缓存自然失效 → P2/P3 重解析 → 下一帧生效
```

**无需重建节点树、无需重跑组件函数、无需重编译**，且天然覆盖所有窗口。

### 18.2 解析错误处理【v4.2 新增】

用户编辑样式文件时必然会出现语法错误或引用不存在的变量：

> **[决策] 解析失败 → 保留旧主题 → 记录错误 → 等待下一次 mtime 变化。绝不因临时错误回退到默认样式。**

```rust
impl StyleWatcher {
    fn try_reload(&mut self, registry: &mut ThemeRegistry) -> Result<(), StyleError> {
        let text = read_to_string(&self.path)?;
        let parsed = ron::from_str::<ThemeSet>(&text)      // 语法错误
            .map_err(StyleError::Parse)?;
        parsed.validate()                                   // 语义错误（$var 未定义等）
            .map_err(StyleError::Validate)?;
        *registry = ThemeRegistry::from(parsed);
        registry.epoch += 1;                                // 仅在完全成功后才 bump
        Ok(())
    }
}
// 调用侧：任何 Err → log + 保留现状，不修改 epoch
```

**关键**：`epoch += 1` **只在解析与校验完全成功后执行**。这样一次失败的编辑不会让 UI 退化——用户保存一个半成品文件时，界面保持上一个可用状态。

- **dev 模式**：输出错误位置（行/列）+ 原因 + 上下文片段
- **release**：记录 Error 日志，UI 静默保持现状

### 18.3 覆盖范围

| 改动类型 | 热重载 | 反馈路径 |
|---|---|---|
| 颜色、间距、字号、圆角、阴影参数、过渡时长、hover 态差值 | ✅ **200ms 内** | 样式文件 |
| 结构变更（加一个子元素、换排列） | ❌ | 重编译 |
| 新增控件类型 | ❌ | 重编译 |
| 布局逻辑改动 | ❌ | 重编译 |

### 18.4 辅助方案与不做的部分

| 手段 | 效果 |
|---|---|
| UI 代码独立为单独 crate | 只重编该 crate 及其下游 |
| `mold` / `lld` 链接器 | 链接时间大幅下降 |
| 增量编译调优 | 减少重编范围 |
| `sccache` | 跨构建缓存 |

> **M1 必须分别量测两个数字**：
>
> | 指标 | 目标 |
> |---|---|
> | 改样式文件 → 看到效果 | **≤ 200ms** |
> | 改一行 DSL 结构代码 → 看到效果 | **≤ 5s** |
>
> 后者超标则触发 13.7 的可回退条款（引入轻量 `view!` 宏）。

**明确不做 dylib 热重载**：跨 dylib 的类型稳定性、`'static` 数据边界、泛型实例化、信号 arena 的跨边界所有权——每一个都是深水区，且与本设计的核心（代际索引、Owner 树）直接冲突。

---

## 19. 快照测试

### 19.1 IR 快照：确定性规范

> **[决策] IR 快照的确定性由三条规定保证，在 M0 建立基线时验证。**

| 来源 | 规范 |
|---|---|
| **浮点** | 所有坐标/尺寸在 **P3 PAINT 生成 DrawOp 时**量化到 1/256 像素（见 17.3，**量化时机不可提前**） |
| **排序** | 所有集合遍历（节点顺序、绘制批次、字形 run）用**稳定排序 + 显式 tie-break**（按 `NodeId` 升序），不依赖 `HashMap` 迭代顺序 |
| **哈希** | 快照用的哈希走**确定性哈希器**（非 SipHash 随机化） |

> **[待验] M0 必须在三个平台（macOS / Windows / Linux）+ 两个优化级别（debug / release）下验证 IR 快照一致。** 若不一致，量化粒度还需收紧。

### 19.2 像素快照：分区容差

| 区域 | 阈值 | 理由 |
|---|---|---|
| 纯色块 / 几何图形 / 边框 | **99.9%** | 渲染确定性高，应严格 |
| 文字 / 抗锯齿边缘 | **99.0%** | 字体栅格化存在跨平台固有差异 |
| 渐变 / 模糊 | **99.5%** | 中间档 |

> **[决策] 区域类型的来源：从 DisplayList 提取元数据，而非图像分析。** 每个 DrawOp 天然知道自己画的是纯色矩形、路径、字形还是渐变——把 rect 列表与类型标签一起导出为 `.region` 文件，比对时按区域分别取阈值。这比边缘检测等启发式**准确且零成本**。

**跨平台处理**：像素快照在 CI 固定容器镜像中运行；基线更新走 `LieUI_SNAPSHOT_UPDATE=1` 的独立 workflow，需人工 review。工具用 `insta`。

---

## 20. 调试接口

```rust
#[cfg(debug_assertions)]
pub struct DebugTree<'a> { app: &'a App }
#[cfg(debug_assertions)]
impl<'a> DebugTree<'a> {
    pub fn find_by_key(&self, key: &str) -> Option<NodeId>;
    pub fn find_by_type(&self, t: ElementTypeId) -> Vec<NodeId>;
    pub fn prop_dump(&self, node: NodeId) -> PropertyDump;
    pub fn tree_dump(&self) -> TreeDump;
}
```

- **release 编译期消失**（`cfg` 门控，零运行时成本）
- 不提供字符串全局选择器；Rust 侧通过句柄直接访问，debug 期可遍历树

---

## 21. M2 前置决策：`leptos_reactive` 评估

> **[决策] M2 第一周用原型验证定案。整个借用模型（尤其"`get()` 必须 `&self`"）不依赖是否有脚本层。**

| # | 评估项 | 不通过则自研 |
|---|---|---|
| 1 | **Owner 语义**：能否表达 Window Owner / App Owner 两级分类 | 只能扁平 Owner |
| 2 | **嵌套依赖追踪**：观察者**栈**还是全局 `current_observer`？后者不支持嵌套 memo | 仅全局单例观察者 |
| 3 | **类型级约束**：能否让 `EffectCtx` 只写属性槽、禁止写信号 | API 暴露写信号能力 |
| 4 | **dispose 语义**：`try_read`、悬垂静默失效、dirty 队列联动清理 | dispose 后 panic 且无 try 变体 |
| 5 | **内部可变性**：**依赖追踪方法必须是 `&self`**（`ReadSignal::get(&self)` 内部完成订阅登记） | 需要 `&mut self` 才能读 |
| 6 | **每次重收集**：effect/memo 每次运行能否清空依赖集重新收集 | 依赖集只增不减 |
| 7 | **定制扩展**：容差比较、`write_if_changed`、`pending_initial` 队列能否接入 | 核心 API sealed |
| 8 | **依赖重量**：是否引入 web/wasm 相关依赖 | 拖入大量非桌面目标依赖 |

> **[硬标准] 8 项必须全部通过，不允许"部分满足"。**
>
> 理由：部分满足意味着要把 leptos 的内部语义适配到我们的模型——而第 5 项（`&self` 追踪）与第 6 项（每次重收集）是**整个借用模型与依赖精确性的地基**。适配这两项等于重写它的运行时，那时"用它的收益"只剩 arena 存储，而代价是长期受其演进约束。**自研反而更可控。**
>
> **决策时限**：M2 第一周结束前必须给出结论并冻结，不允许"再看看"。若结论是自研，M2 剩余时间按第 10 章的完整设计实施。

### 21.1 自研预案提前启动【v4.2 新增】

评审指出 M2 的时间压力：第一周评估 + 5 个极端用例，若结论是自研，剩余时间要完整实现信号系统。

> **[决策] M0 期间就启动自研预案的技术准备，不占用 M2 时间窗。**

| M0 并行任务 | 产出 |
|---|---|
| 通读 `leptos_reactive` 源码（`reactive_graph/` 模块） | 评估清单的 8 项在 M2 前就有**初步答案**，M2 第一周只做原型验证而非从零调研 |
| 按第 10 章设计搭一个**独立的最小信号原型**（单独 crate，不接入主工程） | 自研路径的可行性验证 + API 手感评估；约 300–500 行 |
| 对照两者，形成"采用 / 自研 / 混合"三方案对比 | M2 第一周直接决策，不做无谓探索 |

**这不是提前做 M2 的活，而是把调研成本从 M2 的时间窗移出去。** M0 的 IME 验证有等待期（需要接 winit 事件循环），这段时间是天然的并行窗口。

**验证原型的五个极端用例**：

1. **深层嵌套 Memo**：A → B → Signal C，修改 C 验证 A 更新与依赖图清理
2. **列表 Reconcile × Signal 并发**：`For` 中同时触发增删与内部信号更新，验证 dispose 不误杀正在执行的 effect
3. **悬垂句柄静默失效**：销毁含 1000 信号的组件，立即用旧 `SignalId` 调 `try_read`，验证返回 `None` 且无泄漏
4. **多窗口关闭清理**：两窗口共享 App 信号，快速关闭其一，验证另一窗口 effect 完好、被关闭窗口 effect 全清理、无泄漏
5. **`pending_initial` 去重**：构造"新 effect 创建后其依赖信号同帧被写入"的场景，断言本帧只运行一次

---

## 22. v4.1 → v4.2 修订对照

| # | v4.1 | v4.2 | 来源 |
|---|---|---|---|
| 1 | `value_epoch` 所有 raw 写都自增 | **仅可继承属性触发**（`PropKey::is_inheritable()` 编译期判定） | 评审A-2.1 |
| 2 | Drop 中读信号的约束未写 | **铁律：Drop 中禁用 `read_*`，只能用 `try_*`**（防 double panic → abort） | 评审B-2.2 |
| 3 | 受控组件写入方式未明确 | **13.6：受控组件必须 Signal 双向绑定，严禁 Presenter 直写** | 评审B-2.3 |
| 4 | 量化时机未精确 | **17.3：仅在 P3 生成 DrawOp 时量化，P0–P2 保持原生 f32**（防动画 jitter） | 评审B-2.4 |
| 5 | 动画自然结束的 ANIM 清理未定义 | **12.5：`clear_anim` 唯一入口**，由 `AnimationClock` 三路径调用 | 评审A-1 |
| 6 | `RAN_THIS_FRAME` 重置时机未写 | **11.3：帧首 ① 统一重置**，在冻结 dirty 快照之前 | 评审A-2 |
| 7 | 热重载解析失败未定义 | **18.2：失败保留旧主题 + 记录错误 + 等待下次变化；`epoch += 1` 仅在完全成功后** | 评审A-3 |
| 8 | Worker 积压无上限 | **14.1：单帧上限 N 条（默认 256），超限 dev 告警** | 评审A-5 |
| 9 | `flush_memo` 安全性靠 guard 规则 | **14.2：阶段约束**——effect 执行期间不存在用户回调路径，从架构上消除 | 评审A-6 |
| 10 | M2 时间压力未缓解 | **21.1：M0 并行启动自研预案**（读源码 + 最小原型 + 三方案对比） | 评审A-三 |

**评审 A-4（按层细分 `value_epoch`）已评估但明确不做**：父节点 LOCAL 写入导致子节点继承缓存多余失效的成本仅为一次节点查找，属过度设计。

---

## 23. 实施路线

### 23.1 MVP 控件集

| 里程碑 | 控件 |
|---|---|
| M0–M2 | Box / Text / Button / Input(单行) / Scroll / List(虚拟化) |
| M3 | Checkbox / Radio / Slider / Dropdown / Modal |
| M4 | 样式热重载 + 主题系统 + DSL 打磨 |
| M5 | TextArea(多行+IME) / Tab / Menu / Tree / Table / Tooltip |

**文本输入 + IME 是公认难点**，M0 就做技术验证（parley `PlainEditor` + winit IME 对接），不拖到 M5。

### 23.2 里程碑

| 里程碑 | 内容 | 验收标准 |
|---|---|---|
| **M0 骨架** | Tree + PropertyStore + 阶段化 pass + winit + vello_cpu 全屏重绘；`BackendCaps` + 降级；**IME 验证**；**raw/resolved + 继承存储定稿（含可继承过滤）**；快照基线流程；**并行：leptos 源码调研 + 最小信号原型** | 1 万矩形+文本帧时间达标；**IR 快照三平台两优化级别一致**；IME 组字可用；**自研预案三方案对比就绪** |
| **M1 布局** | taffy + parley 两段式、重排边界、文本布局缓存；**量编译时间** | 改单文本节点不触发整窗重排；**样式 ≤200ms / 结构 ≤5s 有数据** |
| **M2 信号响应式** | 内部可变性分层、观察者栈 + RAII、`guard 不跨边界`、**Drop 禁用 panic 读取**、每次重收集、两阶段铁律、拓扑序 memo、队列去重、Owner 分类、统一 effect 清理；**第一周 leptos 决策** | **五个极端用例**通过；销毁 10 万信号内存回落；**M2 结束前 2 周冻结信号公共 API** |
| **M3 DSL + Reconcile** | builder API、`Show`/`For`/`Match`、key 策略、`pending_initial` 拓扑序、`inherit_dirty`、虚拟化列表；**示例应用** | Reconcile × Signal 并发用例；memo 先于 pending_initial（断言）；首帧无闪烁；**示例应用暴露的人体工学痛点已归档；宏回退触发条件已检查** |
| **M4 样式与打磨** | **RON schema + `StyleWatcher`（含错误保留语义）**、主题系统、DSL 打磨、可选宏 | **改 RON → 200ms 内视觉更新**；**语法错误时 UI 保持上一个可用状态**；多窗口同步生效 |
| **M5 交互完善** | 文本编辑/IME 完整、焦点、命中测试、路由事件、accesskit、剪贴板、**多窗口**、**受控组件**、**动画 ANIM 清理** | 输入法组字正确；屏幕阅读器可读；**多窗口开关压力测试无泄漏**；受控组件无双写告警 |
| **M6 性能** | 剖析、图层缓存、脏矩形、质量档、渲染多线程 | 基准套件可复现；回归护栏进 CI |
| **M7 混合渲染** | 按量化判据（≥30%）接入 vello_hybrid | 双后端像素一致性通过；降级正确 |

**JS 前端降级为可选插件**，M7 之后按需求评估——架构已支持（命令流本就是边界），后置不返工。

### 23.3 关键顺序（五条）

1. **M2 信号 → M3 DSL/Reconcile → M4 样式**。
2. **M0 就建 `BackendCaps` 与 raw/resolved + 继承存储机制**，并验证 IR 快照跨平台确定性。
3. **M2 结束前 2 周冻结信号公共 API**。
4. **M2 第一周定 `leptos_reactive` vs 自研**，8 项硬标准，超时即自研；**M0 并行准备预案**。
5. **M1 量编译时间并据此决定 DSL 形态**（builder 还是轻量宏）。

---

## 附录 A：核心数据结构

```rust
// ── 标识：全部 Copy + 'static ──
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct NodeId(u64);       // idx|gen
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct SignalId(u32);
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct MemoId(u32);
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct EffectId(u32);
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct OwnerId(u32);
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct WindowId(u32);
#[derive(Copy, Clone)] pub struct ViewHandle { node: NodeId, app: AppId }
#[derive(Copy, Clone)] pub struct EntityId(u64);        // Presenter（MVP 兼容）

// ── 响应式：求值阶段 &self，结构阶段 &mut self ──
pub struct ReactiveGraph {
    signals: RefCell<SlotMap<SignalId, SignalSlot>>,
    memos:   RefCell<SlotMap<MemoId,   MemoSlot>>,
    subs:    RefCell<SecondaryMap<RxId, HashSet<EffectId>>>,
    deps:    RefCell<SecondaryMap<EffectId, HashSet<RxId>>>,
    observer_stack:  RefCell<Vec<RxId>>,
    computing:       RefCell<HashSet<MemoId>>,
    dirty:           RefCell<Vec<EffectId>>,
    pending_initial: RefCell<Vec<EffectId>>,
    dirty_memos:     RefCell<Vec<MemoId>>,
    owners:  SlotMap<OwnerId, OwnerNode>,           // 仅结构阶段 &mut
}
// !Sync + !Send：编译期强制 UI 线程亲和
impl ReactiveGraph {
    // ── 求值阶段：&self，guard 不跨调用边界 ──
    pub fn read_signal<T: Clone>(&self, id: SignalId) -> T;        // dispose 后 panic
    pub fn read_memo<T: Clone + PartialEq>(&self, id: MemoId) -> T;
    pub fn try_read_signal<T: Clone>(&self, id: SignalId) -> Option<T>;
    pub fn try_read_memo<T: Clone + PartialEq>(&self, id: MemoId) -> Option<T>;
    pub fn write_signal<T>(&self, id: SignalId, v: T);
    pub fn write_signal_if_changed<T: PartialEq>(&self, id: SignalId, v: T);
    pub fn write_signal_unchecked<T>(&self, id: SignalId, v: T);
    pub fn flush_memo<T: Clone + PartialEq>(&self, id: MemoId) -> T;  // 仅回调阶段可用
    // ── 结构阶段：&mut self ──
    pub fn create_signal<T>(&mut self, owner: OwnerId, v: T) -> SignalId;
    pub fn create_memo<T>(&mut self, owner: OwnerId, f: MemoFn<T>) -> MemoId;
    pub fn create_effect(&mut self, owner: OwnerId, node: NodeId,
                         key: PropKeyId, f: EffectFn) -> EffectId;
    fn remove_effect_raw(&mut self, e: EffectId);              // 三条路径共用
    pub fn dispose_owner(&mut self, id: OwnerId);
    pub fn remove_effect_preserving_owner(&mut self, e: EffectId);
    pub fn effects_with_window(&self, w: WindowId) -> Vec<EffectId>;
}

/// RAII 守卫：panic 时无条件清理 computing 与 observer_stack
struct ComputingGuard<'a> { graph: &'a ReactiveGraph, id: MemoId }
impl Drop for ComputingGuard<'_> { /* 无条件移除 */ }

// ── 队列去重：位标记 ──
bitflags! { struct EffectFlags: u8 {
    const IN_INITIAL = 1<<0; const IN_DIRTY = 1<<1; const RAN_THIS_FRAME = 1<<2;
}}
// 帧首 ① 统一重置 RAN_THIS_FRAME

// ── 单向数据流：类型级约束 ──
pub struct MemoCtx<'a>   { graph: &'a ReactiveGraph }                   // 无写信号
pub struct EffectCtx<'a> {
    graph:  &'a ReactiveGraph,
    window: &'a mut Window,        // 不是 &mut App：避开借用冲突
}
impl EffectCtx<'_> {
    pub fn set_prop<T>(&mut self, node: NodeId, k: PropKey<T>, v: T) -> bool; // 唯一写入口
    pub fn try_read<T: Clone>(&self, id: SignalId) -> Option<T>;
    pub fn read_or<T: Clone>(&self, id: SignalId, default: T) -> T;
}

pub struct EffectSlot { owner: OwnerId, node: NodeId, window: WindowId, key: PropKeyId }

// ── 多窗口 ──
pub struct App {
    rx: ReactiveGraph, theme: ThemeRegistry, text: TextService,
    images: ImageCache, style_watcher: StyleWatcher,
    windows: Vec<Window>, workers: WorkerPool,
}
impl App {
    fn split_borrows(&mut self) -> (&ReactiveGraph, &mut Vec<Window>) {
        (&self.rx, &mut self.windows)
    }
    pub fn create_shared_signal<T>(&mut self, v: T) -> SignalId;  // App Owner 下
    pub fn batch(&mut self, f: impl FnOnce(&mut BatchCtx));
    pub fn close_window(&mut self, wid: WindowId);
}
pub struct Window {
    tree: Tree, props: PropertyStore, states: StateStore,
    buffers: TextBuffers, layout: LayoutStore, dl: DisplayList,
    listeners: ListenerTable, damage: DamageTracker, focus: Option<NodeId>,
    root_owner: OwnerId, surface: Box<dyn Surface>,
}

// ── 属性：4 级 + 短路求值 + 可继承过滤 + 继承缓存 ──
bitflags! { pub struct ValueSource: u8 {
    const DEFAULT = 1<<0; const STYLE = 1<<1;
    const LOCAL   = 1<<2; const ANIM  = 1<<3;
}}
bitflags! { pub struct PropFlags: u8 {
    const INHERITABLE = 1<<0;                    // 【v4.2】编译期常量
}}
pub struct PropKey<T> { slot: u16, flags: PropFlags, _ty: PhantomData<T> }

pub struct InheritSlot {
    resolved: PropValue, epoch: u32,
    source: Option<NodeId>, src_gen: u32,    // 路径压缩
}
pub struct PropertyStore {
    f32s: Column<f32>, u32s: Column<u32>, colors: Column<Color>,
    strings: Column<SharedString>, any: Column<Box<dyn AnyVal>>,
    meta: MetaTable,                  // ValueSource + writer + value_epoch
    inherit: Column<InheritSlot>,
}
impl PropertyStore {
    fn set_raw<T>(&mut self, node: NodeId, key: PropKey<T>, src: ValueSource, v: T) {
        write_slot(node, key, src, v);
        if key.flags.contains(PropFlags::INHERITABLE) {   // 【v4.2】仅可继承才传播
            self.value_epoch[node] += 1;
            self.mark_subtree_inherit_dirty(node);
        }
    }
}
pub enum WriterKind { None, Effect(EffectId), Manual }   // dev 双写检测

// ── 动画清理 ──
impl AnimationClock {
    fn set_anim_raw<T>(&mut self, ..);            // 私有：ANIM slot 唯一写入者
    pub fn finish(&mut self, anim: AnimId);       // → clear_anim
    pub fn cancel(&mut self, anim: AnimId);       // → clear_anim
    fn interrupt(&mut self, anim: AnimId);        // → clear_anim
}

// ── 主题与热重载 ──
pub struct ThemeRegistry { scopes: HashMap<ScopeId, ThemeScope>, epoch: u32 }
pub struct StyleWatcher { path: PathBuf, last_mtime: SystemTime }
impl StyleWatcher {
    /// 仅在解析与校验完全成功后才 bump epoch
    fn try_reload(&mut self, reg: &mut ThemeRegistry) -> Result<(), StyleError>;
}

// ── 确定性 IR：量化仅在 P3 ──
impl DisplayList {
    pub fn push_draw_op(&mut self, op: DrawOp) {
        self.ops.push(op.quantized());       // 1/256 px，唯一量化点
    }
}
```

## 附录 B：DSL 速查

```rust
// ── 结构 ──
Column::new().padding(12).gap(8)
    .child(Text::new().text(&name).font_size(18))
    .child(Button::new().label("点我").on_click(move |cx| count.update(|n| *n + 1)))
    .when(show_footer, |c| c.child(Footer::new()))
    .push_some(opt_item, |c, it| c.child(Row::new(it)))

// ── 条件 / 列表 / 分支 ──
Show::when(&cond).child(|| Text::new().text(&label))
For::each(&items).key(|it| it.id).view(|it| Row::new(it))
Match::on(&state).arm(State::Idle, || Text::new("待机")).arm(State::Busy, || Spinner::new())

// ── 绑定 ──
Text::new().text(&signal)
Text::new().text_fn(move || format!("{} {}", a.get(), b.get()))   // 自动追踪
Text::new().text(&cx.memo(move || format!("{} {}", a.get(), b.get())))

// ── 受控组件（必须 Signal 双向绑定）──
Input::new()
    .value(&vm.text)                      // effect 写 LOCAL
    .on_input(move |s| vm.text.set(s))    // 输入写回 signal → effect 重写 UI

// ── Presenter（仅非数据驱动的纯命令式场景）──
let h: ViewHandle = cx.handle_of(container);
h.set_prop(PADDING, 24.0);
h.find("status_label").set_prop(TEXT, "已保存");

// ── 变更批次（事务性）──
app.batch(|cx| { cx.remove(row_id); cx.set_prop(other_id, VISIBLE, true); });

// ── Worker（长任务下放）──
cx.spawn(async move { let data = heavy_work().await; data })   // 回 UI 线程写信号
```

## 附录 C：编码规范（强制）

> 以下规范源自架构约束，违反会导致 panic、abort 或静默错误。写入项目 Wiki 并在 code review 中检查。

| # | 规范 | 违反后果 |
|---|---|---|
| 1 | **`Drop` 实现中禁用 `read_signal` / `read_memo`，只能用 `try_*`** | double panic → `abort`，无崩溃日志 |
| 2 | **effect 闭包内禁止创建 Signal/Memo/Effect** | 编译期拒绝（结构层 `&mut` 独占） |
| 3 | **effect 内禁止写信号**，只能写属性槽 | 类型系统阻止：`EffectCtx` 无写信号方法 |
| 4 | **effect 内只用 `EffectCtx::set_prop`**，不碰裸 `Window::set_prop` | 绕过存活检查 → 悬垂节点写入 |
| 5 | **effect 不得依赖继承属性**，只依赖显式 Signal/Memo | 依赖追踪不精确 → 漏更新 |
| 6 | **`RefCell` guard 不得跨调用边界**，同一函数内取得并释放 | 借用冲突 panic |
| 7 | **一个属性槽单一写入者**；受控组件必须 Signal 双向绑定 | dev 双写报错 |
| 8 | **ANIM 位只能通过 `AnimationClock` 清理** | 动画结束后 UI 卡在终值 |
| 9 | **浮点量化只在 P3 生成 DrawOp 时进行** | 动画期间 hairline / 曲线视觉抖动 |
| 10 | **样式解析失败时不得 bump epoch** | UI 因临时编辑错误退化到默认样式 |
| 11 | **跨窗口共享信号必须用 `App::create_shared_signal()`** | 窗口关闭导致另一窗口 effect 悬垂 |
| 12 | **effect 清理必须走 `remove_effect_raw`**，不得自行实现 | 依赖边泄漏 |

## 附录 D：依赖与升级

| 依赖 | 基线 | 关注点 |
|---|---|---|
| **vello_cpu** | 0.2.x | `render`/`render_with`、`Resources`、filter 支持度；**预期被重设计** |
| **vello_hybrid** | 0.1.x（M7） | Mask/filter/blend 限制；`Scene`/`Renderer` API |
| **parley** | 0.7.x | `editing`/`cursor` 模块路径、`Alignment` 变体名 |
| **leptos_reactive** | 待评估（M2 第一周） | **8 项硬标准，全过才采用**；M0 期间先做源码调研 |
| taffy / accesskit / winit | 稳定版 | 布局语义、无障碍模型、IME 接口 |
| RON / notify | 稳定版 | 样式文件 schema、文件监听（M4） |
| insta | 稳定版 | IR 快照（确定性验证）+ 像素快照 |

---

## 参考文献

[citation:1] Leptos — *Appendix: The Life Cycle of a Signal*（arena 分配、Copy arena、ownership tree）
https://book.leptos.dev/appendix_life_cycle.html

[citation:3] egui 文档 — *Why immediate mode*（immediate vs retained 取舍、布局悖论）
https://docs.rs/egui/0.36.0

[citation:4] Leptos 0.2.0-alpha — *Signals and scopes are 'static*
https://docsrs.com/crate/leptos/0.2.0-alpha

[citation:6] Sam Sartor — *Statefulness in GUIs*（retained/immediate/hybrid 所有权流转）
https://samsartor.com/guis-1/

[citation:7] Leptos DeepWiki — *Reactivity System*（dispose 后访问、集合信号泄漏、`ArcRwSignal`）
https://deepwiki.com/leptos-rs/book/3-reactivity-system

[citation:11] SolidJS — *Fine-grained reactivity*（组件只运行一次、无 VDOM、细粒度绑定）
https://docs.solidjs.com/advanced-concepts/fine-grained-reactivity

---

## 结语：架构冻结，验证在代码里

v4.2 是 LieUI 架构的最终版本。回顾六轮演进：

| 阶段 | 核心动作 |
|---|---|
| v1 → v2 | 尝试用 JS 做 UI 描述，引入跨界补偿机制 |
| **v2 → v3** | **删掉自找的复杂度**：响应式下沉 Rust 信号 |
| v3 → v3.1 | `expr()` 显式依赖、单向数据流、Reconcile 策略 |
| v3.1 → v3.2 | 内部可变性分层、首帧语义、多窗口路由 |
| v3.2 → v3.3 | 借用拆分、求值顺序、中断的不可恢复语义 |
| **v3.3 → v4.0** | **移除跨语言边界**：一整类问题失去存在理由 |
| v4.0 → v4.1 | 实现级缺陷修补（真 bug / 更优机制 / 硬标准） |
| **v4.1 → v4.2** | **实施级防坑指南 + 编码规范** |

贯穿全程的两条主线：

1. **状态外挂 + 阶段化**——用"任何时刻全局最多一个 `&mut`，且它从不与 `&Tree` 同属一个结构体"化解 Rust 借用检查。这条原则从 v3 一直管到 v4.2，衍生出内部可变性分层、`EffectCtx` 借用拆分、`guard 不跨边界`铁律。
2. **显式优于隐式**——`expr()` 显式依赖、单向数据流、单一写入者、`try_*` 显式容错。每次面对"自动化带来的便利"与"显式带来的确定性"，都选了后者。

**从 M0 起，架构文档的使命转变为"记录实现中发现的偏差"。** 不是继续评审，而是把代码与设计的出入补进对应章节，作为后续维护的上下文。真正的验证在代码里。

```
cargo new LieUI_workspace
```
