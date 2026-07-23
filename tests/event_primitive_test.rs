//! 事件系统与 ViewNode 原语集成测试

use lieui::core::layers::LayerType;
use lieui::geometry::Size;
use lieui::prelude::*;
use lieui::runtime::Runtime;
use lieui::state;
use lieui::view::{node::NodeType, View};

#[test]
fn button_hit_test_returns_listener() {
    let mut runtime = Runtime::new(Size::new(800.0, 600.0));

    let clicked = state::State::new(false);
    let c = clicked.clone();
    let vt = Column::new()
        .child(
            Button::new("Click me")
                .on_click(move || {
                    c.set(true);
                }),
        )
        .build();

    runtime.submit_view_tree(vt);
    let _ = runtime.frame();

    // Button 大致位于 (4,4) ~ (80,40) 区域
    // 从根节点向下找到第一个 Listener
    let root_id = runtime.layers.layer_root(LayerType::Base).unwrap();
    let listener_id = find_listener_in_tree(&runtime, root_id).expect("should find a Listener");
    let cb = runtime.layers.tree.on_click(listener_id);
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

    let root_id = runtime.layers.layer_root(LayerType::Base).unwrap();
    let listener_id = find_listener_in_tree(&runtime, root_id).expect("should find a Listener");

    // 未 hover 时按钮有默认背景色 (220,220,220)
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
    assert_eq!(bg_count_normal, 1, "button always has default background");

    // 设置 hover 状态（在 Listener 上设置，状态会传播到子节点）
    let mut s = runtime.layers.tree.state(listener_id);
    s.hovered = true;
    runtime.layers.tree.set_state(listener_id, s);

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
    assert_eq!(bg_count_hovered, 1, "hover background should also appear");
}

#[test]
fn checkbox_hit_test_returns_listener() {
    let mut runtime = Runtime::new(Size::new(800.0, 600.0));

    let checked = state::State::new(false);
    let c = checked.clone();
    let vt = Row::new()
        .child(
            Checkbox::new(false)
                .label("Toggle")
                .on_click(move || {
                    let cur = *c.get();
                    c.set(!cur);
                }),
        )
        .build();

    runtime.submit_view_tree(vt);
    let _ = runtime.frame();

    let root_id = runtime.layers.layer_root(LayerType::Base).unwrap();
    let listener_id = find_listener_in_tree(&runtime, root_id).expect("should find a Listener");
    let node = runtime.layers.tree.get_node(listener_id);
    eprintln!("checkbox listener node = {:?}", node.type_name());
    let cb = runtime.layers.tree.on_click(listener_id);
    assert!(cb.is_some(), "checkbox should have on_click");

    state::invoke_click(cb.unwrap());
    assert!(*checked.get(), "checkbox callback should toggle state");
}

/// 在 ElementTree 中递归查找第一个 Listener 节点
fn find_listener_in_tree(runtime: &Runtime, id: lieui::core::ElementId) -> Option<lieui::core::ElementId> {
    if let Some(n) = runtime.layers.tree.get_node_ref(id) {
        if n.node_type() == NodeType::Listener {
            return Some(id);
        }
    }
    for child_id in runtime.layers.tree.children_of(id).to_vec() {
        if let Some(found) = find_listener_in_tree(runtime, child_id) {
            return Some(found);
        }
    }
    None
}
