//! Button widget 布局回归测试
//!
//! 确保按钮文本在按钮背景区域内水平和垂直居中。

use lieui::geometry::Size;
use lieui::render::visual::{LayeredElement, VisualElement};
use lieui::runtime::Runtime;
use lieui::text::TextEngine;
use lieui::view::widget::Button;
use lieui::view::View;

fn find_button_parts(
    elements: &[LayeredElement],
    label: &str,
) -> Option<(lieui::render::visual::KRect, (f64, f64, f64, f64))> {
    let mut rect = None;
    let mut text = None;
    for e in elements {
        match &e.element {
            VisualElement::RoundedRect { rect: r, .. } if rect.is_none() => {
                rect = Some(*r);
            }
            VisualElement::TextRun {
                text: t,
                position,
                font_size,
                ..
            } if t.as_ref() == label => {
                let (tw, th) = TextEngine::measure_text(t.as_ref(), *font_size, None);
                text = Some((position.x, position.y, tw, th));
            }
            _ => {}
        }
    }
    Some((rect?, text?))
}

#[test]
fn button_text_is_centered() {
    let label = "Load Sample";
    let vt = Button::new(label).build();

    let mut rt = Runtime::new(Size::new(400.0, 200.0));
    rt.submit_view_tree(vt);
    let elements = rt.frame();

    let (rect, (tx, ty, tw, th)) =
        find_button_parts(&elements, label).expect("button rect and text not found");

    let rect_cx = (rect.x0 + rect.x1) / 2.0;
    let rect_cy = (rect.y0 + rect.y1) / 2.0;
    let text_cx = tx + tw / 2.0;
    let text_cy = ty + th / 2.0;

    let dx = (text_cx - rect_cx).abs();
    let dy = (text_cy - rect_cy).abs();

    assert!(
        dx < 1.0,
        "text not horizontally centered: dx={}, rect_cx={}, text_cx={}",
        dx,
        rect_cx,
        text_cx
    );
    assert!(
        dy < 1.0,
        "text not vertically centered: dy={}, rect_cy={}, text_cy={}",
        dy,
        rect_cy,
        text_cy
    );
}
