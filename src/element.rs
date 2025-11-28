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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementId(u64);

pub trait IElement {
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
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
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

    // #[test]
    // fn test_layout() {
    //     let state = String::new();

    //     let mut tree = ElementTree::new();

    //     // 创建根节点
    //     let root_node = ElementNode::new(DivElement {
    //         style: TaffyStyle {
    //             size: TaffySize {
    //                 width: length(100.0),
    //                 height: length(100.0),
    //             },
    //             ..Default::default()
    //         },
    //         children: vec![],
    //         background_color: Color::from_rgb8(233, 233, 233),
    //     });

    //     let root = tree.add_node(root_node);
    //     tree.set_root(root);

    //     let mut text = TextElement::new("Hello, 中文!".to_string());
    //     text.style.brush = Color::from_rgb8(255, 0, 0).into();

    //     tree.insert_child(root, ElementNode::new(text));

    //     tree.do_layout(800.0, 600.0);

    //     let mut render_cx = RenderContext::new(800, 600);

    //     tree.do_paint(&mut render_cx);

    //     let mut pixmap = Pixmap::new(800, 600);
    //     render_cx.render_to_pixmap(&mut pixmap);

    //     let png = pixmap.into_png().expect("failed to encode png");

    //     std::fs::write("test.png", png).expect("failed to write png")
    // }

    #[test]
    fn test_xml() {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct Div {
            style: taffy::Style,
            children: Vec<Div>,
        }

        let div = Div {
            style: taffy::Style {
                display: taffy::Display::Flex,
                ..Default::default()
            },
            children: vec![],
        };

        let xml = quick_xml::se::to_string(&div).unwrap();

        println!("{}", xml);
    }
}
