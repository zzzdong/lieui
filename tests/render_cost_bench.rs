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
