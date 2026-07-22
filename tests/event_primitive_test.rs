//! 事件系统与 ViewNode 原语集成测试

use lieui::geometry::{Point, Size};
use lieui::prelude::*;
use lieui::runtime::Runtime;
use lieui::state;
use lieui::view::View;

#[test]
fn button_hit_test_returns_listener() {
    let mut runtime = Runtime::new(Size::new(800.0, 600.0));

    let clicked = state::State::new(false);
    let c = clicked.clone();
    let vt = Column::new()
        .child(Button::new("Click me").on_click(move || {
            c.set(true);
        }))
        .build();

    runtime.submit_view_tree(vt);
    let _ = runtime.frame();

    // Button 大致位于 (4,4) ~ (80,40) 区域
    let hit = runtime.layers.hit_test_top(Point::new(20.0, 20.0));
    assert!(hit.is_some(), "hit test should find the button");
    let (layer, id) = hit.unwrap();
    assert_eq!(layer, lieui::core::layers::LayerType::Base);

    // 命中的是 Listener 节点，on_click 有值
    let cb = runtime.layers.tree.on_click(id);
    assert!(cb.is_some(), "hit element should have on_click callback");

    // 触发回调
    state::invoke_click(cb.unwrap());
    assert!(*clicked.get(), "callback should have been invoked");
}

#[test]
fn button_hover_state_affects_background_render() {
    let mut runtime = Runtime::new(Size::new(800.0, 600.0));

    let vt = Column::new().child(Button::new("Hover me")).build();

    runtime.submit_view_tree(vt);
    let _ = runtime.frame();

    let hit = runtime.layers.hit_test_top(Point::new(20.0, 20.0));
    let (_, id) = hit.expect("should hit button");

    // 未 hover 时渲染列表只有文字
    let normal = runtime.frame_render_only();
    let bg_count_normal = normal
        .iter()
        .filter(|e| {
            matches!(
                e.element,
                lieui::render::visual::VisualElement::RoundedRect { .. }
            )
        })
        .count();
    assert_eq!(bg_count_normal, 0, "no background when not hovered");

    // 设置 hover 状态
    let mut s = runtime.layers.tree.state(id);
    s.hovered = true;
    runtime.layers.tree.set_state(id, s);

    let hovered = runtime.frame_render_only();
    let bg_count_hovered = hovered
        .iter()
        .filter(|e| {
            matches!(
                e.element,
                lieui::render::visual::VisualElement::RoundedRect { .. }
            )
        })
        .count();
    assert_eq!(bg_count_hovered, 1, "hover background should be rendered");
}

#[test]
fn checkbox_hit_test_returns_listener() {
    let mut runtime = Runtime::new(Size::new(800.0, 600.0));

    let checked = state::State::new(false);
    let c = checked.clone();
    let vt = Row::new()
        .child(Checkbox::new(false).label("Toggle").on_click(move || {
            c.set(!*c.get());
        }))
        .build();

    runtime.submit_view_tree(vt);
    let _ = runtime.frame();

    let hit = runtime.layers.hit_test_top(Point::new(20.0, 20.0));
    assert!(hit.is_some(), "should hit checkbox");
    let (_, id) = hit.unwrap();
    eprintln!("checkbox hit id = {:?}", id);
    let node = runtime.layers.tree.get_node(id);
    eprintln!("checkbox hit node = {:?}", node.type_name());
    let cb = runtime.layers.tree.on_click(id);
    assert!(cb.is_some(), "checkbox should have on_click");

    state::invoke_click(cb.unwrap());
    assert!(*checked.get(), "checkbox callback should toggle state");
}
