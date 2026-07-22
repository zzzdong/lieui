pub mod app;
pub mod core;
pub mod event;
pub mod geometry;
pub mod layout;
pub mod render;
pub mod runtime;
pub mod state;
pub mod text;
pub mod view;

pub mod prelude {
    pub use crate::app::Application;
    pub use crate::core::ElementId;
    pub use crate::geometry::{Color, Point, Rect, Size};
    pub use crate::render::visual::{LayeredElement, VisualElement};
    pub use crate::render::{Renderer, VelloRenderer};
    pub use crate::runtime::Runtime;
    pub use crate::state::{
        hide_modal, hide_overlay, request_rebuild, request_redraw, show_modal, show_overlay, State,
    };
    pub use crate::view::primitives::{
        Button, Checkbox, Column, Container, Divider, Image, Row, Text,
    };
    pub use crate::view::{View, ViewNode};
}
