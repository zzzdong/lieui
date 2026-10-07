use super::*;
use crate::cmd::apply_cmds;
use crate::event::PointerId;
use crate::layout::layout;
use crate::render::scene::{SceneBuilder, SceneOptions};
use crate::style::{PaintStyle, TextStyle};
use crate::track::{InteractionState, Layer};
use lieui_geom::Size;

fn opts() -> SceneOptions {
    SceneOptions {
        window: Size::new(200.0, 200.0),
        background: Color::new(255, 255, 255),
        focus_ring: false,
        theme: crate::theme::Theme::light(),
    }
}

fn node(t: &mut Track, kind: Kind, w: f32, h: f32) -> NodeId {
    let id = t.create(kind, None);
    t.get_mut(id).unwrap().layout.dim = [w, h];
    id
}

fn build(t: &Track) -> Scene {
    SceneBuilder::new().build(t, &opts(), &[], true)
}

fn green() -> Color {
    Color::new(0, 128, 0)
}

fn red() -> Color {
    Color::new(255, 0, 0)
}

#[test]
fn text_node_emits_a_text_op() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = node(&mut t, Kind::Text("hello".into()), 60.0, 20.0);
    t.get_mut(id).unwrap().text = TextStyle::new().font_size(16.0).color(green());
    t.append_child(root, id);
    layout(&mut t, Size::new(200.0, 200.0));

    let scene = build(&t);
    let text_ops: Vec<_> = scene.ops().iter().filter(|op| matches!(op, Op::Text { .. })).collect();
    assert_eq!(text_ops.len(), 1);
    match text_ops[0] {
        Op::Text { color, origin, .. } => {
            assert_eq!(*color, green());
            assert_eq!(origin.x, 0.0);
        }
        _ => unreachable!(),
    }
    assert_eq!(scene.stats.text_layouts_built, 1);
}

/// 光学对齐：节点矩形高 = 墨迹高，绘制原点需上移到墨迹上缘之外
/// （`origin.y = rect.y - ink.top`），字形墨迹才正好贴住矩形上缘。
#[test]
fn optical_align_puts_the_ink_top_at_the_rect_top() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    // 不设显式尺寸：高度必须来自测度（光学模式 = 墨迹高）
    let id = t.create(Kind::Text("abc".into()), None);
    {
        let n = t.get_mut(id).unwrap();
        n.text = TextStyle::new().font_size(13.0);
        n.text.spec.optical_align = true;
    }
    t.append_child(root, id);
    layout(&mut t, Size::new(200.0, 200.0));

    let rect = crate::layout::rect_of(&t, id);
    let ink = lieui_text::TextEngine::ink_bounds("abc", &t.get(id).unwrap().text.spec).expect("abc 有墨迹");

    let scene = build(&t);
    let Op::Text { origin, .. } = scene.ops().iter().find(|op| matches!(op, Op::Text { .. })).unwrap() else {
        unreachable!()
    };
    assert!(
        (rect.height - ink.height()).abs() < 0.01,
        "节点高度 = 墨迹高度：{} vs {}",
        rect.height,
        ink.height()
    );
    assert!(
        (origin.y - (rect.y - ink.top)).abs() < 0.01,
        "绘制原点上移到墨迹上缘：{} vs {}",
        origin.y,
        rect.y - ink.top
    );
}

/// 光学对齐 + padding：墨迹落在**内容盒**里（padding 不被吃掉）。
///
/// tooltip 就是这种盒子（文本 + 上下各 6px）；若贴 border box 上缘，视觉上会往上顶。
#[test]
fn optical_align_keeps_the_padding_around_the_ink() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = t.create(Kind::Text("tooltip".into()), None);
    {
        let n = t.get_mut(id).unwrap();
        n.text = TextStyle::new().font_size(12.0);
        n.text.spec.optical_align = true;
        n.text.spec.wrap = false;
        n.layout = n
            .layout
            .clone()
            .padding_top(6.0)
            .padding_bottom(6.0)
            .padding_left(10.0)
            .padding_right(10.0);
    }
    t.append_child(root, id);
    layout(&mut t, Size::new(200.0, 200.0));

    let rect = crate::layout::rect_of(&t, id);
    let ink = lieui_text::TextEngine::ink_bounds("tooltip", &t.get(id).unwrap().text.spec).expect("tooltip 有墨迹");
    assert!(
        (rect.height - (ink.height() + 12.0)).abs() < 0.01,
        "盒高 = 墨迹高 + 上下 padding：{} vs {}",
        rect.height,
        ink.height()
    );

    let scene = build(&t);
    let Op::Text { origin, .. } = scene.ops().iter().find(|op| matches!(op, Op::Text { .. })).unwrap() else {
        unreachable!()
    };
    let ink_top = origin.y + ink.top;
    let ink_left = origin.x;
    assert!(
        (ink_top - (rect.y + 6.0)).abs() < 0.01,
        "墨迹上缘落在内容盒上缘（rect.y + 6）：{ink_top} vs {}",
        rect.y + 6.0
    );
    assert!(
        (ink_left - (rect.x + 10.0)).abs() < 0.01,
        "墨迹左缘落在内容盒左缘（rect.x + 10）：{ink_left} vs {}",
        rect.x + 10.0
    );
    // 上下留白相等 ⇒ 视觉居中
    let bottom_gap = rect.bottom() - (ink_top + ink.height());
    assert!(
        ((ink_top - rect.y) - bottom_gap).abs() < 0.01,
        "上下 padding 对称：上 {} vs 下 {}",
        ink_top - rect.y,
        bottom_gap
    );
}

#[test]
fn empty_text_is_skipped() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = node(&mut t, Kind::Text(String::new()), 60.0, 20.0);
    t.append_child(root, id);
    layout(&mut t, Size::new(200.0, 200.0));

    let scene = build(&t);
    assert!(!scene.ops().iter().any(|op| matches!(op, Op::Text { .. })));
    assert_eq!(scene.stats.text_layouts_built, 0);
}

#[test]
fn button_label_is_centered() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = node(&mut t, Kind::Button { label: "OK".into() }, 80.0, 30.0);
    t.get_mut(id).unwrap().text = TextStyle::new().font_size(14.0);
    t.append_child(root, id);
    layout(&mut t, Size::new(200.0, 200.0));

    let scene = build(&t);
    let Op::Text { layout, origin, .. } = scene.ops().iter().find(|op| matches!(op, Op::Text { .. })).unwrap() else {
        unreachable!()
    };
    // 水平居中：左右留白相等（±1px 容差）
    let left = origin.x;
    let right = 80.0 - origin.x - layout.width();
    assert!((left - right).abs() < 1.0, "left={left} right={right}");
    // 垂直居中：上下留白相等（label 行盒在按钮内双轴居中）
    let top = origin.y;
    let bottom = 30.0 - origin.y - layout.height();
    assert!((top - bottom).abs() < 1.0, "top={top} bottom={bottom}");
}

/// icon_button 的字形行盒在按钮内**双轴居中**（gallery 图标行的对齐依据）。
#[test]
fn icon_button_glyph_is_centered_in_the_button() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = node(
        &mut t,
        Kind::Button {
            label: "\u{e5cd}".into(), // close
        },
        40.0,
        32.0,
    );
    t.get_mut(id).unwrap().text = TextStyle::new().font_size(20.0);
    t.append_child(root, id);
    layout(&mut t, Size::new(200.0, 200.0));

    let scene = build(&t);
    let Op::Text { layout, origin, .. } = scene.ops().iter().find(|op| matches!(op, Op::Text { .. })).unwrap() else {
        unreachable!()
    };
    // 行盒（20×20，图标字体行高系数 1.0）应居中于 40×32：上下各 6、左右各 10
    let top = origin.y;
    let bottom = 32.0 - origin.y - layout.height();
    let left = origin.x;
    let right = 40.0 - origin.x - layout.width();
    assert!((top - bottom).abs() < 1.0, "top={top} bottom={bottom}");
    assert!((left - right).abs() < 1.0, "left={left} right={right}");
}

#[test]
fn checkbox_draws_a_border_and_a_mark() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = node(&mut t, Kind::Checkbox { checked: false }, 20.0, 20.0);
    t.append_child(root, id);
    layout(&mut t, Size::new(200.0, 200.0));

    let scene = build(&t);
    assert!(scene.ops().iter().any(|op| matches!(op, Op::Border { .. })));
    assert!(!scene.ops().iter().any(|op| matches!(
        op,
        Op::Rect { radius, .. } if (*radius - 1.5).abs() < 1e-6
    )));

    t.get_mut(id).unwrap().kind = Kind::Checkbox { checked: true };
    let scene = build(&t);
    assert!(
        scene.ops().iter().any(|op| matches!(
            op,
            Op::Rect { radius, .. } if (*radius - 1.5).abs() < 1e-6
        )),
        "勾选后应画出标记"
    );
}

#[test]
fn slider_draws_track_and_knob_at_the_value_position() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = node(
        &mut t,
        Kind::Slider {
            value: 0.5,
            min: 0.0,
            max: 1.0,
            dragging: false,
        },
        100.0,
        20.0,
    );
    t.append_child(root, id);
    layout(&mut t, Size::new(200.0, 200.0));

    let scene = build(&t);
    // 轨道 + 已填充 + 滑块 = 3 个矩形（不含底色）
    let rects = scene
        .ops()
        .iter()
        .filter(|op| matches!(op, Op::Rect { rect, .. } if rect.width <= 100.0))
        .count();
    assert_eq!(rects, 3);

    let knob = scene
        .ops()
        .iter()
        .find_map(|op| match op {
            Op::Rect { rect, radius, .. } if (*radius - 6.0).abs() < 1e-6 => Some(*rect),
            _ => None,
        })
        .unwrap();
    assert!((knob.x - (100.0 - 12.0) * 0.5).abs() < 0.01, "滑块在中点：{knob:?}");
}

/// 回归：绘制曾把 value 硬归一化到 0..1，范围 0..10 时值 ≥1 就顶死最右
/// （gallery 音量滑块的症状）。0..10 范围取中点 ⇒ 滑块必须仍在中点。
#[test]
fn slider_respects_a_non_unit_range() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = node(
        &mut t,
        Kind::Slider {
            value: 5.0,
            min: 0.0,
            max: 10.0,
            dragging: false,
        },
        100.0,
        20.0,
    );
    t.append_child(root, id);
    layout(&mut t, Size::new(200.0, 200.0));

    let knob = build(&t)
        .ops()
        .iter()
        .find_map(|op| match op {
            Op::Rect { rect, radius, .. } if (*radius - 6.0).abs() < 1e-6 => Some(*rect),
            _ => None,
        })
        .unwrap();
    assert!(
        (knob.x - (100.0 - 12.0) * 0.5).abs() < 0.01,
        "值 5/10 ⇒ 滑块在中点：{knob:?}"
    );
}

#[test]
fn progress_fills_proportionally() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = node(&mut t, Kind::Progress { value: 0.25 }, 100.0, 8.0);
    t.get_mut(id).unwrap().paint = PaintStyle::new().background(green());
    t.append_child(root, id);
    layout(&mut t, Size::new(200.0, 200.0));

    let scene = build(&t);
    let filled = scene
        .ops()
        .iter()
        .find_map(|op| match op {
            // 底色是整条 100 宽，填充部分是 25 宽
            Op::Rect { rect, color, .. } if *color == green() && rect.width < 100.0 => Some(*rect),
            _ => None,
        })
        .unwrap();
    assert_eq!(filled.width, 25.0);
    assert_eq!(filled.height, 8.0);
}

#[test]
fn disabled_content_is_dimmed() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = node(&mut t, Kind::Box, 20.0, 20.0);
    t.get_mut(id).unwrap().paint = PaintStyle::new().background(red());
    t.get_mut(id).unwrap().interaction = InteractionState {
        enabled: false,
        ..InteractionState::default()
    };
    t.append_child(root, id);
    layout(&mut t, Size::new(200.0, 200.0));

    let scene = build(&t);
    let c = scene
        .ops()
        .iter()
        .find_map(|op| match op {
            Op::Rect { rect, color, .. } if rect.width == 20.0 => Some(*color),
            _ => None,
        })
        .unwrap();
    assert_eq!(c.a, 127, "禁用 ⇒ alpha 减半");
}

// ── 输入框绘制 ──

fn input_node(t: &mut Track, text: &str, placeholder: &str) -> NodeId {
    let id = t.create(
        Kind::Input {
            text: text.to_string(),
            placeholder: placeholder.to_string(),
            caret: text.len(),
            anchor: text.len(),
            preedit: String::new(),
            scroll: 0.0,
        },
        None,
    );
    {
        let n = t.get_mut(id).unwrap();
        n.layout = n
            .layout
            .clone()
            .width(200.0)
            .height(28.0)
            .padding_left(8.0)
            .padding_top(4.0);
        n.paint = PaintStyle::new().background(Color::WHITE);
    }
    id
}

fn text_ops(scene: &Scene) -> Vec<(Point, Color)> {
    scene
        .ops()
        .iter()
        .filter_map(|op| match op {
            Op::Text { origin, color, .. } => Some((*origin, *color)),
            _ => None,
        })
        .collect()
}

#[test]
fn input_draws_placeholder_when_empty() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = input_node(&mut t, "", "姓名");
    t.append_child(root, id);
    layout(&mut t, Size::new(300.0, 100.0));

    let scene = build(&t);
    let texts = text_ops(&scene);
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0].0.x, 8.0, "文本从左内边距开始");
    // 空文本 + 未聚焦 ⇒ 没有光标矩形
    assert!(!scene.ops().iter().any(|op| matches!(
        op,
        Op::Rect { rect, .. } if rect.width < 2.0 && rect.height > 10.0
    )));
}

#[test]
fn input_draws_the_text_and_a_caret_when_focused() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = input_node(&mut t, "hi", "");
    t.append_child(root, id);
    layout(&mut t, Size::new(300.0, 100.0));

    let scene = build(&t);
    let texts = text_ops(&scene);
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0].0.x, 8.0);

    // 聚焦 + 闪烁相位为亮 ⇒ 出现光标（1.5px 宽、比文本高）
    t.set_focused(id, crate::track::FocusState::Keyboard);
    t.blink_on = true;
    let scene = build(&t);
    let caret = scene.ops().iter().find_map(|op| match op {
        Op::Rect { rect, .. } if rect.width < 2.0 && rect.height > 10.0 => Some(*rect),
        _ => None,
    });
    let caret = caret.expect("有焦点 ⇒ 画光标");
    assert!(caret.x >= 8.0, "光标在左内边距之后：{caret:?}");
}

#[test]
fn input_draws_a_selection_rect() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = input_node(&mut t, "abcdef", "");
    t.append_child(root, id);
    layout(&mut t, Size::new(300.0, 100.0));

    // 选中前两个字符（caret=2, anchor=0）
    {
        let n = t.get_mut(id).unwrap();
        if let Kind::Input { caret, anchor, .. } = &mut n.kind {
            *caret = 2;
            *anchor = 0;
        }
        n.interaction.focused = true;
    }

    let scene = build(&t);
    let sel = scene.ops().iter().find_map(|op| match op {
        Op::Rect { rect, color, .. } if color.a > 0 && color.a < 200 && rect.width > 1.0 => Some(*rect),
        _ => None,
    });
    let sel = sel.expect("有选区 ⇒ 画高亮");
    assert!(sel.width > 2.0, "选区有宽度：{sel:?}");
    assert!(sel.x >= 8.0);
}

#[test]
fn input_draws_a_preedit_underline() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = input_node(&mut t, "", "");
    t.append_child(root, id);
    layout(&mut t, Size::new(300.0, 100.0));

    let before = build(&t).ops().len();
    t.input_set_preedit(id, "zhong".to_string());
    let scene = build(&t);
    assert!(scene.ops().len() > before, "组合串 ⇒ 文本 + 下划线");
}

// ── 滚动条 ──

const SCROLL_WIN: Size = Size::new(200.0, 100.0);

/// 滚动容器（200×100，竖直内容 300）+ 内容子节点；已布局
fn scroll_tree() -> (Track, NodeId, NodeId) {
    let mut t = Track::new();
    let sc = t.create(Kind::Box, None);
    {
        let n = t.get_mut(sc).unwrap();
        n.layout.dim = [200.0, 100.0];
        n.layout.overflow_scroll = true;
        n.layout.show_scrollbar = true;
    }
    t.add_root(Layer::Content, None, sc);
    let content = t.create(Kind::Box, None);
    t.get_mut(content).unwrap().layout.dim = [200.0, 300.0];
    t.append_child(sc, content);
    crate::layout::layout(&mut t, SCROLL_WIN);
    (t, sc, content)
}

fn pointer(kind: EventKind, pos: Point) -> Event {
    Event::pointer(kind, PointerId(0), pos, PointerButton::Left)
}

#[test]
fn thumb_geometry_is_proportional_with_a_minimum_length() {
    let (t, sc, _) = scroll_tree();
    let rect = t.get(sc).unwrap().rect();
    let content = t.get(sc).unwrap().content_size;
    assert!((content.height - 300.0).abs() < 0.5, "内容尺寸被布局写回：{content:?}");

    let (track_r, thumb) = vscroll_parts(rect, content, 0.0).expect("溢出 ⇒ 有竖直 thumb");
    assert!((track_r.x + track_r.width - rect.right()).abs() < 3.0, "贴右缘");
    assert!((thumb.height - (100.0 - 4.0) * 100.0 / 300.0).abs() < 0.5, "长度按比例");
    assert!((thumb.y - (rect.y + 2.0)).abs() < 0.5, "offset=0 ⇒ 顶端");

    // 没溢出 ⇒ 无 thumb
    let mut t2 = Track::new();
    let sc2 = t2.create(Kind::Box, None);
    {
        let n = t2.get_mut(sc2).unwrap();
        n.layout.dim = [200.0, 100.0];
        n.layout.overflow_scroll = true;
        n.layout.show_scrollbar = true;
    }
    t2.add_root(Layer::Content, None, sc2);
    let small = t2.create(Kind::Box, None);
    t2.get_mut(small).unwrap().layout.dim = [200.0, 50.0];
    t2.append_child(sc2, small);
    crate::layout::layout(&mut t2, SCROLL_WIN);
    assert!(vscroll_parts(t2.get(sc2).unwrap().rect(), t2.get(sc2).unwrap().content_size, 0.0).is_none());
}

#[test]
fn thumb_drag_maps_position_to_offset_and_releases() {
    let (mut t, sc, _) = scroll_tree();
    let rect = t.get(sc).unwrap().rect();
    let content = t.get(sc).unwrap().content_size;
    let (_, thumb) = vscroll_parts(rect, content, 0.0).unwrap();
    let grab = Point::new(thumb.center().x, thumb.y + 4.0); // 抓在 thumb 上部

    // 按下：建立拖拽会话 + 捕获
    let mut cmds = CmdBuf::new();
    handle(&mut t, sc, &pointer(EventKind::PointerPressed, grab), &mut cmds);
    apply_cmds(&mut t, cmds.as_slice());
    let drag = t.get(sc).unwrap().scroll_drag.expect("按下 thumb ⇒ 进入拖拽");
    assert!(drag.vertical);
    assert_eq!(t.captured_by(PointerId(0)), Some(sc), "拖拽期间捕获指针");

    // 拖到 track 中点 ⇒ offset ≈ max_scroll 的一半
    let travel = rect.height - 4.0 - thumb.height;
    let mid_y = rect.y + 2.0 + travel * 0.5;
    let mut cmds = CmdBuf::new();
    handle(
        &mut t,
        sc,
        &pointer(EventKind::PointerMoved, Point::new(grab.x, mid_y + 4.0)),
        &mut cmds,
    );
    apply_cmds(&mut t, cmds.as_slice());
    let oy = t.scroll_offset(sc).1;
    let max_scroll = 200.0;
    assert!((oy - max_scroll * 0.5).abs() < 1.5, "拖到中点 ⇒ offset≈一半：{oy}");

    // 松手：会话结束 + 捕获释放，offset 保留
    let mut cmds = CmdBuf::new();
    handle(
        &mut t,
        sc,
        &pointer(EventKind::PointerReleased, Point::new(grab.x, mid_y)),
        &mut cmds,
    );
    apply_cmds(&mut t, cmds.as_slice());
    assert!(t.get(sc).unwrap().scroll_drag.is_none());
    assert_eq!(t.captured_by(PointerId(0)), None);
    assert!(t.scroll_offset(sc).1 > 0.0, "松手后滚动位置保留");
}

#[test]
fn grabbing_the_thumb_top_does_not_jump_the_scroll() {
    let (mut t, sc, _) = scroll_tree();
    let rect = t.get(sc).unwrap().rect();
    let content = t.get(sc).unwrap().content_size;
    // 先滚到一半再抓 thumb 顶端
    t.set_scroll_offset(sc, (0.0, 100.0));
    let (_, thumb) = vscroll_parts(rect, content, 100.0).unwrap();

    let mut cmds = CmdBuf::new();
    handle(
        &mut t,
        sc,
        &pointer(EventKind::PointerPressed, Point::new(thumb.center().x, thumb.y + 0.5)),
        &mut cmds,
    );
    apply_cmds(&mut t, cmds.as_slice());
    let oy_before = t.scroll_offset(sc).1;

    // 只往下挪 1px ⇒ offset 只动一点点（≈ max_scroll / travel）
    handle(
        &mut t,
        sc,
        &pointer(EventKind::PointerMoved, Point::new(thumb.center().x, thumb.y + 1.5)),
        &mut cmds,
    );
    apply_cmds(&mut t, cmds.as_slice());
    let oy = t.scroll_offset(sc).1;
    let step = 200.0 / (rect.height - 4.0 - thumb.height);
    assert!(
        (oy - (oy_before + step)).abs() < 1.0,
        "抓取点不跳：{oy_before} → {oy}（一步应≈{step}）"
    );
}

#[test]
fn draw_emits_thumb_when_overflowing() {
    let (t, sc, _) = scroll_tree();
    let scene = build(&t);
    let rect = t.get(sc).unwrap().rect();
    let thumb_x_hit = scene.ops().iter().any(|op| match op {
        Op::Rect { rect: r, color, .. } => {
            *color == crate::theme::Theme::light().scrollbar_thumb
                && r.width <= SCROLLBAR_WIDTH + 0.5
                && r.x + r.width <= rect.right() + 0.5
        }
        _ => false,
    });
    assert!(thumb_x_hit, "溢出 ⇒ 场景里有 thumb 矩形");
}

/// **回归（真机报障：导出 PNG 后最小化窗口就崩）**：窗口最小化 ⇒ 客户区 0×0
/// ⇒ 布局把容器高度算成**负数** ⇒ 滚动条 thumb 的 `clamp(MIN_THUMB_LEN, track_len)`
/// 上下界反序（实测 `min = 24.0, max = -29.99`）⇒ `f32::clamp` panic。
///
/// 顺带钉住同源的另一个触发条件：容器**比最小 thumb（24px）还矮**时也会反序。
#[test]
fn scroll_parts_survives_collapsed_and_short_viewports() {
    let tall = Size::new(220.0, 4000.0);

    // ① 负高度（最小化后的真实情形）⇒ 没有轨道可画，而不是 panic
    let neg = Rect::new(0.0, 0.0, 220.0, -25.99);
    assert!(vscroll_parts(neg, tall, 0.0).is_none(), "负高度不画滚动条");

    // ② 零高度 ⇒ 同上
    let zero = Rect::new(0.0, 0.0, 220.0, 0.0);
    assert!(vscroll_parts(zero, tall, 0.0).is_none());

    // ③ 轨道比最小 thumb 还短：thumb 铺满轨道，长度合法（0 ≤ thumb ≤ track）
    let short = Rect::new(0.0, 0.0, 220.0, 10.0);
    let (track, thumb) = vscroll_parts(short, tall, 0.0).expect("矮容器仍给滚动条");
    assert!(track.height > 0.0, "轨道长度为正");
    assert!(thumb.height > 0.0, "thumb 长度为正");
    assert!(
        thumb.height <= track.height + 0.01,
        "thumb 不比轨道长：{} vs {}",
        thumb.height,
        track.height
    );

    // ④ NaN 尺寸（布局一旦出 NaN 就会流到这里）⇒ 不画，而不是画出 NaN 几何
    let nan = Rect::new(0.0, 0.0, 220.0, f32::NAN);
    assert!(vscroll_parts(nan, tall, 0.0).is_none());

    // ⑤ 正常情形不受影响：thumb 在轨道内、且不短于最小长度
    let normal = Rect::new(0.0, 0.0, 220.0, 800.0);
    let (track, thumb) = vscroll_parts(normal, tall, 0.0).expect("有滚动条");
    assert!(thumb.height >= MIN_THUMB_LEN - 0.01, "thumb 不小于最小长度");
    assert!(thumb.height < track.height, "内容溢出 ⇒ thumb 比轨道短");
}

/// 滚动条是**覆盖层**：必须画在子项（列表项）之后，否则会被盖住
/// （pdfkit 左侧页列表的症状）。
#[test]
fn scrollbar_is_drawn_above_the_content() {
    let (mut t, sc, _) = scroll_tree();
    // 给内容子节点一个铺满的底色（模拟列表项的整行背景）
    let content = t.children(sc)[0];
    t.get_mut(content).unwrap().paint.background_color = Some(Color::RED);

    let scene = build(&t);
    let thumb = scene.ops().iter().position(|op| {
        matches!(op, Op::Rect { rect, color, .. }
                if *color == crate::theme::Theme::light().scrollbar_thumb
                    && rect.width <= SCROLLBAR_WIDTH + 0.5)
    });
    let item = scene
        .ops()
        .iter()
        .position(|op| matches!(op, Op::Rect { color, .. } if *color == Color::RED));
    let (thumb, item) = (thumb.expect("有 thumb"), item.expect("有内容底色"));
    assert!(thumb > item, "滚动条（op #{thumb}）必须在内容（op #{item}）之后绘制");
}

// ── 开关 / 单选 ──

#[test]
fn switch_draws_pill_track_with_accent_when_on() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let on = node(&mut t, Kind::Switch { on: true }, 40.0, 20.0);
    let off = node(&mut t, Kind::Switch { on: false }, 40.0, 20.0);
    t.append_child(root, on);
    t.append_child(root, off);
    layout(&mut t, Size::new(200.0, 100.0));

    let light = crate::theme::Theme::light();
    let accent = light.accent;
    let scene = build(&t);
    let tracks: Vec<(Rect, Color)> = scene
        .ops()
        .iter()
        .filter_map(|op| match op {
            Op::Rect {
                rect, color, radius, ..
            } if *radius >= 9.0 => Some((*rect, *color)),
            _ => None,
        })
        .collect();
    assert_eq!(tracks.len(), 2, "两个药丸轨道：{tracks:?}");
    assert!(
        tracks.iter().any(|(r, c)| *c == accent && (r.width - 40.0).abs() < 0.5),
        "on ⇒ accent 轨道"
    );
    assert!(
        tracks
            .iter()
            .any(|(r, c)| *c == light.control_border && (r.width - 40.0).abs() < 0.5),
        "off ⇒ 边框色轨道"
    );
}

#[test]
fn radio_draws_a_dot_only_when_selected() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let sel = node(
        &mut t,
        Kind::Radio {
            selected: true,
            value: "a".into(),
        },
        18.0,
        18.0,
    );
    let unsel = node(
        &mut t,
        Kind::Radio {
            selected: false,
            value: "b".into(),
        },
        18.0,
        18.0,
    );
    t.append_child(root, sel);
    t.append_child(root, unsel);
    layout(&mut t, Size::new(200.0, 100.0));

    let accent = crate::theme::Theme::light().accent;
    let scene = build(&t);
    let dots = scene.ops().iter().filter(|op| {
        matches!(op,
            Op::Rect { color, .. } if *color == accent)
    });
    assert_eq!(dots.count(), 1, "只有选中的那项画圆点");
}

/// 焦点框颜色走主题 token（此前是硬编码的蓝）
#[test]
fn focus_ring_uses_the_theme_token() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = node(&mut t, Kind::Button { label: "ok".into() }, 60.0, 24.0);
    {
        let n = t.get_mut(id).unwrap();
        n.interaction.focused = true;
        n.focus_state = crate::track::FocusState::Keyboard;
    }
    t.append_child(root, id);
    layout(&mut t, Size::new(200.0, 200.0));

    let dark = crate::theme::Theme::dark();
    let mut o = opts();
    o.focus_ring = true;
    o.theme = dark;
    let scene = SceneBuilder::new().build(&t, &o, &[], true);
    let borders: Vec<Color> = scene
        .ops()
        .iter()
        .filter_map(|op| match op {
            Op::Border { color, .. } => Some(*color),
            _ => None,
        })
        .collect();
    assert!(borders.contains(&dark.focus_ring), "焦点框用主题 token：{borders:?}");
}

#[test]
fn shadow_is_offset_and_expanded() {
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let id = node(&mut t, Kind::Box, 40.0, 40.0);
    t.get_mut(id).unwrap().paint = PaintStyle::new().shadow(crate::style::ShadowSpec::new(
        4.0,
        6.0,
        8.0,
        2.0,
        Color::rgba(0, 0, 0, 60),
    ));
    t.append_child(root, id);
    layout(&mut t, Size::new(200.0, 200.0));

    let scene = build(&t);
    let Op::Shadow {
        rect, std_dev, color, ..
    } = scene.ops().iter().find(|op| matches!(op, Op::Shadow { .. })).unwrap()
    else {
        unreachable!()
    };
    assert_eq!(*rect, Rect::new(4.0 - 2.0, 6.0 - 2.0, 44.0, 44.0));
    assert_eq!(*std_dev, 4.0);
    assert_eq!(color.a, 60);
}
