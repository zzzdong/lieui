// 渲染一帧的成本分解：**不是回归测试**（计时断言跨机器不可比）。
//
// 跑法：`cargo test --release --test render_cost_bench -- --nocapture --ignored`
//
// ## 为什么要有这个实验
//
// 在决定是否做 C4（Scene 增量缓存 / display list）之前，需要先知道
// **Scene 重建到底是瓶颈，还是被高估的开销**。
// 之前 D-a（FlexNode 持久化）就是因为我高估了"几百次堆分配"而被推迟 ——
// 同类判断不能再犯第二次，所以这次先量。
//
// ## 实验设计
//
// 固定窗口像素数，变化节点数 N，在 `damage_all`（全窗口重绘）下测每帧时间：
//
//     time(N) = a · N  +  b · Pixels
//                 ↑           ↑
//        每节点成本(Scene)   每像素成本(光栅)
//
// 光栅成本与 N 无关（N 只影响 Scene 遍历），所以两个规模的差值就能解出 `a`。
// 若 `a` 很小，说明 C4 优化的是**次要项**，不值得做。

use std::time::Instant;

use lieui::layout::layout;
use lieui::prelude::Color;
use lieui::render::Renderer;
use lieui::track::{Kind, Layer, Track};
use lieui_geom::Size;

const WINDOW: Size = Size::new(1280.0, 800.0);
const FRAMES: usize = 40;

// ── 长列表场景：裁剪栈 culling 的实际收益 ──
//
// 这是本次修复的**目标场景**：500 行 × 20px = 10000px 高的列表装在 800px 高的窗口里。
// 修复前 500 行的原语**全部**被提交（nodes_culled ≈ 0），因为容器 bbox 必然与脏区相交。

fn long_list(n: usize) -> Track {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.get_mut(root).unwrap().layout.dim = [WINDOW.width, WINDOW.height];
    t.add_root(Layer::Content, None, root);
    let list = t.create(Kind::Box, None);
    {
        let node = t.get_mut(list).unwrap();
        node.layout.dim = [WINDOW.width, WINDOW.height];
        node.paint.clip_content = true;
        node.layout.overflow_scroll = true;
    }
    t.append_child(root, list);
    for i in 0..n {
        let row = t.create(Kind::Box, None);
        {
            let node = t.get_mut(row).unwrap();
            node.layout.dim = [200.0, 20.0];
            node.paint.background_color = Some(Color::rgba((i % 7) as u8, 90, 160, 255));
        }
        t.append_child(list, row);
        if i % 4 == 0 {
            let txt = t.create(Kind::Text(format!("行 {i}")), None);
            t.append_child(row, txt);
        }
    }
    layout(&mut t, WINDOW);
    t
}

#[test]
#[ignore = "性能测量，非回归测试；手动跑"]
fn long_list_culling_benefit() {
    use lieui::render::{SceneBuilder, SceneOptions};

    println!(
        "{:>8}  {:>12}  {:>12}  {:>14}  {:>10}",
        "行数", "culled", "visited", "ops", "ms/帧"
    );
    println!("{}", "-".repeat(64));

    for rows in [200usize, 1000, 5000] {
        let t = long_list(rows);

        let mut b = SceneBuilder::new();
        let opts = SceneOptions {
            window: WINDOW,
            ..Default::default()
        };
        let scene = b.build(&t, &opts, &[], true);
        let st = scene.stats;

        // 计时
        let mut r = Renderer::new(WINDOW, Color::rgba(250, 250, 250, 255));
        for _ in 0..3 {
            r.render(&t, &[], true);
        }
        let t0 = Instant::now();
        for _ in 0..FRAMES {
            r.render(&t, &[], true);
        }
        let ms = t0.elapsed().as_secs_f64() * 1e3 / FRAMES as f64;

        println!(
            "{:>8}  {:>12}  {:>12}  {:>14}  {:>10.3}",
            rows, st.nodes_culled, st.nodes_visited, st.ops, ms
        );
    }

    println!();
    println!("预期：culled ≈ 行数 − 可见行数（40行 = 800px / 20px）");
    println!("修复前：culled ≈ 0（容器 bbox 必然与脏区相交 ⇒ 全部子节点被遍历并提交）");
}

// ============================================================================
// P1 候选场景测量：另外两条"减少绘制量"的路径各能省多少？
//
// 上面的长列表场景已被上一批的裁剪栈 culling 解决（9x）。剩下两条候选：
//
//   候选 A —— **按可视区域裁剪子树**：`walk` 目前只在
//             `clips_children`（clip_content / overflow_scroll / 显式 clip）时
//             才做子树级跳过。**不裁剪**的容器即使有 thousands 个子节点、
//             且只有少数落在脏区内，仍会**全部遍历**。
//
//   候选 B —— **逐原语 culling 也用 clip**：`push_culled` 之类站点
//             （widgets/mod.rs 的 4 处）只判"与脏区相交"，
//             不判"是否被祖先 clip 裁掉"。节点**部分**可见时，
//             被裁掉那部分的原语仍会提交。
//
// **先量后改**：如果两者都省不到 5%，就不做。
// ============================================================================

/// 候选 A 的场景：**不裁剪**的大容器 + 大量子节点，只有少数落在小脏区内。
///
/// 与长列表的关键差异：`clip_content = false` ⇒ `clips_children` 为假
/// ⇒ `walk` 不会做子树级跳过。
fn unclipped_container(n: usize) -> Track {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.get_mut(root).unwrap().layout.dim = [WINDOW.width, WINDOW.height];
    t.add_root(Layer::Content, None, root);

    // ★ 不设 clip_content / overflow_scroll / clip
    let holder = t.create(Kind::Box, None);
    t.get_mut(holder).unwrap().layout.dim = [WINDOW.width, 40.0 * n as f32];
    t.append_child(root, holder);

    for i in 0..n {
        let row = t.create(Kind::Box, None);
        {
            let node = t.get_mut(row).unwrap();
            node.layout.dim = [WINDOW.width, 40.0];
            node.paint.background_color = Some(Color::rgba((i % 7) as u8, 90, 160, 255));
        }
        t.append_child(holder, row);
    }
    layout(&mut t, WINDOW);
    t
}

/// 候选 B 的场景：**部分可见**的节点 —— 容器裁剪到只露出 4px，
/// 但子节点本身是 200x20 的大矩形 ⇒ 绝大部分被clip 裁掉却仍提交原语。
fn partially_clipped_primitives(n: usize) -> Track {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.get_mut(root).unwrap().layout.dim = [WINDOW.width, WINDOW.height];
    t.add_root(Layer::Content, None, root);

    // 裁剪到只有 4px 高
    let clip = t.create(Kind::Box, None);
    {
        let node = t.get_mut(clip).unwrap();
        node.layout.dim = [WINDOW.width, 4.0];
        node.paint.clip_content = true;
    }
    t.append_child(root, clip);

    // 子节点远大于裁剪区 ⇒ 每个都"部分可见"或完全不可见。
    //
    // ★ `flex_shrink = 0` 是必需的：容器只有 4px 高，800 个 200px 的子节点
    //   若允许收缩，flex 会把它们**压成 0.8px 并全部堆进 y ∈ [0,4]** ⇒
    //   全部可见，`ops` 恒等于 n，**测不到任何东西**。
    //   （实测确认：无 `flex_shrink = 0` 时 child[i].height = 0.800。）
    //
    //   这与 `build()` 对滚动容器子节点做的处理是同一件事。
    for _ in 0..n {
        let row = t.create(Kind::Box, None);
        {
            let node = t.get_mut(row).unwrap();
            node.layout.dim = [WINDOW.width, 200.0];
            node.layout.flex_shrink = 0.0;
            node.paint.background_color = Some(Color::rgba(30, 144, 255, 255));
        }
        t.append_child(clip, row);
    }
    layout(&mut t, WINDOW);
    t
}

#[test]
#[ignore = "性能测量，非回归测试；手动跑"]
fn p1_candidate_scenarios() {
    use lieui::render::{SceneBuilder, SceneOptions};

    // 小脏区：左上角 64x64
    let dmg = [lieui_geom::Rect::new(0.0, 0.0, 64.0, 64.0)];
    let opts = SceneOptions {
        window: WINDOW,
        ..Default::default()
    };

    let measure = |name: &str, t: &Track, all: bool| {
        let mut b = SceneBuilder::new();
        let sc = b.build(t, &opts, &dmg, all);
        let st = sc.stats;

        let mut r = Renderer::new(WINDOW, Color::rgba(250, 250, 250, 255));
        for _ in 0..3 {
            r.render(t, &dmg, all);
        }
        let t0 = Instant::now();
        for _ in 0..FRAMES {
            r.render(t, &dmg, all);
        }
        let ms = t0.elapsed().as_secs_f64() * 1e3 / FRAMES as f64;
        println!(
            "{name:<34} visited={:<6} culled={:<6} ops={:<6} {ms:>8.3} ms/帧",
            st.nodes_visited, st.nodes_culled, st.ops
        );
    };

    println!("== 候选 A：不裁剪的大容器，子节点只有少数落在 64x64 脏区内 ==");
    for n in [200usize, 1000, 5000] {
        let t = unclipped_container(n);
        measure(&format!("unclipped_container({n})"), &t, false);
    }

    println!();
    println!("== 候选 B：部分可见节点（容器只露 4px，子节点 200x20）==");
    for n in [50usize, 200, 800] {
        let t = partially_clipped_primitives(n);
        measure(&format!("partially_clipped({n})"), &t, false);
    }

    println!();
    println!("读法：");
    println!("  · 候选 A：若 visited 随 n 线性增长而 ops 几乎不变 ⇒");
    println!("    说明\"不裁剪容器\"白白遍历了大量子树 ⇒ 候选 A 有价值。");
    println!("  · 候选 B：若 ops 接近 n ⇒ 大量原语被提交但被clip 裁掉");
    println!("    ⇒ 候选 B 有价值；若 ops 已被节点级culling 压到很低 ⇒ 收益有限。");
    println!("  · 两者的 ms/帧 都只�� 0.2ms ⇒ 都不值得做（参考长列表的 0.906ms）。");
}

/// 逐原语 clip 剔除的**真实适用场景**：原语 bounds 远大于节点 rect。
///
/// ## 为什么 `partially_clipped_primitives` 测不到它
///
/// 那个场景里 799 个子节点是被**节点级**裁剪挡掉的（`culled=799`）
/// ——它们的 `screen ∩ clip` 为空，`walk` 直接 `return`，**根本没走到逐原语**。
///
/// ## 这里测的是节点级**挡不住**的那一类
///
/// 节点自身与 clip **相交**（所以节点级放行），但它的**阴影原语**完全落在
/// clip 之外 —— 阴影的 bounds 远大于节点 rect（`paint_bounds` 会 inflate），
/// 这是 `track.rs` 里 `paint_bounds_covers_shadow_blur_reach` 钉住的行为。
///
/// 修��前：阴影原语照样提交（`cull.hit` 只判脏区）。
/// 修复后：`hit_visible` 判它与 clip 不相交 ⇒ 不提交。
fn shadow_outside_clip(n: usize) -> Track {
    use lieui::style::ShadowSpec;
    use lieui_layout::FlexDirection;

    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.get_mut(root).unwrap().layout.dim = [WINDOW.width, WINDOW.height];
    t.add_root(Layer::Content, None, root);

    // 裁剪到只有 4px 高
    let clip = t.create(Kind::Box, None);
    {
        let node = t.get_mut(clip).unwrap();
        node.layout.dim = [WINDOW.width, 4.0];
        node.paint.clip_content = true;
        // ★ **水平排列**：所有子节点的 y 都恰好落在 [0,4] ⇒ 全部与 clip 重合
        //   ⇒ 节点级裁剪**全部放行**（`nodes_culled` 应为 0）。
        //   若用 column 方向，800 个 4px 节点会超出容器，后面的 y > 4
        //   ⇒ 又被节点级挡住（实测 `culled=799`），根本测不到逐原语。
        node.layout.flex_direction = FlexDirection::Row;
        node.layout.flex_shrink = 0.0;
    }
    t.append_child(root, clip);

    for _ in 0..n {
        let row = t.create(Kind::Box, None);
        {
            let node = t.get_mut(row).unwrap();
            node.layout.dim = [10.0, 4.0];
            node.layout.flex_shrink = 0.0;
            // ★ 阴影**向下**偏移 30px + 模糊 12 ⇒ 阴影原语的 y ∈ [30, 58]，
            //   与 clip 的 y ∈ [0,4] **完全不相交** ⇒ 节点级挡不住，只有
            //   逐原语的 `hit_visible` 能挡。
            node.paint.shadow = Some(ShadowSpec::new(0.0, 30.0, 12.0, 0.0, Color::rgba(0, 0, 0, 128)));
            node.paint.background_color = Some(Color::rgba(30, 144, 255, 255));
        }
        t.append_child(clip, row);
    }
    layout(&mut t, WINDOW);
    t
}

#[test]
#[ignore = "性能测量，非回归测试；手动跑"]
fn p1_shadow_outside_clip() {
    use lieui::render::{SceneBuilder, SceneOptions};

    let dmg = [lieui_geom::Rect::new(0.0, 0.0, 64.0, 64.0)];
    let opts = SceneOptions {
        window: WINDOW,
        ..Default::default()
    };

    println!("== 节点与 clip 重合（节点级放行），但阴影原语在 clip 外 ==");
    for n in [50usize, 200, 800] {
        let t = shadow_outside_clip(n);
        let mut b = SceneBuilder::new();
        let st = b.build(&t, &opts, &dmg, false).stats;

        let mut r = Renderer::new(WINDOW, Color::rgba(250, 250, 250, 255));
        for _ in 0..3 {
            r.render(&t, &dmg, false);
        }
        let t0 = Instant::now();
        for _ in 0..FRAMES {
            r.render(&t, &dmg, false);
        }
        let ms = t0.elapsed().as_secs_f64() * 1e3 / FRAMES as f64;
        println!(
            "shadow_outside_clip({n:<4})         visited={:<6} culled={:<5} ops={:<6} {ms:>8.3} ms/帧",
            st.nodes_visited, st.nodes_culled, st.ops
        );
    }
    println!();
    println!("读法：culled 应为 0（节点与 clip 重合，节点级放行）");
    println!("      ⇒ 此时 ops 的差异**只能**来自逐原语 clip 剔除。");
}
