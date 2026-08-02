//! 图标字体 / Icon / IconButton 集成测试（无头）
//!
//! 验证：字体文件可注册且 family 名正确；IconName 码点与随附 codepoints 映射一致；
//! Icon 产出带字形与图标字体的 Text 节点；IconButton 外壳 + 点击 + hover/pressed 变色接线；
//! 渲染管线输出带图标字体的 TextRun。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use lieui::core::ElementId;
use lieui::event::{Event, EventContext, EventType, HitTestResult, Modifiers, MouseButton};
use lieui::geometry::{Color, Point, Size};
use lieui::render::visual::VisualElement;
use lieui::runtime::{ElementTree, Runtime};
use lieui::view::node::{Callback, ViewNode};
use lieui::widget::{
    BuildContext, Icon, IconButton, IconButtonVariant, IconName, StateMap, Widget,
};

fn new_ctx() -> BuildContext {
    BuildContext::new(Rc::new(RefCell::new(StateMap::new())))
}

/// 复刻 Application::handle_lie_event 的分发语义。
fn handle_lie_event(tree: &ElementTree, id: ElementId, event: &Event, ctx: &mut EventContext) {
    ctx.set_event(event.clone());
    ctx.set_current(id, tree.layout(id).rect());
    lieui::event::dispatch_node_listeners(tree, id, event, ctx);
}

/// 读取随附的 codepoints 映射（name -> hex）。
fn codepoint_map() -> HashMap<String, String> {
    std::fs::read_to_string(format!(
        "{}/assets/MaterialIcons-Regular.codepoints",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("codepoints file should exist")
    .lines()
    .filter_map(|l| l.split_once(' '))
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

#[test]
fn icon_font_is_embedded_and_valid() {
    // 内嵌字节与随附字体文件一致，且是合法 TrueType（magic 0x00010000）。
    let embedded = lieui::widget::icon::ICON_FONT_BYTES;
    let file = std::fs::read(format!(
        "{}/assets/MaterialIcons-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("font file should exist");
    assert_eq!(embedded, file.as_slice());
    assert!(
        embedded.starts_with(&[0x00, 0x01, 0x00, 0x00]),
        "not a TrueType font"
    );
}

#[test]
fn icon_name_codepoints_match_asset_map() {
    let map = codepoint_map();
    let samples = [
        (IconName::Add, "add"),
        (IconName::Remove, "remove"),
        (IconName::Close, "close"),
        (IconName::Check, "check"),
        (IconName::Done, "done"),
        (IconName::Search, "search"),
        (IconName::Settings, "settings"),
        (IconName::Delete, "delete"),
        (IconName::Menu, "menu"),
        (IconName::ArrowForward, "arrow_forward"),
        (IconName::Home, "home"),
        (IconName::Person, "person"),
        (IconName::Folder, "folder"),
        (IconName::Star, "star"),
        (IconName::Favorite, "favorite"),
        (IconName::Save, "save"),
        (IconName::Edit, "edit"),
        (IconName::PlayArrow, "play_arrow"),
        (IconName::Refresh, "refresh"),
        (IconName::Mail, "mail"),
        (IconName::Info, "info"),
        (IconName::Warning, "warning"),
        (IconName::Error, "error"),
        (IconName::CheckCircle, "check_circle"),
        (IconName::Lock, "lock"),
        (IconName::ShoppingCart, "shopping_cart"),
        (IconName::Notifications, "notifications"),
        (IconName::Visibility, "visibility"),
        (IconName::List, "list"),
        (IconName::Download, "download"),
        (IconName::Upload, "upload"),
        (IconName::OpenInNew, "open_in_new"),
        (IconName::DateRange, "date_range"),
        (IconName::CloseFullscreen, "close_fullscreen"),
        (IconName::RotateLeft, "rotate_left"),
        (IconName::RotateRight, "rotate_right"),
        (IconName::Rotate90DegreesCcw, "rotate_90_degrees_ccw"),
        (IconName::Rotate90DegreesCw, "rotate_90_degrees_cw"),
        (IconName::Flip, "flip"),
        (IconName::FlipToBack, "flip_to_back"),
        (IconName::FlipToFront, "flip_to_front"),
        (IconName::ArrowDropDown, "arrow_drop_down"),
        (IconName::ArrowCircleRight, "arrow_circle_right"),
        (IconName::CheckBox, "check_box"),
        (IconName::CheckBoxOutlineBlank, "check_box_outline_blank"),
        (IconName::RadioButtonUnchecked, "radio_button_unchecked"),
        (IconName::ToggleOn, "toggle_on"),
        (IconName::FormatBold, "format_bold"),
        (IconName::FormatItalic, "format_italic"),
        (IconName::FormatUnderlined, "format_underlined"),
        (IconName::FormatAlignLeft, "format_align_left"),
        (IconName::FormatListBulleted, "format_list_bulleted"),
        (IconName::Shuffle, "shuffle"),
        (IconName::Repeat, "repeat"),
        (IconName::FastForward, "fast_forward"),
        (IconName::PlaylistAdd, "playlist_add"),
        (IconName::ContentCut, "content_cut"),
        (IconName::ContentPaste, "content_paste"),
        (IconName::FolderOpen, "folder_open"),
        (IconName::MoreHoriz, "more_horiz"),
        (IconName::Cached, "cached"),
        (IconName::Wifi, "wifi"),
        (IconName::Bluetooth, "bluetooth"),
        (IconName::BatteryFull, "battery_full"),
        (IconName::ChatBubble, "chat_bubble"),
        (IconName::Keyboard, "keyboard"),
        (IconName::Mouse, "mouse"),
        (IconName::Laptop, "laptop"),
        (IconName::Gamepad, "gamepad"),
        (IconName::GroupAdd, "group_add"),
        (IconName::Map, "map"),
        (IconName::Directions, "directions"),
        (IconName::Restaurant, "restaurant"),
        (IconName::LocalCafe, "local_cafe"),
        (IconName::AddAPhoto, "add_a_photo"),
        (IconName::CameraAlt, "camera_alt"),
        (IconName::Collections, "collections"),
        (IconName::Landscape, "landscape"),
    ];
    for (icon, name) in samples {
        let expected = map.get(name).expect("name should exist in map");
        assert_eq!(
            format!("{:x}", icon.codepoint()),
            *expected,
            "codepoint mismatch for {name}"
        );
    }
}

#[test]
fn icon_name_from_str_roundtrip() {
    // 抽样验证 name_str() 与 FromStr 双向转换
    let cases = [
        IconName::Add,
        IconName::DeleteForever,
        IconName::Wifi,
        IconName::FormatBold,
        IconName::Rotate90DegreesCcw,
        IconName::ArrowDropDown,
        IconName::CheckBoxOutlineBlank,
        IconName::DirectionsWalk,
        IconName::AddAPhoto,
        IconName::LocalGroceryStore,
        IconName::LocalPrintShop,
    ];
    for icon in cases {
        let name = icon.name_str();
        let parsed: IconName = name.parse().unwrap();
        assert_eq!(parsed, icon, "roundtrip failed for {name}");
    }
    assert!("non_existent_icon".parse::<IconName>().is_err());
}

#[test]
fn icon_builds_text_node_with_glyph_and_font() {
    let mut ctx = new_ctx();
    let node = Icon::new(IconName::Add, 20.0).build(&mut ctx);
    match node {
        ViewNode::Text { content, style, .. } => {
            assert_eq!(content, IconName::Add.char().to_string());
            assert_eq!(style.font_size, 20.0);
            assert_eq!(style.font_family, IconName::FONT_FAMILY);
            assert!(!style.wrap, "icon glyph should stay single-line");
            assert_eq!(
                style.color,
                lieui::theme::current().text.regular_default,
                "default color should come from theme"
            );
        }
        other => panic!("expected Text node, got {:?}", other.node_type()),
    }
}

#[test]
fn icon_button_wires_click_variants_and_icon_child() {
    let clicked = Rc::new(RefCell::new(0i32));
    let c = Rc::clone(&clicked);
    let mut ctx = new_ctx();
    let node = IconButton::new(IconName::Search)
        .variant(IconButtonVariant::Primary)
        .size(32.0)
        .icon_size(20.0)
        .on_click(move || *c.borrow_mut() += 1)
        .build(&mut ctx);

    let ViewNode::Div {
        layout,
        paint,
        children,
        listeners,
        ..
    } = node
    else {
        panic!("IconButton root should be Div");
    };

    // 外壳尺寸
    assert_eq!(layout.dim[0], 32.0);
    assert_eq!(layout.dim[1], 32.0);
    // Primary：品牌色背景 + 圆角
    assert!(paint.background_color.is_some(), "primary should have bg");
    assert!(paint.border_radius > 0.0);

    // 点击监听器
    let click_listeners: Vec<_> = listeners
        .iter()
        .filter(|l| l.event == EventType::Click)
        .collect();
    assert_eq!(click_listeners.len(), 1);
    if let Callback::Simple(cb) = &click_listeners[0].callback {
        cb();
    } else {
        panic!("on_click should be Simple callback");
    }
    assert_eq!(*clicked.borrow(), 1, "click callback should fire");

    // 子节点：通用 Button 将内容放在一个居中包裹 Div 内，最里层是图标字形节点。
    assert_eq!(children.len(), 1);
    let ViewNode::Div {
        children: inner, ..
    } = &children[0]
    else {
        panic!("content wrapper should be Div");
    };
    assert_eq!(inner.len(), 1);
    let ViewNode::Text { content, style, .. } = &inner[0] else {
        panic!("icon child should be Text");
    };
    assert_eq!(content, &IconName::Search.char().to_string());
    assert_eq!(style.font_size, 20.0);
    assert_eq!(style.font_family, IconName::FONT_FAMILY);
    assert_eq!(style.color, Color::WHITE, "primary variant uses white icon");
    assert!(style.hover_color.is_some() && style.pressed_color.is_some());
}

#[test]
fn icon_button_renders_textrun_with_icon_font() {
    let mut runtime = Runtime::new(Size::new(200.0, 100.0));
    let mut ctx = BuildContext::new(Rc::new(RefCell::new(StateMap::new())));
    let vt = IconButton::new(IconName::Settings).build(&mut ctx);
    runtime.submit_view_tree(vt, true);
    let _ = runtime.frame();

    let visuals = runtime.frame_render_only();
    let runs: Vec<_> = visuals
        .iter()
        .filter_map(|l| match &l.element {
            lieui::render::visual::VisualElement::TextRun {
                text,
                font_family,
                font_size,
                color,
                ..
            } => Some((
                text.as_ref().to_string(),
                font_family.clone(),
                *font_size,
                *color,
            )),
            _ => None,
        })
        .collect();
    assert!(
        runs.iter().any(|(t, f, s, _)| {
            t == &IconName::Settings.char().to_string()
                && f.contains("Material Icons")
                && (*s - 18.0).abs() < 0.01
        }),
        "expected icon TextRun, got {:?}",
        runs
    );
}

#[test]
fn icon_button_without_listener_still_gets_hover_and_pressed() {
    let mut runtime = Runtime::new(Size::new(200.0, 100.0));
    let mut ctx = new_ctx();
    // 无任何回调的 IconButton：hover/pressed 反馈不应依赖 listener。
    let vt = IconButton::new(IconName::Search).build(&mut ctx);
    runtime.submit_view_tree(vt, true);
    let _ = runtime.frame();

    let root = runtime.layers.content_root_id().expect("root");
    let p = Point::new(14.0, 14.0); // 28x28 按钮中心
    let (_, target, _) = runtime.layers.hit_test_top(p).expect("hit");
    let path = runtime.layers.path_to(target);
    let hit = HitTestResult { target, path };

    // 悬停：无监听器的按钮根节点也应获得 hovered 状态。
    {
        let tree = &runtime.layers.tree;
        let mut em = runtime.layers.event_manager.borrow_mut();
        em.handle_mouse_move(p, Some(&hit), tree, |id, ev, c| {
            handle_lie_event(tree, id, ev, c)
        });
    }
    assert!(
        runtime.layers.tree.state(root).hovered,
        "no-listener icon button should receive hovered state"
    );

    // hover 背景应进入渲染树（Plain 变体 hover 为 secondary_default）。
    let visuals = runtime.frame_render_only();
    let t = lieui::theme::current();
    let has_hover_bg = visuals.iter().any(|l| {
        matches!(
            &l.element,
            VisualElement::RoundedRect { style, .. } if style.fill == Some(t.background.secondary_default)
        )
    });
    assert!(
        has_hover_bg,
        "hover background should render for no-listener button"
    );

    // 按下：同样获得 pressed 状态，释放后清除。
    {
        let tree = &runtime.layers.tree;
        let mut em = runtime.layers.event_manager.borrow_mut();
        em.handle_mouse_down(
            p,
            MouseButton::Left,
            Modifiers::default(),
            &hit,
            tree,
            |id, ev, c| handle_lie_event(tree, id, ev, c),
        );
    }
    assert!(
        runtime.layers.tree.state(root).pressed,
        "no-listener icon button should receive pressed state"
    );

    {
        let tree = &runtime.layers.tree;
        let mut em = runtime.layers.event_manager.borrow_mut();
        em.handle_mouse_up(p, MouseButton::Left, &hit, tree, |id, ev, c| {
            handle_lie_event(tree, id, ev, c)
        });
    }
    assert!(
        !runtime.layers.tree.state(root).pressed,
        "pressed state should clear on release"
    );
}
