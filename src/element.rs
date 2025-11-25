pub mod world;
pub mod div;
pub mod text;

pub use div::DivElement;
pub use text::TextElement;

use taffy::{
    AvailableSpace, Dimension, Layout as TaffyLayout, NodeId, NodeId as TaffyId, PrintTree,
    Size as TaffySize, Style as TaffyStyle, TaffyTree,
    prelude::{TaffyMaxContent, length},
};
use vello_cpu::{RenderContext, kurbo::Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
