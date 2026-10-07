use super::*;

#[test]
fn icon_creates_icon_font_text() {
    let mut v = ViewBuf::new();
    v.begin();
    v.column(|c| {
        c.icon("close");
        c.icon("no-such-icon");
    });
    let root = &v.nodes[v.roots[0].node as usize];
    let a = &v.nodes[root.children[0] as usize];
    let b = &v.nodes[root.children[1] as usize];
    match &a.kind {
        KindDesc::Text(s) => assert_eq!(s, "\u{e5cd}", "close = e5cd"),
        _ => panic!("icon 应是 Text"),
    }
    assert_eq!(a.text.spec.font_family.as_str(), crate::icon::ICON_FONT_FAMILY);
    assert!(!a.text.spec.wrap, "图标不换行");
    assert_eq!(a.text.spec.font_size, 20.0);
    match &b.kind {
        KindDesc::Text(s) => assert_eq!(s.as_str(), "□"),
        _ => panic!(),
    }
}

#[test]
fn icon_button_is_a_button_with_icon_glyph() {
    let mut v = ViewBuf::new();
    v.begin();
    v.column(|c| {
        c.icon_button("settings");
    });
    let root = &v.nodes[v.roots[0].node as usize];
    let a = &v.nodes[root.children[0] as usize];
    assert_eq!(a.kind.tag(), crate::track::KindTag::Button, "icon_button 走按钮交互");
    match &a.kind {
        KindDesc::Button { label } => assert_eq!(label, "\u{e8b8}"),
        _ => panic!(),
    }
    assert_eq!(a.text.spec.font_family, crate::icon::ICON_FONT_FAMILY);
}

/// 浮层的两处"框架默认视觉"走主题 token：Modal 遮罩 = `theme.backdrop`、
/// 弹层投影色 = `theme.shadow`（此前是硬编码的黑）。
#[test]
fn layer_defaults_use_theme_tokens() {
    let dark = crate::theme::Theme::dark();
    let mut v = ViewBuf::new();
    v.set_theme(dark);
    v.begin();
    v.text("anchor").key("anchor");
    v.popup_at("anchor", Placement::Below, |p| {
        p.text("popup");
    });
    v.modal(|m| {
        m.text("modal");
    });

    let popup = v.roots.iter().find(|r| r.layer == Layer::Popup).unwrap();
    let shadow = v.nodes[popup.node as usize].paint.shadow.expect("弹层根有投影");
    assert_eq!(shadow.color, dark.shadow, "投影色 = theme.shadow");

    let modal = v.roots.iter().find(|r| r.layer == Layer::Modal).unwrap();
    assert_eq!(modal.opts.backdrop, Some(dark.backdrop), "遮罩 = theme.backdrop");
}

#[test]
fn builds_a_nested_tree_with_content_root() {
    let mut v = ViewBuf::new();
    v.begin();
    v.column(|c| {
        c.gap(12.0);
        c.text("title").font_size(48.0);
        c.row(|r| {
            r.gap(4.0);
            r.button("a");
            r.button("b");
        });
    });

    // 1 个内容根
    assert_eq!(v.roots.len(), 1);
    assert_eq!(v.roots[0].layer, Layer::Content);

    // 结构：column → [title, row → [a, b]]
    let root = &v.nodes[v.roots[0].node as usize];
    assert_eq!(root.children.len(), 2);
    assert_eq!(root.layout.item_space, 12.0);
    assert!(matches!(root.kind, KindDesc::Box));
    assert_eq!(root.children[0], 1);
    let title = &v.nodes[1];
    assert!(matches!(title.kind, KindDesc::Text(_)));
    assert_eq!(title.text.spec.font_size, 48.0);
    let row = &v.nodes[root.children[1] as usize];
    assert_eq!(row.children.len(), 2);
    assert_eq!(
        v.nodes[row.children[0] as usize].kind.tag(),
        crate::track::KindTag::Button
    );
    assert_eq!(v.nodes.len(), 5);
}

#[test]
fn begin_resets_for_reuse() {
    let mut v = ViewBuf::new();
    v.begin();
    v.column(|c| {
        c.text("x");
    });
    let cap = v.nodes.capacity();

    v.begin();
    assert!(v.nodes.is_empty());
    assert!(v.roots.is_empty());
    v.column(|c| {
        c.text("y");
    });
    assert_eq!(v.nodes.len(), 2, "复用后不残留");
    assert!(v.nodes.capacity() >= cap);
}

#[test]
fn layers_are_nested_and_owned() {
    let mut v = ViewBuf::new();
    v.begin();
    v.column(|c| {
        c.text("body");
    });
    v.overlay(|o| {
        o.text("WATERMARK");
    });
    v.modal(|m| {
        m.text("确认？");
        m.popup_at("more", Placement::RightOf, |p| {
            p.text("子菜单");
        });
    });

    let layers: Vec<Layer> = v.roots.iter().map(|r| r.layer).collect();
    assert_eq!(layers, vec![Layer::Content, Layer::Overlay, Layer::Modal, Layer::Popup]);

    // Content 无 owner；Overlay/Modal 的 owner 都是内容根；Popup 的 owner 是 Modal
    let content = 0;
    let modal = 2;
    let popup: usize = 3;
    assert_eq!(v.roots[content].owner, None);
    assert_eq!(v.roots[1].owner, Some(content as u32));
    assert_eq!(v.roots[modal].owner, Some(content as u32));
    assert_eq!(v.roots[popup].owner, Some(modal as u32));

    // modal 的 opts 默认：backdrop + 阻断下层（居中由层语义给出，不占用 anchor）
    let mo = &v.roots[modal].opts;
    assert!(mo.backdrop.is_some());
    assert!(mo.blocks_below);
    assert!(mo.anchor.is_none());
    // overlay 默认命中穿透
    assert!(!v.roots[1].opts.hit_test_visible);
    // popup 锚点是 key
    assert_eq!(
        v.roots[popup].opts.anchor.as_ref().map(|a| match &a.target {
            crate::track::AnchorTarget::Key(k) => k.clone(),
            _ => panic!("popup 锚点应为 key"),
        }),
        Some(Key::Str("more".into()))
    );
}

#[test]
fn popup_at_point_records_a_point_anchor() {
    let mut v = ViewBuf::new();
    v.begin();
    v.column(|c| {
        c.text("body");
    });
    v.popup_at_point(Point::new(120.0, 60.0), Placement::Below, |p| {
        p.text("菜单项");
    });

    let popup = v
        .roots
        .iter()
        .find(|r| r.layer == Layer::Popup)
        .expect("应声明了 Popup 层");
    match &popup.opts.anchor.as_ref().unwrap().target {
        crate::track::AnchorTarget::Point(p) => {
            assert_eq!(*p, Point::new(120.0, 60.0), "锚点就是那个点");
        }
        other => panic!("点锚点应记为 Point：{other:?}"),
    }
}

#[test]
fn keyed_list_records_keys_in_order() {
    let mut v = ViewBuf::new();
    v.begin();
    let items = [10u64, 20, 30];
    v.column(|c| {
        c.keyed_list(
            items.iter().copied(),
            |id| *id,
            |v, id| {
                v.text(format!("item {id}"));
            },
        );
    });

    let root = &v.nodes[v.roots[0].node as usize];
    assert_eq!(root.children.len(), 3);
    assert_eq!(
        root.child_keys,
        vec![Some(Key::U64(10)), Some(Key::U64(20)), Some(Key::U64(30))]
    );
    // 顺序与 children 对齐
    let first = &v.nodes[root.children[0] as usize];
    match &first.kind {
        KindDesc::Text(s) => assert_eq!(s, "item 10"),
        _ => panic!(),
    }
}

#[test]
fn handlers_are_recorded_per_event_kind() {
    let mut v = ViewBuf::new();
    v.begin();
    v.column(|c| {
        c.button("+1").on_tap(|| {});
        c.button("ctx").on_tap_with(|_cx| {});
    });
    let root = &v.nodes[v.roots[0].node as usize];
    let a = &v.nodes[root.children[0] as usize];
    let b = &v.nodes[root.children[1] as usize];
    assert_eq!(a.handlers.len(), 1);
    assert_eq!(a.handlers[0].kind, EventKind::Tapped);
    assert_eq!(b.handlers[0].kind, EventKind::Tapped);
    assert!(!a.handlers[0].handled_events_too);
}

#[test]
fn spacer_and_scroll_configuration() {
    let mut v = ViewBuf::new();
    v.begin();
    v.column(|c| {
        c.spacer();
        c.scroll(|s| s.expand(true));
    });
    let root = &v.nodes[v.roots[0].node as usize];
    assert_eq!(v.nodes[root.children[0] as usize].layout.flex_grow, 1.0);
    let sc = &v.nodes[root.children[1] as usize];
    assert!(sc.layout.overflow_scroll);
    assert!(sc.paint.clip_content);
    assert_eq!(sc.layout.flex_grow, 1.0);
}

#[test]
#[should_panic(expected = "内容根只能声明一次")]
fn second_content_root_panics() {
    let mut v = ViewBuf::new();
    v.begin();
    v.column(|c| {
        c.text("a");
    });
    v.column(|c| {
        c.text("b");
    }); // 顶层第二个内容根 → 应当被断言拦下
}
