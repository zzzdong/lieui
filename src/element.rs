pub mod div;
pub mod text;
pub mod world;

pub use div::DivElement;
pub use text::TextElement;
pub use world::ElementRef;

use taffy::{
    AvailableSpace, Dimension, Layout as TaffyLayout, NodeId, NodeId as TaffyId, PrintTree,
    Size as TaffySize, Style as TaffyStyle, TaffyTree,
    prelude::{TaffyMaxContent, length},
};
use vello_cpu::{RenderContext, kurbo::Rect};

use crate::event::{PointerEvent, PointerEventType};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementId(u64);

pub trait IElement<T> {
    fn paint(&mut self, cx: &mut RenderContext, layout: &TaffyLayout) {}

    fn measure(
        &mut self,
        size: TaffySize<Option<f32>>,
        available: TaffySize<AvailableSpace>,
        style: &TaffyStyle,
    ) -> TaffySize<f32> {
        TaffySize::ZERO
    }

    fn layout_style(&self) -> TaffyStyle {
        TaffyStyle::default()
    }

    fn on_event(&mut self, event: winit::event::WindowEvent, state: &mut T) -> bool {
        false
    }

    /// 指针按下事件处理
    fn on_pointer_pressed(&mut self, event: &PointerEvent, state: &mut T) -> bool {
        false
    }

    /// 指针释放事件处理
    fn on_pointer_released(&mut self, event: &PointerEvent, state: &mut T) -> bool {
        false
    }

    /// 指针移动事件处理
    fn on_pointer_moved(&mut self, event: &PointerEvent, state: &mut T) -> bool {
        false
    }

    /// 指针进入元素边界事件处理
    fn on_pointer_entered(&mut self, event: &PointerEvent, state: &mut T) -> bool {
        false
    }

    /// 指针离开元素边界事件处理
    fn on_pointer_exited(&mut self, event: &PointerEvent, state: &mut T) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use taffy::prelude::length;
    use vello_cpu::{Pixmap, peniko::Color};

    use crate::{
        element::{
            DivElement, TextElement,
            world::{ElementNode, ElementTree},
        },
        paint::TextColor,
    };

    use super::*;

    #[test]
    fn test_layout() {
        let state = String::new();

        let mut tree = ElementTree::<String>::new();

        // 创建根节点
        let root_node = ElementNode::new(DivElement {
            style: TaffyStyle {
                size: TaffySize {
                    width: length(100.0),
                    height: length(100.0),
                },
                ..Default::default()
            },
            children: vec![],
        });

        let root = tree.add_node(root_node);
        tree.set_root(root);

        let mut text = TextElement::new("Hello, 中文!".to_string());
        text.style.brush = Color::from_rgb8(255, 0, 0).into();

        tree.insert_child(root, ElementNode::new(text));

        tree.do_layout(800.0, 600.0);

        let mut render_cx = RenderContext::new(800, 600);

        tree.do_paint(&mut render_cx);

        let mut pixmap = Pixmap::new(800, 600);
        render_cx.render_to_pixmap(&mut pixmap);

        let png = pixmap.into_png().expect("failed to encode png");

        std::fs::write("test.png", png).expect("failed to write png")
    }
}
