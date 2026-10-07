use super::*;
use crate::layout::layout;
use crate::track::{Kind, Layer, NodeId, Track};
use lieui_layout::FlexDirection;
use vello_cpu::color::PremulRgba8;

const W: f32 = 220.0;
const H: f32 = 120.0;
const BG: Color = Color::new(255, 255, 255);
const RED: Color = Color::new(255, 0, 0);
const BLUE: Color = Color::new(0, 80, 255);

fn px(r: &Renderer, x: u16, y: u16) -> PremulRgba8 {
    let pix = r.pixmap();
    pix.data()[usize::from(y) * usize::from(pix.width()) + usize::from(x)]
}

fn expect(p: PremulRgba8, c: Color) {
    let a = u16::from(c.a);
    let premul = |v: u8| ((u16::from(v) * a) / 255) as u8;
    assert_eq!(
        (p.r, p.g, p.b, p.a),
        (premul(c.r), premul(c.g), premul(c.b), c.a),
        "像素不匹配：{p:?}"
    );
}

/// 滚动容器（100 宽 × 全高，10 行 20 高的蓝条）+ 右侧一块红箱（与滚动内容
/// 的脏矩形无任何交集）。对应 gallery 的嵌套滚动区：一滚就产生 10+ 个
/// 碎片脏矩形，触发 `damage_batches` 的整窗回退。
fn scrolled_window() -> (Track, NodeId, NodeId) {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    {
        let n = t.get_mut(root).unwrap();
        n.layout.dim = [W, H];
        n.layout.flex_direction = FlexDirection::Row;
    }
    t.add_root(Layer::Content, None, root);

    let sc = t.create(Kind::Box, None);
    {
        let n = t.get_mut(sc).unwrap();
        n.layout.dim = [100.0, H];
        n.layout.flex_direction = FlexDirection::Column;
        n.layout.overflow_scroll = true;
        n.paint.background_color = Some(BG);
    }
    t.append_child(root, sc);
    for _ in 0..10 {
        let row = t.create(Kind::Box, None);
        let n = t.get_mut(row).unwrap();
        n.layout.dim = [80.0, 20.0];
        n.paint.background_color = Some(BLUE);
        t.append_child(sc, row);
    }

    let red = t.create(Kind::Box, None);
    {
        let n = t.get_mut(red).unwrap();
        n.layout.dim = [100.0, H];
        n.paint.background_color = Some(RED);
    }
    t.append_child(root, red);
    (t, sc, red)
}

/// 回归：滚动碎片脏矩形 > 8 ⇒ 光栅批次退化为整窗重画，此时场景若仍按
/// 脏区剔除，脏区外的元素（右侧红箱）会被擦成底色。修复后批次先算，
/// 退化整窗 ⇒ 场景也不剔除。
#[test]
fn full_window_fallback_does_not_erase_elements_outside_damage() {
    let (mut t, sc, _red) = scrolled_window();
    layout(&mut t, Size::new(W, H));
    let _ = t.take_damage(); // 清掉首布局的整窗脏

    let mut r = Renderer::new(Size::new(W, H), BG);
    r.render(&t, &[], true); // 首帧全量
    expect(px(&r, 150, 60), RED);
    expect(px(&r, 40, 10), BLUE);

    // 滚 20：10 行全部位移 ⇒ 20 个旧∪新脏矩形（> 8 ⇒ 整窗回退）
    assert!(t.set_scroll_offset(sc, (0.0, 20.0)));
    layout(&mut t, Size::new(W, H));
    let (damage, all) = t.take_damage();
    assert!(!all);
    assert!(damage.len() > 8, "应产生 >8 个碎片脏矩形：{}", damage.len());

    r.render(&t, &damage, all);
    // 滚动内容动了、脏区外的红箱还在
    expect(px(&r, 40, 10), BLUE);
    expect(px(&r, 40, 30), BLUE);
    expect(px(&r, 150, 60), RED);
}

/// 回归（D2）：`Track::destroy` 必须登记被删子树的**旧**绘制范围。
///
/// bug 表现：脏区是"旧像素 ∪ 新像素"。布局写回会登记旧∪新矩形，但**删除路径不登记**
/// —— 于是只要同帧存在**其它**脏矩形（脏区非空 ⇒ 渲染层不再按整窗兜底），
/// 被删节点所占的那块像素就没人重绘 ⇒ **残影**。
///
/// 测试因此必须同时制造一个"无关脏区"，否则整窗兜底会让这个 bug 测不出来。
#[test]
fn destroy_registers_damage_for_removed_subtree() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    {
        let n = t.get_mut(root).unwrap();
        n.layout.dim = [W, H];
        n.layout.flex_direction = FlexDirection::Row;
    }
    t.add_root(Layer::Content, None, root);

    // keep：留着的蓝箱，同时充当"同帧的其它脏区"来源
    let keep = t.create(Kind::Box, None);
    {
        let n = t.get_mut(keep).unwrap();
        n.layout.dim = [50.0, H];
        n.paint.background_color = Some(BLUE);
    }
    t.append_child(root, keep);

    // doomed：待删的红箱，占x 50..100
    let doomed = t.create(Kind::Box, None);
    {
        let n = t.get_mut(doomed).unwrap();
        n.layout.dim = [50.0, H];
        n.paint.background_color = Some(RED);
    }
    t.append_child(root, doomed);

    layout(&mut t, Size::new(W, H));
    let mut r = Renderer::new(Size::new(W, H), BG);
    r.render(&t, &[], true);
    expect(px(&r, 75, 60), RED); // 首帧：红箱在位
    let _ = t.take_damage();

    // 删掉红箱 + 给 keep 标脏（制造"同帧有其它脏区"这个 bug 触发条件）
    t.destroy(doomed);
    t.mark_paint_dirty(keep);
    layout(&mut t, Size::new(W, H));

    let (damage, all) = t.take_damage();
    assert!(!all, "这里应只有局部脏区");
    assert!(
        damage.iter().any(|d| d.x <= 50.0 && d.right() >= 100.0),
        "destroy 必须登记被删子树的旧矩形，实际脏区：{damage:?}"
    );

    r.render(&t, &damage, all);
    // 红箱没了 ⇒ 那块像素必须是底色。修复前这里是 RED（残影）。
    expect(px(&r, 75, 60), BG);
    // 蓝箱不受影响
    expect(px(&r, 25, 60), BLUE);
}

// ─────────────────── C3 / D3：图片参与裁剪栈 ───────────────────

use std::sync::Arc;

/// 构造一个纯色 RGBA8 图片
fn solid_image(w: u32, h: u32, c: [u8; 4]) -> Arc<crate::track::ImageData> {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..(w * h) {
        rgba.extend_from_slice(&c);
    }
    Arc::new(crate::track::ImageData {
        width: w,
        height: h,
        rgba,
    })
}

/// 回归（D3 / C3）：**图片必须参与裁剪栈**。
///
/// bug 表现：图片是手动 blit（`Op::Image` 在 `submit` 里被跳过），
/// 所以此前**完全不受 `PushClip` 约束** ⇒ 滚动容器 / 圆角裁剪里的图片
/// 会**溢出到裁剪区外**（源码里曾自述这个限制）。
///
/// 场景：一个 60×100 的容器（`overflow_scroll` ⇒ 渲染层自动裁剪），
/// 里面放一张 200×40 的图片（比容器宽）⇒ 图片右侧应被裁掉。
#[test]
fn image_is_clipped_by_its_container() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    {
        let n = t.get_mut(root).unwrap();
        n.layout.dim = [W, H];
        n.paint.background_color = Some(BG);
    }
    t.add_root(Layer::Content, None, root);

    // 60 宽 × 全高的滚动容器（渲染层据此产出 PushClip）
    let sc = t.create(Kind::Box, None);
    {
        let n = t.get_mut(sc).unwrap();
        n.layout.dim = [60.0, H];
        n.layout.flex_direction = FlexDirection::Column;
        n.layout.overflow_scroll = true;
        n.paint.background_color = Some(BG);
    }
    t.append_child(root, sc);

    // 一张 200×40 的图片（远宽于容器）
    let img = t.create(
        Kind::Image(Arc::new(crate::track::ImageData {
            width: 200,
            height: 40,
            rgba: {
                let mut v = Vec::with_capacity(200 * 40 * 4);
                for _ in 0..(200 * 40) {
                    v.extend_from_slice(&[255, 0, 0, 255]); // 红
                }
                v
            },
        })),
        None,
    );
    t.get_mut(img).unwrap().layout.dim = [200.0, 40.0];
    t.append_child(sc, img);

    layout(&mut t, Size::new(W, H));
    let mut r = Renderer::new(Size::new(W, H), BG);
    r.render(&t, &[], true);

    // 容器内（x < 60）⇒ 红色
    expect(px(&r, 30, 20), RED);
    // 容器外（x > 60）⇒ 必须是背景色，**不能**有图片溢出（D3）
    expect(px(&r, 90, 20), BG);
    expect(px(&r, 150, 20), BG);
}

/// 无裁剪时图片正常铺满（确认上一条不是"把图片整个干掉了"）。
#[test]
fn unclipped_image_still_renders() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    {
        let n = t.get_mut(root).unwrap();
        n.layout.dim = [W, H];
        n.paint.background_color = Some(BG);
    }
    t.add_root(Layer::Content, None, root);

    let img = t.create(Kind::Image(solid_image(200, 40, [255, 0, 0, 255])), None);
    t.get_mut(img).unwrap().layout.dim = [200.0, 40.0];
    t.append_child(root, img);

    layout(&mut t, Size::new(W, H));
    let mut r = Renderer::new(Size::new(W, H), BG);
    r.render(&t, &[], true);

    expect(px(&r, 30, 20), RED); // 容器内
    expect(px(&r, 150, 20), RED); // ★ 无裁剪 ⇒ 右侧也是图片（与上一条对照）
}
