// 本文件由 `#[path]` 从 `scene.rs` 挂入，充当其 `mod tests`。
//
// 下面这些附带 mod 原本是 `scene.rs` 的顶层 mod。外移后它们的
// `use super::xxx` 指向本模块（而非 `scene`），而本模块顶层原有的
// `use super::*;`（来自 `mod tests`）正好把父模块的项引进来 ——
// 所以**无需额外导入**，多加一行反而触发 unused 警告。
//   附带的 mod: clip_culling, primitive_clip_culling

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
        .position(|op| {
            matches!(op, Op::Rect { color, .. }
                if *color == LayerOpts::for_layer(Layer::Modal).backdrop.unwrap())
        })
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

    let border_count = |scene: &Scene| scene.ops().iter().filter(|op| matches!(op, Op::Border { .. })).count();

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
    let cull = Cull::new(&[Rect::new(10.0, 10.0, 10.0, 10.0)], false, Size::new(100.0, 100.0));
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
    assert_eq!(
        kind_name(&Kind::Slider {
            value: 0.0,
            min: 0.0,
            max: 1.0,
            dragging: false
        }),
        "Slider"
    );
    let _ = (Flags::PAINT_DIRTY, Layer::Content);
}

/// **裁剪栈 culling** 的效果验收（降 overdraw）。
///
/// ## 它钉住的契约
///
/// 修复前 `Cull` 只携带窗口级脏区、**不携带祖先裁剪矩形**。于是滚动容器里
/// **滚出可视区**的行仍通过 `cull.hit` ⇒ 原语被提交 ⇒ 光栅器求交后**才丢弃**，
/// 开销已经发生。修复后 `walk` 递归传递 `parent_clip`（窗口坐标），
/// 节点 bbox 与祖先裁剪**无交集**时整棵子树直接退出。
///
/// ## 为什么必须用「计数」而不是「像素」验收
///
/// 被裁掉的像素**本来也不会显示**，所以**像素级断言对这次改动完全失明** ——
/// 改对改错屏幕上一模一样。唯一能区分的观测量是
/// `SceneStats::nodes_culled` / `ops`（**提交了多少**而非**画出了多少**）。
///
/// > 与本项目此前的教训同源：**验证手段必须能区分对错**，否则就是假测试。
mod clip_culling {
    use super::*;
    use crate::layout::layout;
    use crate::track::{Kind, Layer, NodeId, Track, Transform};

    const W: f32 = 300.0;
    const H: f32 = 200.0;
    const ROW_H: f32 = 20.0;

    fn opts() -> SceneOptions {
        SceneOptions {
            window: Size::new(W, H),
            ..Default::default()
        }
    }

    fn stats(t: &Track) -> SceneStats {
        let mut b = SceneBuilder::new();
        let scene = b.build(t, &opts(), &[], true);
        scene.stats
    }

    fn new_root(t: &mut Track) -> NodeId {
        let root = t.create(Kind::Box, None);
        t.get_mut(root).unwrap().layout.dim = [W, H];
        t.add_root(Layer::Content, None, root);
        root
    }

    /// `rows` 行 × `ROW_H`，装在 `H` 高的滚动容器里。
    fn long_list(rows: usize) -> Track {
        let mut t = Track::new();
        let root = new_root(&mut t);
        let list = t.create(Kind::Box, None);
        {
            let n = t.get_mut(list).unwrap();
            n.layout.dim = [W, H];
            n.paint.clip_content = true;
            n.layout.overflow_scroll = true;
        }
        t.append_child(root, list);
        for _ in 0..rows {
            let row = t.create(Kind::Box, None);
            {
                let n = t.get_mut(row).unwrap();
                n.layout.dim = [W, ROW_H];
                n.paint.background_color = Some(Color::rgba(200, 200, 255, 255));
            }
            t.append_child(list, row);
        }
        layout(&mut t, Size::new(W, H));
        t
    }

    /// ★ 核心：滚出可视区的行不再被遍历。
    ///
    /// 500 行 × 20px = 10000px 高，窗口只有 200px ⇒ 至少 490 行不可见。
    /// 修复前它们**全部**被遍历并提交原语（`nodes_culled` ≈ 0）。
    #[test]
    fn scrolled_out_rows_are_culled_by_ancestor_clip() {
        const ROWS: usize = 500;
        let t = long_list(ROWS);
        let s = stats(&t);
        let visible = (H / ROW_H) as usize; // 10 行

        assert!(
            s.nodes_culled >= ROWS - visible - 2,
            "滚出可视区的行应被祖先裁剪 cull 掉：期望 culled ≳ {}，\
                 实际 nodes_culled={} nodes_visited={} ops={}",
            ROWS - visible - 2,
            s.nodes_culled,
            s.nodes_visited,
            s.ops
        );
    }

    /// 配套方向：**可见的行不能被误cull**。
    ///
    /// 万一 cull 过头把可见内容也丢了，上一个测试照样会过 —— 方向必须成对钉。
    #[test]
    fn visible_rows_are_not_culled() {
        let t = long_list(10); // 10 × 20 = 200 = 恰好全部可见
        let s = stats(&t);
        assert_eq!(
            s.nodes_culled, 0,
            "全部可见时不应有任何 cull：visited={} ops={}",
            s.nodes_visited, s.ops
        );
        assert!(s.ops >= 10, "10 个可见行都应提交原语，实际 ops={}", s.ops);
    }

    /// ★ 一般化场景：内层容器超出外层裁剪区，其子节点也要被 cull
    /// （不只滚动容器 —— 任何 `clip_content` 都适用）。
    ///
    /// ★★ **构造陷阱（踩过）**：子节点必须显式 `flex_shrink = 0`。
    /// 否则 flex 会把它们**收缩到恰好装进外层裁剪区**（实测：100 个 `400×4`
    /// 被压成 `400×1`、容器被压成 `400×100`），于是**每一个都落在裁剪区内**，
    /// `culled = 0` 是**正确行为**而不是 bug。
    /// 写这类测试时必须先确认布局结果符合预期（`rect` 打印一遍即可）。
    #[test]
    fn nested_clip_culls_grandchildren_outside_outer_clip() {
        let mut t = Track::new();
        let root = new_root(&mut t);

        let outer = t.create(Kind::Box, None);
        {
            let n = t.get_mut(outer).unwrap();
            n.layout.dim = [100.0, 100.0];
            n.paint.clip_content = true;
        }
        t.append_child(root, outer);

        let inner = t.create(Kind::Box, None);
        {
            let n = t.get_mut(inner).unwrap();
            n.layout.dim = [400.0, 400.0];
            n.layout.flex_shrink = 0.0;
        }
        t.append_child(outer, inner);

        for _ in 0..100 {
            let child = t.create(Kind::Box, None);
            {
                let n = t.get_mut(child).unwrap();
                n.layout.dim = [400.0, 8.0];
                n.layout.flex_shrink = 0.0;
                n.paint.background_color = Some(Color::rgba(10, 10, 10, 255));
            }
            t.append_child(inner, child);
        }
        layout(&mut t, Size::new(W, H));
        let s = stats(&t);
        // 100 行 × 8px = 800px，裁剪区只有 100px 高 ⇒ 约 87 行不可见
        assert!(
            s.nodes_culled >= 70,
            "内层超出外层裁剪区的子节点应被 cull：实际 culled={} visited={} \
                 （若为 0，先确认子节点有没有被 flex 收缩进裁剪区）",
            s.nodes_culled,
            s.nodes_visited
        );
    }

    /// 显式 `n.clip` 同样参与（不只 `clip_content` / `overflow_scroll`）。
    #[test]
    fn explicit_clip_also_culls_children() {
        let mut t = Track::new();
        let root = new_root(&mut t);
        let holder = t.create(Kind::Box, None);
        {
            let n = t.get_mut(holder).unwrap();
            n.layout.dim = [W, H];
            n.clip = Some(Rect::new(0.0, 0.0, 50.0, 50.0));
        }
        t.append_child(root, holder);
        for _ in 0..50 {
            let child = t.create(Kind::Box, None);
            {
                let n = t.get_mut(child).unwrap();
                n.layout.dim = [W, 10.0]; // 横向全宽 ⇒ 大部分在 clip 外
                n.paint.background_color = Some(Color::rgba(0, 128, 0, 255));
            }
            t.append_child(holder, child);
        }
        layout(&mut t, Size::new(W, H));
        let s = stats(&t);
        // 阈值不写死：布局把 50 行排布在 300×200 里，落在 50×50 clip 内的行数
        // 取决于行高与起始位置。**只要"相当一部分"被cull 掉就说明修复生效**
        // （修复前恒为 0）—— 精确值对契约没有额外价值。
        assert!(
            s.nodes_culled >= 30,
            "显式 clip 外的子节点应被大量 cull（修复前恒为 0）：实际 culled={}",
            s.nodes_culled
        );
    }

    /// 变换下的裁剪仍正确：祖先裁剪在**窗口坐标**求交，而节点的 `screen` bbox
    /// 已经过 `transform.bounding_box` 映射。
    #[test]
    fn clip_culling_under_transform_is_consistent() {
        let mut t = Track::new();
        let root = new_root(&mut t);
        let holder = t.create(Kind::Box, None);
        {
            let n = t.get_mut(holder).unwrap();
            n.layout.dim = [100.0, 100.0];
            n.paint.clip_content = true;
        }
        t.append_child(root, holder);

        // 子节点被平移到裁剪区**之外**（x = 500 > 100）
        let kid = t.create(Kind::Box, None);
        {
            let n = t.get_mut(kid).unwrap();
            n.layout.dim = [50.0, 50.0];
            n.paint.background_color = Some(Color::rgba(255, 0, 0, 255));
            n.transform = Transform {
                translate: (500.0, 0.0),
                ..Default::default()
            };
        }
        t.append_child(holder, kid);
        layout(&mut t, Size::new(W, H));
        let s = stats(&t);
        assert!(
            s.nodes_culled >= 1,
            "被平移出裁剪区的节点应被 cull：实际 culled={}",
            s.nodes_culled
        );
    }
}

/// 逐原语 clip 剔除（P1 · 候选 B）。
///
/// ## 与上一批"节点级裁剪"的分工
///
/// 上一批按**节点 rect** 裁：`screen ∩ clip` 为空 ⇒ 整棵子树跳过。
/// 本批按**原语 bounds** 裁：节点与 clip 相交所以节点级放行，
/// 但它的某些原语（典型：**阴影**，bounds 被 `paint_bounds` inflate 到远大于 rect）
/// 完全落在 clip 外 ⇒ 不该提交。
mod primitive_clip_culling {
    use crate::layout::layout;
    use crate::style::ShadowSpec;
    use crate::track::{Kind, Layer, Track};
    use lieui_geom::{Rect, Size};
    use lieui_layout::FlexDirection;

    const WINDOW: Size = Size::new(300.0, 100.0);

    /// 裁剪区 + 一排**与 clip 完全重合**的节点，每个带一个向下溢出 30px 的阴影。
    ///
    /// 节点级裁剪对它们**全部放行**（`nodes_culled` 应为 0），
    /// 所以 ops 的差异只能来自逐原语剔除。
    fn shadow_rows(n: usize) -> Track {
        let mut t = Track::new();
        let root = t.create(Kind::Box, None);
        t.get_mut(root).unwrap().layout.dim = [WINDOW.width, WINDOW.height];
        t.add_root(Layer::Content, None, root);

        let clip = t.create(Kind::Box, None);
        {
            let node = t.get_mut(clip).unwrap();
            node.layout.dim = [WINDOW.width, 10.0];
            node.paint.clip_content = true;
            node.layout.flex_direction = FlexDirection::Row;
            node.layout.flex_shrink = 0.0;
        }
        t.append_child(root, clip);

        for _ in 0..n {
            let row = t.create(Kind::Box, None);
            {
                let node = t.get_mut(row).unwrap();
                // 10px 高 ⇒ 与 clip 完全重合 ⇒ 节点级放行。
                // ★ 宽 5px：40 个共 200px < 窗口 300px，**全部落在窗口内**。
                //   （第一版用 10px ⇒ 40×10 = 400 > 300，后 7 个被**窗口**裁掉，
                //    ops 只剩 33 —— 第三次栽在"构造没先验证"上。）
                node.layout.dim = [5.0, 10.0];
                node.layout.flex_shrink = 0.0;
                // 阴影向下溢出 30px + 模糊 12 ⇒ 完全在 clip（高 10）之外
                node.paint.shadow = Some(ShadowSpec::new(
                    0.0,
                    30.0,
                    12.0,
                    0.0,
                    lieui_geom::Color::rgba(0, 0, 0, 128),
                ));
                node.paint.background_color = Some(lieui_geom::Color::rgba(30, 144, 255, 255));
            }
            t.append_child(clip, row);
        }
        layout(&mut t, WINDOW);
        t
    }

    /// ★ 核心：节点与 clip 重合（节点级放行），但**阴影原语不得提交**。
    ///
    /// ★ 断言**只数 `Op::Shadow`**，而不是总 ops 数 ——
    ///   `ops()` 里还有 `PushClip` / `PopClip` / 容器背景等非阴影项，
    ///   按总数断言会把它们算进去（第一版写了 `ops <= 42` 就被这个绊倒：
    ///   实际 43 = 40 背景 + 1 容器背景 + PushClip + PopClip，**阴影其实是 0**）。
    #[test]
    fn shadow_primitive_fully_outside_clip_is_not_submitted() {
        let t = shadow_rows(40);
        let mut b = crate::render::scene::SceneBuilder::new();
        let sc = b.build(
            &t,
            &crate::render::SceneOptions {
                window: WINDOW,
                ..Default::default()
            },
            &[Rect::new(0.0, 0.0, WINDOW.width, WINDOW.height)],
            false,
        );
        let shadows = sc
            .ops()
            .iter()
            .filter(|op| matches!(op, crate::render::Op::Shadow { .. }))
            .count();
        assert_eq!(
            shadows, 0,
            "★ 40 个阴影完全在 clip 外（clip 高 10，阴影在 y∈[30,40]），不该提交"
        );
    }

    /// ★★ **反向**：与 clip 相交的原语**必须**提交。
    ///
    /// 只测上一条不够 —— 若`hit_visible` 永远返回 false，
    /// 第一个断言会过而这个会挂。两侧一起钉，方向才完整。
    #[test]
    fn primitives_inside_clip_are_still_submitted() {
        let t = shadow_rows(40);
        let mut b = crate::render::scene::SceneBuilder::new();
        let sc = b.build(
            &t,
            &crate::render::SceneOptions {
                window: WINDOW,
                ..Default::default()
            },
            &[Rect::new(0.0, 0.0, WINDOW.width, WINDOW.height)],
            false,
        );
        // 40 个节点各有背景矩形，全部与 clip 重合 ⇒ 必须全部提交
        assert!(
            sc.ops().len() >= 40,
            "★ 与 clip 相交的背景必须提交（ops={}）",
            sc.ops().len()
        );
    }

    /// ★★ 回归：**兄弟节点不得互相裁剪**。
    ///
    /// 这是本批实现中真实踩到的 bug：`walk` 返回前若没把裁剪栈恢复成
    /// "进入本节点时"的值，`sc` 返回后 `cull.clip` 仍停在 `sc` 的裁剪区，
    /// 于是**下一个兄弟** `red`（在 sc 之外）被整个裁掉 ⇒ 渲染成底色。
    ///
    /// 由既有的 `render::full_window_fallback_does_not_erase_elements_outside_damage`
    /// 抓到；本条把它钉在被测模块里，让因果关系一眼可见。
    #[test]
    fn siblings_do_not_inherit_each_others_clip() {
        let mut t = Track::new();
        let root = t.create(Kind::Box, None);
        t.get_mut(root).unwrap().layout.dim = [WINDOW.width, 100.0];
        t.get_mut(root).unwrap().layout.flex_direction = FlexDirection::Row;
        t.add_root(Layer::Content, None, root);

        // 兄弟 1：窄的裁剪容器（x ∈ [0, 40)）
        let sc = t.create(Kind::Box, None);
        {
            let node = t.get_mut(sc).unwrap();
            node.layout.dim = [40.0, 100.0];
            node.paint.clip_content = true;
            node.paint.background_color = Some(lieui_geom::Color::rgba(0, 0, 255, 255));
        }
        t.append_child(root, sc);

        // 兄弟 2：在 sc **之外**（x ∈ [40, 140)）—— 若继承了 sc 的裁剪就会被整块丢掉
        let red = t.create(Kind::Box, None);
        {
            let node = t.get_mut(red).unwrap();
            node.layout.dim = [100.0, 100.0];
            node.paint.background_color = Some(lieui_geom::Color::rgba(255, 0, 0, 255));
        }
        t.append_child(root, red);

        layout(&mut t, WINDOW);

        let mut b = crate::render::scene::SceneBuilder::new();
        let sc = b.build(
            &t,
            &crate::render::SceneOptions {
                window: WINDOW,
                ..Default::default()
            },
            &[Rect::new(0.0, 0.0, WINDOW.width, WINDOW.height)],
            false,
        );

        // 兄弟 2 的红色必须真的进了绘制列表
        let has_red = sc.ops().iter().any(|op| {
            matches!(op,
                    crate::render::Op::Rect { color, .. }
                        if color.r > 200 && color.g < 60 && color.b < 60)
        });
        assert!(has_red, "★ 兄弟节点不得继承上一个兄弟的裁剪（红色方块应仍被提交）");
    }
}
