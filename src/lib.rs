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
    pub use crate::event::{Event, EventType, MouseButton, Key};
    pub use crate::geometry::{Color, Point, Rect, Size};
    pub use crate::layout::{LayoutConstraint, BoxStyle};
    pub use crate::render::visual::{LayeredElement, VisualElement};
    pub use crate::render::{Renderer, VelloRenderer};
    pub use crate::runtime::Runtime;
    pub use crate::state::State;
    pub use crate::view::node::PropMap;
    pub use crate::view::primitives::{Button, Checkbox, Column, Container, Divider, Image, Row, Text};
    pub use crate::view::{KeyedView, View, ViewExt, ViewNode};
}
