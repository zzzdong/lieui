pub mod animation;
pub mod app;
pub mod clipboard;
pub mod core;
pub mod event;
pub mod geometry;
pub mod layout;
pub mod perf;
pub mod render;
pub mod runtime;
pub mod state;
pub mod text;
pub mod theme;
pub mod view;
pub mod widget;

pub mod prelude {
    // ── app ──
    pub use crate::app::set_window_size;
    pub use crate::app::window_size;
    pub use crate::app::Application;
    pub use crate::core::ElementId;
    pub use crate::render::visual::{LayeredElement, VisualElement};
    pub use crate::render::{Renderer, VelloRenderer};
    pub use crate::runtime::Runtime;

    // ── state ──
    pub use crate::state::{
        hide_modal, hide_overlay, request_rebuild, request_redraw, show_modal, show_overlay, State,
    };

    // ── geometry ──
    pub use crate::geometry::{Color, Point, Rect, Size};

    // ── flex / layout ──
    pub use crate::layout::{FlexAlign, FlexDirection, FlexWrap};

    // ── theme ──
    pub use crate::theme::{current, set, Theme};

    // ── widgets ──
    pub use crate::widget::{
        Button, Card, Checkbox, Column, Container, Divider, Image, Input, Progress, Radio, Row,
        ScrollView, Slider, Switch, Tab, Text, Tooltip, VirtualList,
    };
}
