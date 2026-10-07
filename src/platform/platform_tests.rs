use super::*;
use vello_cpu::color::PremulRgba8;

fn px(r: u8, g: u8, b: u8, a: u8) -> PremulRgba8 {
    PremulRgba8::from_u8_array([r, g, b, a])
}

#[test]
fn pack_xrgb_passes_through_opaque_pixels() {
    let src = [px(255, 128, 64, 255), px(0, 0, 0, 255)];
    let mut out = [0u32; 2];
    assert_eq!(pack_xrgb(&src, &mut out), 2);
    assert_eq!(out[0], 0x00FF_8040);
    assert_eq!(out[1], 0x0000_0000);
}

#[test]
fn pack_xrgb_unpremultiplies_and_ignores_alpha() {
    // 半透明白：premultiplied 之后是 (128,128,128,128) ⇒ 反预乘回 (255,255,255)
    let src = [px(128, 128, 128, 128)];
    let mut out = [0u32; 1];
    pack_xrgb(&src, &mut out);
    assert_eq!(out[0], 0x00FF_FFFF);

    // alpha = 0 ⇒ 视为背景（黑），不产生除零
    let src = [px(0, 0, 0, 0)];
    pack_xrgb(&src, &mut out);
    assert_eq!(out[0], 0);
}

#[test]
fn pack_xrgb_stops_at_the_shorter_side() {
    let src = [px(1, 2, 3, 255); 5];
    let mut out = [0u32; 2];
    assert_eq!(pack_xrgb(&src, &mut out), 2);
}

#[test]
fn softbuffer_damage_scales_and_clamps() {
    let physical = Size::new(200.0, 100.0);
    // 逻辑 (10,10,20,20) 在 2x 下 = 物理 (20,20,40,40)
    let d = softbuffer_damage(physical, &[Rect::new(10.0, 10.0, 20.0, 20.0)], false, 2.0);
    assert_eq!(d.len(), 1);
    assert_eq!(rect_of(&d[0]), (20, 20, 40, 40));

    // 越界被裁掉
    let d = softbuffer_damage(physical, &[Rect::new(500.0, 500.0, 10.0, 10.0)], false, 1.0);
    assert!(d.is_empty());

    // 整窗脏 ⇒ 一条覆盖全窗的矩形
    let d = softbuffer_damage(physical, &[], true, 1.0);
    assert_eq!(d.len(), 1);
    assert_eq!(rect_of(&d[0]), (0, 0, 200, 100));
}

#[test]
fn softbuffer_damage_uses_physical_size_at_fractional_scale() {
    let physical = Size::new(150.0, 75.0); // 逻辑 100×50 @1.5x
    let d = softbuffer_damage(physical, &[Rect::new(0.0, 0.0, 100.0, 50.0)], false, 1.5);
    assert_eq!(d.len(), 1);
    assert_eq!(rect_of(&d[0]), (0, 0, 150, 75));
}

fn rect_of(r: &softbuffer::Rect) -> (u32, u32, u32, u32) {
    (r.x, r.y, r.width.get(), r.height.get())
}

// ── DPI：逻辑 / 物理换算 ──

/// 光标：winit 给的是**物理**坐标 ⇒ 命中前必须除以 scale。
///
/// 这条测试是**防回归的钉子**：曾经有人以为 winit 给的是逻辑坐标，
/// 差点把这行除法当成"重复换算"删掉（那会让 2× 屏上的点击全部错位一半）。
#[test]
fn cursor_position_is_converted_from_physical_to_logical() {
    let p = PhysicalPosition::new(300.0, 150.0);
    assert_eq!(physical_to_logical(p, 1.0), Point::new(300.0, 150.0));
    assert_eq!(physical_to_logical(p, 2.0), Point::new(150.0, 75.0));
    assert_eq!(physical_to_logical(p, 1.5), Point::new(200.0, 100.0));
}

/// 坏的 scale（0 / NaN / 负）不能把布局污染成 NaN：统一按 1.0 处理
#[test]
fn insane_scale_falls_back_to_one() {
    let p = PhysicalPosition::new(10.0, 20.0);
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(sane_scale(bad), 1.0, "scale = {bad}");
        assert_eq!(physical_to_logical(p, bad), Point::new(10.0, 20.0));
    }
}

/// 窗口尺寸的双向换算自洽：`物理 → 逻辑 → 物理` 回到原值（非小数缩放时精确）
#[test]
fn physical_and_logical_sizes_round_trip() {
    let logical = Size::new(800.0, 600.0);
    for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
        let physical = logical_size_to_physical(logical, scale);
        assert_eq!(physical.width, (800.0 * scale).round() as u32, "scale {scale}");
        assert_eq!(physical.height, (600.0 * scale).round() as u32, "scale {scale}");
        // 1.25 / 2 / 3 能精确回推；1.5 的 600 → 900 也精确
        let back = physical_size_to_logical(physical, scale);
        assert!(
            (back.width - logical.width).abs() < 0.001 && (back.height - logical.height).abs() < 0.001,
            "scale {scale}: {back:?} != {logical:?}"
        );
    }
}

/// 尺寸至少 1×1：缩到 0 会让 softbuffer 的 `resize` 拿不到 `NonZeroU32`
#[test]
fn physical_size_never_collapses_to_zero() {
    let p = logical_size_to_physical(Size::new(0.0, 0.0), 2.0);
    assert_eq!((p.width, p.height), (1, 1));
}

// ─────────────────── C1 / D22：局部上屏只拷矩形内像素 ───────────────────

/// 构造 `w × h` 的 premul pixmap，像素值按 `(x, y)` 唯一编码，便于定位拷贝范围。
fn marker_pixmap(w: u32, h: u32) -> Vec<vello_cpu::color::PremulRgba8> {
    use vello_cpu::color::PremulRgba8;
    (0..(w * h) as usize)
        .map(|i| {
            let v = (i % 251 + 1) as u8;
            PremulRgba8 {
                r: v,
                g: v,
                b: v,
                a: 255,
            }
        })
        .collect()
}

/// ⚠️ `softbuffer::Length` 是 `NonZeroU32` ⇒ 长度 0 **无法表达**。
/// `softbuffer_damage` 里同样用 `.max(1)` 兜底（见其 `nz`），这里保持一致：
/// 传 0 会被夹成 1。
fn sb_rect(x: u32, y: u32, w: u32, h: u32) -> softbuffer::Rect {
    let nz = |v: u32| NonZeroU32::new(v.max(1)).unwrap_or(NonZeroU32::MIN);
    softbuffer::Rect {
        x,
        y,
        width: nz(w),
        height: nz(h),
    }
}

/// 回归（D22 / C1）：局部上屏的拷贝量必须 **∝ 脏区面积**，而不是"脏区高度 × 全窗宽"。
///
/// bug 表现：旧实现按 `row * stride .. +stride` 整行拷、忽略 `r.x` / `r.width`
/// ⇒ 40×20 的脏区实际拷 20 × 全窗宽像素。
#[test]
fn copy_damage_rects_only_touches_the_rect() {
    const W: u32 = 200;
    const H: u32 = 100;
    let src = marker_pixmap(W, H);
    let mut dst = vec![0u32; src.len()];
    let stride = W as usize;

    // 一个 40×20 的小脏区
    let rects = [sb_rect(10, 5, 40, 20)];
    let copied = copy_damage_rects(&src, &mut dst, stride, &rects);

    assert_eq!(copied, 40 * 20, "拷贝量应恰好等于脏区面积");
    // 脏区外的像素一个都不该被动
    let touched = dst.iter().filter(|d| **d != 0).count();
    assert_eq!(touched, 40 * 20, "脏区外不得被写入");
    // 脏区内每个像素都应被写入，且值与源一致（打包后 r 通道在高位）
    for row in 5..25usize {
        for col in 10..50usize {
            let i = row * stride + col;
            assert_ne!(dst[i], 0, "({row},{col}) 在脏区内却没被拷贝");
        }
    }
    // 同行但脏区外（x < 10 / x >= 50）不应被动
    assert_eq!(dst[5 * stride + 9], 0, "同行脏区左侧不应被动");
    assert_eq!(dst[5 * stride + 50], 0, "同行脏区右侧不应被动");
    // 相邻行也不应被动
    assert_eq!(dst[4 * stride + 10], 0, "脏区上方一行不应被动");
    assert_eq!(dst[25 * stride + 10], 0, "脏区下方一行不应被动");
}

/// 多个不相邻的脏区：拷贝量应是各面积之和（而不是并集/整窗）。
#[test]
fn copy_damage_rects_sums_multiple_rects() {
    const W: u32 = 64;
    const H: u32 = 64;
    let src = marker_pixmap(W, H);
    let mut dst = vec![0u32; src.len()];
    let stride = W as usize;

    let rects = [sb_rect(0, 0, 10, 10), sb_rect(50, 50, 8, 8)];
    let copied = copy_damage_rects(&src, &mut dst, stride, &rects);
    assert_eq!(copied, 100 + 64);
    assert_eq!(dst.iter().filter(|d| **d != 0).count(), 100 + 64);
}

/// 越界必须**静默裁剪**而不是 panic（窗口被最小化 / 脏区超出表面尺寸）。
#[test]
fn copy_damage_rects_clips_out_of_bounds() {
    const W: u32 = 32;
    const H: u32 = 32;
    let src = marker_pixmap(W, H);
    let stride = W as usize;
    let mut dst = vec![0u32; src.len()];

    // 右边界溢出
    let copied = copy_damage_rects(&src, &mut dst, stride, &[sb_rect(30, 0, 10, 2)]);
    assert_eq!(copied, 2 * 2, "只应拷贝界内那2 列");
    // 下边界溢出
    let mut dst2 = vec![0u32; src.len()];
    let copied2 = copy_damage_rects(&src, &mut dst2, stride, &[sb_rect(0, 31, 4, 10)]);
    assert_eq!(copied2, 4, "下边界溢出：只拷界内那1 行 × 4 列");
    // 完全在界外
    let mut dst3 = vec![0u32; src.len()];
    assert_eq!(
        copy_damage_rects(&src, &mut dst3, stride, &[sb_rect(100, 100, 5, 5)]),
        0
    );
    assert!(dst3.iter().all(|d| *d == 0));
    // 零尺寸：`Length` 是 NonZero ⇒ 宽度 0 被夹成 1 ⇒ 拷 1 像素（与 `softbuffer_damage` 的
    // `.max(1)` 兜底一致）。断言的是这个**已记录在案**的行为，不是"零尺寸不拷"。
    let mut dst4 = vec![0u32; src.len()];
    assert_eq!(copy_damage_rects(&src, &mut dst4, stride, &[sb_rect(0, 0, 0, 1)]), 1);
    // stride 为 0 不应除零 panic
    assert_eq!(copy_damage_rects(&src, &mut dst3, 0, &[sb_rect(0, 0, 5, 5)]), 0);
}

/// 回归对照：**旧行为**（整行拷）的拷贝量是多少 —— 量化 D22 到底浪费了多少。
/// 这条不是断言旧行为，而是把"整窗 × 高度"与"脏区面积"的倍数关系固定下来，
/// 让"修了之后省了多少"有据可查。
#[test]
fn old_behaviour_would_copy_the_whole_row() {
    const W: u32 = 200;
    const H: u32 = 100;
    let src = marker_pixmap(W, H);
    let stride = W as usize;
    let rect = sb_rect(10, 5, 40, 20);

    let new_copied = {
        let mut dst = vec![0u32; src.len()];
        copy_damage_rects(&src, &mut dst, stride, &[rect])
    };
    let old_copied = rect.height.get() as usize * stride; // 旧实现：整行 × 行数

    assert_eq!(new_copied, 800);
    assert_eq!(old_copied, 20 * 200);
    assert_eq!(
        old_copied / new_copied,
        5,
        "40×20 的脏区，旧实现多拷了 5 倍（= 整窗宽 / 脏区宽）"
    );
}
