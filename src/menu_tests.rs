use super::*;
use crate::layout::{layout, place_anchored_layers};
use crate::theme::Theme;
use crate::track::{Kind as TKind, Layer, NodeId, Track};
use lieui_geom::{Rect, Size};

const WIN: Size = Size::new(320.0, 240.0);

/// 造一个"内容根 + 锚在左上的弹层 + 菜单"，返回 (Track, 菜单列, 弹层根)
///
/// 主题固定成 [`Theme::dark`]：菜单的配色全来自主题 token，测试要断言的就是
/// "取的是哪个 token"，所以描述树与断言必须看同一份主题（`ViewBuf::new()` 自带浅色）。
fn menu_of(f: impl FnOnce(&mut MenuRef<'_>)) -> (Track, NodeId, NodeId) {
    let mut v = ViewBuf::new();
    v.set_theme(Theme::dark());
    v.begin();
    v.column(|c| {
        c.text("body");
    });
    v.popup_at_point(
        crate::geom::Point::new(20.0, 20.0),
        crate::track::Placement::Below,
        |p| {
            p.menu(f);
        },
    );
    let mut t = Track::new();
    crate::align::align(&mut t, &v);
    layout(&mut t, WIN);
    place_anchored_layers(&mut t, WIN);
    let popup = t
        .roots()
        .iter()
        .find(|r| r.layer == Layer::Popup)
        .expect("应存在 Popup 层")
        .node;
    let menu = t.get(popup).unwrap().children[0];
    (t, menu, popup)
}

/// 菜单列的直接子节点（行 / 分隔线）
fn rows(t: &Track, menu: NodeId) -> Vec<NodeId> {
    t.get(menu).unwrap().children.clone()
}

/// 任意节点的直接子节点
fn kids(t: &Track, node: NodeId) -> Vec<NodeId> {
    t.get(node).unwrap().children.clone()
}

fn label_of(t: &Track, node: NodeId) -> String {
    match &t.get(node).unwrap().kind {
        TKind::Text(s) => s.clone(),
        other => panic!("应是文本节点：{other:?}"),
    }
}

/// 三项 + 一条分隔线：结构必须是 [行, 行, 线, 行]
#[test]
fn menu_renders_one_row_per_item_plus_separators() {
    let (t, menu, _) = menu_of(|m| {
        m.item("打开");
        m.item("另存为");
        m.separator();
        m.item("退出");
    });
    let r = rows(&t, menu);
    assert_eq!(r.len(), 4, "3 项 + 1 分隔线");
    // 三项都是"行容器"（Box），分隔线也是 Box 但没有子节点
    for &id in [&r[0], &r[1], &r[3]] {
        assert!(matches!(t.get(id).unwrap().kind, TKind::Box), "项是容器行");
        assert!(!t.get(id).unwrap().children.is_empty(), "行里有子节点");
    }
    assert!(t.get(r[2]).unwrap().children.is_empty(), "分隔线是叶子（1px 线）");
    assert_eq!(label_of(&t, kids(&t, r[0])[0]), "打开");
    assert_eq!(label_of(&t, kids(&t, r[1])[0]), "另存为");
    assert_eq!(label_of(&t, kids(&t, r[3])[0]), "退出");
}

/// 分隔线 = 1px 细线 + 上下留白（不是空行占位）
#[test]
fn separator_is_a_thin_line_with_margins() {
    let (t, menu, _) = menu_of(|m| {
        m.item("a");
        m.separator();
        m.item("b");
    });
    let sep = rows(&t, menu)[1];
    let n = t.get(sep).unwrap();
    let l = &n.layout;
    assert_eq!(l.dim[1], SEPARATOR_H, "线高 1px");
    assert_eq!(l.margin[CSSDirection::Top as usize], SEPARATOR_PAD_Y, "上留白");
    assert_eq!(l.margin[CSSDirection::Bottom as usize], SEPARATOR_PAD_Y, "下留白");
}

/// 无勾选 / 无图标时**不留空白槽**（标签从行首开始）
#[test]
fn slots_appear_only_when_needed() {
    let (t, menu, _) = menu_of(|m| {
        m.item("普通");
        m.item("剪切");
    });
    let r = rows(&t, menu);
    // [label, spacer]
    assert_eq!(kids(&t, r[0]).len(), 2, "只有标签 + 撑开");
    assert_eq!(label_of(&t, kids(&t, r[0])[0]), "普通");

    // 出现勾选时：**所有**项都多一个定宽槽（文字仍左对齐）
    let (t2, menu2, _) = menu_of(|m| {
        m.item("粗体").checked(true);
        m.item("斜体");
    });
    let r2 = rows(&t2, menu2);
    assert_eq!(kids(&t2, r2[0]).len(), 3, "勾 + 标签 + 撑开");
    assert_eq!(kids(&t2, r2[1]).len(), 3, "未勾选的项也留同宽空槽");
    assert_eq!(
        label_of(&t2, kids(&t2, r2[0])[0]),
        icon_char("check").to_string(),
        "勾上的是图标字体的 ✓ 码点"
    );
    assert_eq!(label_of(&t2, kids(&t2, r2[1])[0]), "", "未勾选是空文本");
    // 定宽 ⇒ 文字起始 x 一致
    let x_label = crate::layout::rect_of(&t2, kids(&t2, r2[0])[1]).x;
    let x_other = crate::layout::rect_of(&t2, kids(&t2, r2[1])[1]).x;
    assert!((x_label - x_other).abs() < 0.01, "文字左对齐：{x_label} vs {x_other}");
}

/// 图标槽同理
#[test]
fn icon_slot_is_blank_when_an_item_has_none() {
    let (t, menu, _) = menu_of(|m| {
        m.item("复制").icon("content_copy");
        m.item("粘贴");
    });
    let r = rows(&t, menu);
    assert_eq!(kids(&t, r[0]).len(), 3, "图标 + 标签 + 撑开");
    assert_eq!(kids(&t, r[1]).len(), 3, "留空槽保持对齐");
    let glyph = icon_char("content_copy");
    assert_eq!(label_of(&t, kids(&t, r[0])[0]), glyph.to_string());
    assert_eq!(label_of(&t, kids(&t, r[1])[0]), "");
}

/// 快捷键在标签之后、贴行尾（spacer 撑开）
#[test]
fn accelerator_sits_at_the_right_edge() {
    let (t, menu, _) = menu_of(|m| {
        m.item("复制").accelerator("Ctrl+C");
    });
    let row = rows(&t, menu)[0];
    let kids = &t.get(row).unwrap().children;
    assert_eq!(kids.len(), 3, "标签 + 撑开 + 快捷键");
    assert_eq!(label_of(&t, kids[2]), "Ctrl+C");
    let label_r = crate::layout::rect_of(&t, kids[0]);
    let accel_r = crate::layout::rect_of(&t, kids[2]);
    assert!(accel_r.x > label_r.right(), "快捷键在标签右侧");
    // 贴到菜单右边缘（留出菜单的右内边距）
    let menu_r = crate::layout::rect_of(&t, menu);
    assert!(
        (menu_r.right() - ITEM_PAD_X - accel_r.right()).abs() < 1.0,
        "快捷键贴右内边距：{accel_r:?} 菜单 {menu_r:?}"
    );
}

/// 禁用态：灰字 + `enabled = false`（框架在事件路由阶段跳过禁用节点）
#[test]
fn disabled_item_is_dimmed_and_marked_disabled() {
    let theme = Theme::dark();
    let (t, menu, _) = menu_of(|m| {
        m.item("剪切").enabled(false);
        m.item("复制");
    });
    let r = rows(&t, menu);
    let off = t.get(r[0]).unwrap();
    assert!(!off.interaction.enabled, "节点被标记为禁用");
    let fg_off = kids(&t, r[0])[0];
    assert_eq!(
        t.get(fg_off).unwrap().text.color,
        theme.text_secondary,
        "禁用项用次要文字色"
    );
    let on = t.get(r[1]).unwrap();
    assert!(on.interaction.enabled);
    assert_eq!(t.get(kids(&t, r[1])[0]).unwrap().text.color, theme.text);
}

/// 点击挂在**行**上（不是标签文本）：hit 测试给行，`.on_tap` 也挂行
#[test]
fn tap_handler_is_attached_to_the_row() {
    let (t, menu, _) = menu_of(|m| {
        m.item("打开").on_tap(|| {});
    });
    let row = rows(&t, menu)[0];
    let n = t.get(row).unwrap();
    assert_eq!(n.handlers.len(), 1, "行上一个处理器");
    assert_eq!(n.handlers[0].kind, EventKind::Tapped);
    assert!(t.get(kids(&t, row)[0]).unwrap().handlers.is_empty(), "标签不挂处理器");
}

/// hover / 按下底色取主题 token（菜单外观统一由主题决定）
#[test]
fn hover_colors_come_from_the_theme() {
    let theme = Theme::dark();
    let (t, menu, _) = menu_of(|m| {
        m.item("打开");
    });
    let row = rows(&t, menu)[0];
    let p = &t.get(row).unwrap().paint;
    assert_eq!(p.hover_background, Some(theme.control_hover));
    assert_eq!(p.pressed_background, Some(theme.control_pressed));
    assert_eq!(
        t.get(row).unwrap().paint.border_radius,
        theme.control_radius,
        "圆角跟随主题"
    );
}

/// 宽度有下限、内容自适应
#[test]
fn menu_width_has_a_floor_but_grows_with_content() {
    let (t, menu, _) = menu_of(|m| {
        m.item("短");
    });
    let menu_r = crate::layout::rect_of(&t, menu);
    assert!(menu_r.width >= MENU_MIN_WIDTH, "下限：{menu_r:?}");
    // 弹层收缩到内容（不应被拉成整窗）
    assert!(menu_r.width < WIN.width);

    let (t2, menu2, _) = menu_of(|m| {
        m.item("这是一个非常非常长的菜单项标签");
    });
    let wide = crate::layout::rect_of(&t2, menu2);
    assert!(
        wide.width > menu_r.width,
        "长标签让菜单变宽而不是截断：{wide:?} vs {menu_r:?}"
    );
}

/// `min_width` 可覆盖（窄菜单 / 宽菜单）
#[test]
fn min_width_is_configurable() {
    let (t, menu, _) = menu_of(|m| {
        m.min_width(240.0);
        m.item("短");
    });
    assert!(crate::layout::rect_of(&t, menu).width >= 240.0);
}

/// 空菜单也不该崩（只有一层容器 + 上下内边距）
#[test]
fn empty_menu_is_still_a_container() {
    let (t, menu, popup) = menu_of(|_| {});
    assert_eq!(rows(&t, menu).len(), 0);
    let popup_r = crate::layout::rect_of(&t, popup);
    assert!(popup_r.height >= MENU_PAD_Y * 2.0, "至少留出上下内边距");
}

/// 项标签为空串也不该 panic（图标槽就是靠空串实现的）
#[test]
fn empty_label_is_allowed() {
    let (t, menu, _) = menu_of(|m| {
        m.item("");
    });
    let row = rows(&t, menu)[0];
    assert_eq!(label_of(&t, kids(&t, row)[0]), "");
    assert!(crate::layout::rect_of(&t, row).height > 0.0);
}

/// 菜单在弹层里**贴指针**（点锚点）：左上角就是那个点
#[test]
fn menu_is_positioned_at_the_anchor_point() {
    let mut v = ViewBuf::new();
    v.begin();
    v.column(|c| {
        c.text("body");
    });
    let at = crate::geom::Point::new(40.0, 30.0);
    v.popup_at_point(at, crate::track::Placement::Below, |p| {
        p.menu(|m| {
            m.item("x");
        });
    });
    let mut t = Track::new();
    crate::align::align(&mut t, &v);
    layout(&mut t, WIN);
    place_anchored_layers(&mut t, WIN);
    let popup = t.roots().iter().find(|r| r.layer == Layer::Popup).unwrap().node;
    let r: Rect = crate::layout::rect_of(&t, popup);
    assert!((r.x - at.x).abs() < 0.5 && (r.y - (at.y + 4.0)).abs() < 0.5, "{r:?}");
}
