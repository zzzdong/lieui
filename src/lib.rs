pub mod app;
pub mod builder;
pub mod core;
pub mod event;
pub mod geometry;
pub mod layout;
pub mod render;
pub mod state;
pub mod text;
pub mod widget;
pub mod widgets;

pub mod prelude {
    pub use crate::app::App;
    pub use crate::builder::{BuildContext, BuildSnapshot};
    pub use crate::core::{ViewContext, WidgetId};
    pub use crate::event::{Event, EventType, MouseButton};
    pub use crate::geometry::{Color, Point, Rect, Size};
    pub use crate::layout::LayoutConstraint;
    pub use crate::state::State;
    pub use crate::widget::Widget;
    pub use crate::widgets::{
        Button, Checkbox, Column, Container, Divider, Image, ProgressBar, Row, Slider, Switch, Text,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_widget() {
        let mut ctx = core::ViewContext::new(geometry::Size::new(800.0, 600.0));
        let text = widgets::Text::new("Hello");
        let id = ctx.create(text);
        assert!(ctx.get::<widgets::Text>(id).is_some());
    }
}
