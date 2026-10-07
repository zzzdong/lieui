use super::*;
use crate::render::scene::Scene;
use vello_cpu::color::PremulRgba8;

const W: f32 = 120.0;
const H: f32 = 80.0;
const BG: Color = Color::new(255, 255, 255);
const RED: Color = Color::new(255, 0, 0);
const BLUE: Color = Color::new(0, 0, 255);

fn size() -> Size {
    Size::new(W, H)
}

/// 用 op 列表直接拼一个场景（跳过保留树，专注光栅层）
fn scene(ops: Vec<Op>) -> Scene {
    let mut s = Scene::default();
    s.push(Op::Rect {
        rect: Rect::new(0.0, 0.0, W, H),
        radius: 0.0,
        color: BG,
        transform: Affine::IDENTITY,
    });
    for op in ops {
        s.push(op);
    }
    s
}

fn full(ops: Vec<Op>) -> Scene {
    scene(ops)
}

fn raster(ops: Vec<Op>) -> (Rasterizer, RasterStats) {
    let mut r = Rasterizer::new(size());
    let s = full(ops);
    let stats = r.rasterize(&s, &[], true);
    (r, stats)
}

// ── 三种批次策略的不变量（覆盖完备 + 行带互不重叠） ──

fn cases() -> Vec<Vec<Rect>> {
    vec![
        vec![Rect::new(10.0, 10.0, 20.0, 20.0)],
        vec![
            Rect::new(0.0, 0.0, 20.0, 20.0),
            Rect::new(100.0, 0.0, 20.0, 20.0),
            Rect::new(0.0, 60.0, 20.0, 20.0),
            Rect::new(100.0, 60.0, 20.0, 20.0),
        ],
        (0..6)
            .map(|i| Rect::new(20.0, 5.0 + i as f32 * 10.0, 30.0, 8.0))
            .collect(),
        vec![
            // 同带里左右两块（行带会合并成整行宽）
            Rect::new(5.0, 20.0, 20.0, 10.0),
            Rect::new(90.0, 25.0, 20.0, 10.0),
        ],
    ]
}

/// 像素级完备性：脏矩形里的**每个像素**都要落在某个批次里（否则漏画 ⇒ 残影）。
///
/// 注意不能要求"某个批次完整包含某个脏矩形"——行带会**按 y 切开**矩形，
/// 覆盖依然完备，只是分布在不同带里。
fn covers_every_pixel(batches: &[Rect], damage: &[Rect]) -> bool {
    for d in damage {
        let x0 = d.x.floor() as i32;
        let y0 = d.y.floor() as i32;
        let x1 = d.right().ceil() as i32;
        let y1 = d.bottom().ceil() as i32;
        for y in y0..y1 {
            for x in x0..x1 {
                let p = lieui_geom::Point::new(x as f32 + 0.5, y as f32 + 0.5);
                if !batches.iter().any(|b| b.contains(p)) {
                    return false;
                }
            }
        }
    }
    true
}

/// 每个策略的输出都必须**覆盖所有脏像素**
#[test]
fn every_strategy_covers_all_damage() {
    for damage in cases() {
        for (name, batches) in [
            ("exact", damage_batches(size(), &damage, false)),
            ("union", damage_batches_union(size(), &damage, false)),
            ("bands", damage_batches_bands(size(), &damage, false)),
        ] {
            assert!(!batches.is_empty(), "{name} 不该空");
            assert!(
                covers_every_pixel(&batches, &damage),
                "{name} 漏像素：{damage:?} -> {batches:?}"
            );
        }
    }
}

/// 行带的额外不变量：**互不重叠**（重叠会让同一像素被重复光栅化）
#[test]
fn bands_never_overlap() {
    for damage in cases() {
        let bands = damage_batches_bands(size(), &damage, false);
        for (i, a) in bands.iter().enumerate() {
            for b in bands.iter().skip(i + 1) {
                assert!(!a.intersects(b), "行带不应重叠：{a:?} 与 {b:?} 相交");
            }
        }
    }
}

/// 行带按 y 聚合：上下分离的两块 ⇒ 两个带，而**不是**整窗（对比单包围盒）
#[test]
fn bands_stay_local_where_union_covers_everything() {
    let damage = vec![Rect::new(10.0, 5.0, 20.0, 10.0), Rect::new(10.0, 65.0, 20.0, 10.0)];
    let bands = damage_batches_bands(size(), &damage, false);
    assert_eq!(bands.len(), 2, "上下两块 ⇒ 两个带：{bands:?}");
    let px: f32 = bands.iter().map(|r| r.width * r.height).sum();
    assert!(px < W * H * 0.2, "只画两块附近：{px}");

    let union = damage_batches_union(size(), &damage, false);
    let upx: f32 = union.iter().map(|r| r.width * r.height).sum();
    assert!(upx > px * 3.0, "单包围盒会吃掉整窗（{upx} vs {px}）");
}

/// 脏区为空 / 整窗脏：三种策略一致
#[test]
fn strategies_agree_on_the_trivial_cases() {
    assert!(damage_batches(size(), &[], false).is_empty());
    assert!(damage_batches_union(size(), &[], false).is_empty());
    assert!(damage_batches_bands(size(), &[], false).is_empty());

    let full = vec![Rect::new(0.0, 0.0, W, H)];
    for b in [
        damage_batches(size(), &full, true),
        damage_batches_union(size(), &full, true),
        damage_batches_bands(size(), &full, true),
    ] {
        assert_eq!(b, vec![Rect::new(0.0, 0.0, W, H)]);
    }
}

/// `batches_for` 与 `rasterize` 的批次决策一致（解耦后仍同源）
#[test]
fn batches_for_matches_the_rasterize_path() {
    let r = Rasterizer::with_scale(size(), 2.0);
    let damage = vec![Rect::new(10.0, 10.0, 20.0, 20.0)];
    let via_helper = r.batches_for(&damage, false);
    // 逻辑 → 物理（2×）后与直接调用纯函数的物理批次一致
    let scaled = vec![Rect::new(20.0, 20.0, 40.0, 40.0)];
    assert_eq!(via_helper, damage_batches(Size::new(W * 2.0, H * 2.0), &scaled, false));
}

fn px(r: &Rasterizer, x: u16, y: u16) -> PremulRgba8 {
    let pix = r.pixmap();
    pix.data()[usize::from(y) * usize::from(pix.width()) + usize::from(x)]
}

fn expect(p: PremulRgba8, c: Color) {
    // 直链 → 预乘（不透明时相同）
    let a = u16::from(c.a);
    let premul = |v: u8| ((u16::from(v) * a) / 255) as u8;
    assert_eq!(
        (p.r, p.g, p.b, p.a),
        (premul(c.r), premul(c.g), premul(c.b), c.a),
        "像素不匹配：{p:?}"
    );
}

#[test]
fn image_op_blits_pixels_contained() {
    // 2×2 四色图放进 4×4 的目标矩形（contain：正好铺满 4×4）
    let img = crate::track::ImageData {
        width: 2,
        height: 2,
        rgba: vec![
            255, 0, 0, 255, // 红
            0, 255, 0, 255, // 绿
            0, 0, 255, 255, // 蓝
            255, 255, 0, 255, // 黄
        ],
    };
    let (r, _) = raster(vec![Op::Image {
        image: std::sync::Arc::new(img),
        rect: Rect::new(2.0, 2.0, 4.0, 4.0),
        transform: Affine::IDENTITY,
    }]);

    // contain 缩放 2 倍：左上象限 = 红、右下象限 = 黄
    assert_eq!(px(&r, 3, 3), PremulRgba8::from_u8_array([255, 0, 0, 255]));
    assert_eq!(px(&r, 5, 5), PremulRgba8::from_u8_array([255, 255, 0, 255]));
    // 矩形外仍是背景
    assert_eq!(px(&r, 1, 1), PremulRgba8::from_u8_array([255, 255, 255, 255]));
    assert_eq!(px(&r, 7, 7), PremulRgba8::from_u8_array([255, 255, 255, 255]));
}

/// 图片不再"永远在最上层"：图片**之后**的原语必须能盖住它。
///
/// （症状来源：PDF 预览是图片、loading 遮罩是后画的原语，旧实现把图片统一放到
/// 批次末尾 blit ⇒ 遮罩被预览盖住，用户"看不到进度 modal"。）
///
/// **这条同时是 vello_cpu 0.3 的陷阱护栏**：0.3 的
/// `RasterizerSettings::default()` 带 `target_init: Clear(透明)`，直接用默认会把
/// 批次画布上已有内容（尤其是前面手工 blit 的图片）整片擦掉 —— 改成 `Clear` 跑这条，
/// 图片像素会变成全透明（已验证）。
#[test]
fn primitives_after_an_image_are_painted_above_it() {
    let img = crate::track::ImageData {
        width: 2,
        height: 2,
        rgba: vec![
            255, 0, 0, 255, 255, 0, 0, 255, // 全红
            255, 0, 0, 255, 255, 0, 0, 255,
        ],
    };
    let blue = Color::rgba(0, 0, 255, 255);
    let red = Color::rgba(255, 0, 0, 255);
    let (r, _) = raster(vec![
        // (10,10) 起 20×20 的红图
        Op::Image {
            image: std::sync::Arc::new(img),
            rect: Rect::new(10.0, 10.0, 20.0, 20.0),
            transform: Affine::IDENTITY,
        },
        // 图**之后**画一条蓝条，压在图上
        Op::Rect {
            rect: Rect::new(12.0, 12.0, 6.0, 6.0),
            radius: 0.0,
            color: blue,
            transform: Affine::IDENTITY,
        },
    ]);
    expect(px(&r, 14, 14), blue); // 后画的盖住图
    expect(px(&r, 25, 25), red); // 蓝条之外仍是图
    expect(px(&r, 2, 2), BG); // 图之外是底色
}

#[test]
fn background_is_filled() {
    let (r, stats) = raster(vec![]);
    assert!(stats.rasterized);
    assert_eq!(stats.batches, 1);
    assert_eq!(stats.pixels, u64::from(W as u16) * u64::from(H as u16));
    for &(x, y) in &[(0u16, 0u16), (60, 40), (119, 79)] {
        expect(px(&r, x, y), BG);
    }
}

#[test]
fn rect_is_painted_at_its_position_with_antialiased_edges() {
    let (r, _) = raster(vec![Op::Rect {
        rect: Rect::new(10.0, 10.0, 20.0, 20.0),
        radius: 0.0,
        color: RED,
        transform: Affine::IDENTITY,
    }]);

    expect(px(&r, 20, 20), RED);
    expect(px(&r, 5, 5), BG);
    expect(px(&r, 50, 50), BG);
}

#[test]
fn rounded_rect_leaves_the_corner_untouched() {
    let (r, _) = raster(vec![Op::Rect {
        rect: Rect::new(10.0, 10.0, 40.0, 40.0),
        radius: 10.0,
        color: RED,
        transform: Affine::IDENTITY,
    }]);
    expect(px(&r, 30, 30), RED);
    expect(px(&r, 11, 11), BG);
}

/// M3 的核心承诺：脏区只重画自己那几行，其余像素**原样保留**
#[test]
fn damage_band_keeps_the_rest_of_the_frame() {
    let mut r = Rasterizer::new(size());

    // 第一帧：画一块大红
    let s1 = full(vec![Op::Rect {
        rect: Rect::new(0.0, 0.0, W, H),
        radius: 0.0,
        color: RED,
        transform: Affine::IDENTITY,
    }]);
    r.rasterize(&s1, &[], true);
    expect(px(&r, 60, 40), RED);

    // 第二帧：只把 (10,10,20,20) 弄成蓝色，脏区只报告那一块
    let s2 = full(vec![
        Op::Rect {
            rect: Rect::new(0.0, 0.0, W, H),
            radius: 0.0,
            color: RED,
            transform: Affine::IDENTITY,
        },
        Op::Rect {
            rect: Rect::new(10.0, 10.0, 20.0, 20.0),
            radius: 0.0,
            color: BLUE,
            transform: Affine::IDENTITY,
        },
    ]);
    let stats = r.rasterize(&s2, &[Rect::new(10.0, 10.0, 20.0, 20.0)], false);

    assert_eq!(stats.batches, 1);
    assert_eq!(stats.pixels, 400, "只画 20×20");
    expect(px(&r, 20, 20), BLUE);
    expect(px(&r, 60, 40), RED);
}

#[test]
fn several_damage_rects_become_separate_batches() {
    let mut r = Rasterizer::new(size());
    let s = full(vec![]);
    let stats = r.rasterize(
        &s,
        &[Rect::new(0.0, 0.0, 10.0, 10.0), Rect::new(0.0, 60.0, 10.0, 10.0)],
        false,
    );
    assert_eq!(stats.batches, 2);
    assert_eq!(stats.pixels, 200);
}

#[test]
fn clip_crops_the_primitive() {
    let (r, _) = raster(vec![
        Op::PushClip {
            rect: Rect::new(0.0, 0.0, 40.0, 40.0),
            transform: Affine::IDENTITY,
        },
        Op::Rect {
            rect: Rect::new(0.0, 0.0, W, H),
            radius: 0.0,
            color: RED,
            transform: Affine::IDENTITY,
        },
        Op::PopClip,
    ]);
    expect(px(&r, 20, 20), RED);
    expect(px(&r, 80, 60), BG);
}

#[test]
fn opacity_layer_blends_with_the_background() {
    let (r, _) = raster(vec![
        Op::PushOpacity { opacity: 0.5 },
        Op::Rect {
            rect: Rect::new(0.0, 0.0, 40.0, 40.0),
            radius: 0.0,
            color: Color::new(0, 0, 0),
            transform: Affine::IDENTITY,
        },
        Op::PopOpacity,
    ]);
    let p = px(&r, 20, 20);
    // 半透明黑盖白底 ⇒ 中灰
    assert!((110..=145).contains(&p.r), "期望中灰，得到 {p:?}");
    assert_eq!(p.a, 255);
}

#[test]
fn transform_is_applied() {
    let (r, _) = raster(vec![Op::Rect {
        rect: Rect::new(0.0, 0.0, 20.0, 20.0),
        radius: 0.0,
        color: RED,
        transform: Affine::translate(50.0, 30.0),
    }]);
    expect(px(&r, 10, 10), BG);
    expect(px(&r, 60, 40), RED);
}

#[test]
fn text_paints_glyphs() {
    let spec = lieui_text::TextSpec {
        font_size: 24.0,
        ..Default::default()
    };
    let layout = std::sync::Arc::new(lieui_text::create_text_layout("Hg", &spec, Color::BLACK));
    assert!(layout.width() > 0.0, "字体可用");

    let (r, _) = raster(vec![Op::Text {
        layout,
        origin: lieui_geom::Point::new(4.0, 4.0),
        color: Color::BLACK,
        transform: Affine::IDENTITY,
    }]);

    // 文本区域里应出现非背景像素
    let mut painted = 0;
    for y in 0..28u16 {
        for x in 0..40u16 {
            let p = px(&r, x, y);
            if p.r < 200 {
                painted += 1;
            }
        }
    }
    assert!(painted > 5, "应画出字形，实际 {painted} 像素");
}

#[test]
fn shadow_paints_around_the_rect() {
    let (r, _) = raster(vec![Op::Shadow {
        rect: Rect::new(40.0, 30.0, 40.0, 20.0),
        radius: 4.0,
        std_dev: 4.0,
        color: Color::rgba(0, 0, 0, 120),
        transform: Affine::IDENTITY,
    }]);
    let p = px(&r, 60, 40);
    assert!(p.r < 240, "矩形处被阴影压暗：{p:?}");
    // 远处不受影响
    expect(px(&r, 5, 5), BG);
}

#[test]
fn resize_rebuilds_the_pixmap() {
    let mut r = Rasterizer::new(Size::new(10.0, 10.0));
    assert_eq!(r.size(), Size::new(10.0, 10.0));
    r.resize(Size::new(30.0, 20.0));
    assert_eq!(r.pixmap().width(), 30);
    assert_eq!(r.pixmap().height(), 20);
}

// ── 纯函数：行带计算 ──

#[test]
fn batches_dedupe_and_merge_adjacent() {
    let b = damage_batches(
        size(),
        &[
            Rect::new(10.0, 10.0, 5.0, 5.0),
            Rect::new(10.0, 10.0, 5.0, 5.0),     // 完全重复 ⇒ 丢弃
            Rect::new(-20.0, -20.0, 30.0, 30.0), // 裁到窗口内 ⇒ (0,0,10,10)
        ],
        false,
    );
    // ⚠️ C2 之后语义变了：**相邻/接触的矩形也会被合并**（原断言是"只去重完全相同的"）。
    // 这里裁剪后得到 (0,0,10,10) 与 (10,10,5,5) —— 它们正好**接触**于 (10,10)
    // ⇒ 合并成一块。这正是 C2 想要的（把碎片压到个位数）；
    // 旧断言记录的"2 块"在新行为下不再成立，故一并更新。
    assert_eq!(b.len(), 1, "接触的矩形应合并：{b:?}");
    assert_eq!(b[0], Rect::new(0.0, 0.0, 15.0, 15.0));
}

#[test]
fn batches_round_to_whole_pixels() {
    let b = damage_batches(size(), &[Rect::new(10.2, 10.2, 4.0, 5.1)], false);
    assert_eq!(b.len(), 1);
    assert_eq!(b[0], Rect::new(10.0, 10.0, 5.0, 6.0), "10.2..14.2 / 10.2..15.3");
}

#[test]
fn batches_fall_back_to_the_full_window_when_damage_is_large_or_fragmented() {
    assert_eq!(
        damage_batches(size(), &[Rect::new(0.0, 0.0, W, H * 0.5)], false),
        vec![Rect::new(0.0, 0.0, W, H)],
        "面积过半 ⇒ 整窗"
    );
    assert_eq!(damage_batches(size(), &[], true), vec![Rect::new(0.0, 0.0, W, H)]);

    // 碎片过多（> 8）⇒ 整窗（免去多次上下文重建）
    let many: Vec<Rect> = (0..9).map(|i| Rect::new(i as f32, i as f32, 1.0, 1.0)).collect();
    assert_eq!(damage_batches(size(), &many, false).len(), 1);
}

#[test]
fn no_damage_means_no_work() {
    assert!(damage_batches(size(), &[], false).is_empty());
    // 完全在窗口外的脏区被丢弃
    assert!(damage_batches(size(), &[Rect::new(-50.0, -50.0, 10.0, 10.0)], false).is_empty());

    let mut r = Rasterizer::new(size());
    let s = full(vec![]);
    let stats = r.rasterize(&s, &[], false);
    assert!(!stats.rasterized);
    assert_eq!(stats.batches, 0);
}

#[test]
fn narrow_damage_only_rasterizes_its_own_area() {
    let mut r = Rasterizer::new(size());
    let s = full(vec![]);
    // 一个 2×3 的小脏区：只应光栅化 6 个像素（不是整行宽度）
    let stats = r.rasterize(&s, &[Rect::new(100.0, 50.0, 2.0, 3.0)], false);
    assert_eq!(stats.pixels, 6);
}

#[test]
fn zero_sized_window_is_a_no_op() {
    let mut r = Rasterizer::new(Size::new(0.0, 0.0));
    let s = full(vec![]);
    let stats = r.rasterize(&s, &[], true);
    assert!(!stats.rasterized || stats.pixels > 0);
    // 不 panic 即可（内部按 1×1 兜底）
    assert!(r.pixmap().width() >= 1);
}

// ─────────────────── C2：脏区合并（D21） ───────────────────
#[test]
fn merge_rects_unions_overlapping_rects() {
    let a = Rect::new(0.0, 0.0, 100.0, 20.0);
    let b = Rect::new(90.0, 10.0, 100.0, 20.0); // 与 a 重叠
    let out = merge_rects(vec![a, b]);
    assert_eq!(out.len(), 1, "相交的矩形应合并成一块");
    assert_eq!(out[0], Rect::new(0.0, 0.0, 190.0, 30.0));
}

#[test]
fn merge_rects_unions_nearby_rects_within_gap() {
    // 间隙 2px ≤ MERGE_GAP ⇒ 合并（滚动碎片的旧位置/新位置就是这种关系）
    let a = Rect::new(0.0, 0.0, 100.0, 20.0);
    let b = Rect::new(0.0, 22.0, 100.0, 20.0);
    let out = merge_rects(vec![a, b]);
    assert_eq!(out.len(), 1, "间隙 ≤ GAP 的应合并");
    assert_eq!(out[0].height, 42.0);
}

#[test]
fn merge_rects_keeps_distant_rects_apart() {
    // 垂直间隔 200px ≫ GAP ⇒ 不合并（这是"分散小更新"必须保持的性质，
    // 否则全部并成包围盒就退化成 damage_batches_union 的问题）
    let a = Rect::new(0.0, 0.0, 50.0, 20.0);
    let b = Rect::new(0.0, 200.0, 50.0, 20.0);
    let out = merge_rects(vec![a, b]);
    assert_eq!(out.len(), 2, "远离的矩形不该被合并");
}

#[test]
fn merge_rects_never_shrinks_total_area() {
    // ★ 关键安全性质：合并只会让**重画面积变大或不变**（并集 ⊇ 各部分），
    //所以"少画"这个风险不存在 —— 绝不会漏画。
    let rects = vec![
        Rect::new(0.0, 0.0, 100.0, 20.0),
        Rect::new(90.0, 10.0, 100.0, 20.0),
        Rect::new(300.0, 300.0, 40.0, 40.0),
    ];
    let before: f64 = rects.iter().map(|r| f64::from(r.width * r.height)).sum();
    let after: f64 = merge_rects(rects.clone())
        .iter()
        .map(|r| f64::from(r.width * r.height))
        .sum();
    assert!(
        after >= before,
        "合并后面积必须 ⊇ 合并前（before={before}, after={after}）"
    );
}

#[test]
fn merge_rects_is_idempotent() {
    let rects = vec![
        Rect::new(0.0, 0.0, 100.0, 20.0),
        Rect::new(90.0, 10.0, 100.0, 20.0),
        Rect::new(0.0, 22.0, 100.0, 20.0),
    ];
    let once = merge_rects(rects.clone());
    let twice = merge_rects(once.clone());
    assert_eq!(once, twice, "合并应当幂等");
}

#[test]
fn merge_rects_transitively_merges_a_chain() {
    // a~b 相交、b~c 相交 ⇒ 三者应全部并成一块（贪心必须传递闭包）
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(8.0, 0.0, 10.0, 10.0);
    let c = Rect::new(16.0, 0.0, 10.0, 10.0);
    let out = merge_rects(vec![a, b, c]);
    assert_eq!(out.len(), 1, "链式相接应合并成一块");
    assert_eq!(out[0].width, 26.0);
}

#[test]
fn merge_rects_handles_empty_and_single() {
    assert!(merge_rects(Vec::new()).is_empty());
    let one = Rect::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(merge_rects(vec![one]), vec![one]);
}

/// C2 的判据：**面积**主导，而非碎片数。
///
/// 旧判据 `out.len() > 8` ⇒ **任何 ≥9 个分散更新都退化成整窗**，
/// 即使它们的总��积只有窗口的 1.6%（实测 10 个分散按钮 = 7546 / 921600像素）。
/// 那时局部分批反而比整窗便宜得多，退化是纯浪费。
#[test]
fn many_small_scattered_rects_stay_local() {
    let size = Size::new(1280.0, 720.0);
    // 20 个分散的小矩形（每个 40×20，垂直间隔 30px ⇒ 互不相邻）
    let rects: Vec<Rect> = (0..20)
        .map(|i| Rect::new(100.0, 10.0 + i as f32 * 30.0, 40.0, 20.0))
        .collect();
    let total_pixels: f64 = rects.iter().map(|r| f64::from(r.width * r.height)).sum();

    let batches = damage_batches(size, &rects, false);

    // 20 > 旧的 8 ⇒ 旧实现会整窗；现在应保持局部
    assert!(
        batches.len() > 1 || batches[0] != Rect::new(0.0, 0.0, size.width, size.height),
        "20 个分散小矩形不该退化成整窗"
    );
    let covered: f64 = batches.iter().map(|r| f64::from(r.width * r.height)).sum();
    assert!(
        covered < f64::from(size.width * size.height) * 0.2,
        "重画面积应远小于整窗（实际 {covered} / {}）",
        f64::from(size.width * size.height)
    );
    assert!(
        covered >= total_pixels,
        "覆盖面积必须 ⊇ 原始脏区（{covered} < {total_pixels}）"
    );
}
