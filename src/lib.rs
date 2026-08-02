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
pub mod window;

pub mod prelude {
    // ── app / window ──
    pub use crate::app::{Application, CloseAction};
    pub use crate::core::ElementId;
    pub use crate::render::visual::{LayeredElement, VisualElement};
    pub use crate::render::{Renderer, VelloRenderer};
    pub use crate::runtime::Runtime;
    pub use crate::window::WindowConfig;

    // ── state ──
    pub use crate::core::layers::{Anchor, FocusPolicy, LayerKind, LayerOptions};
    pub use crate::state::{
        LayerSpec, State, hide_modal, hide_overlay, request_rebuild, request_redraw,
        request_window_close, show_layer, show_modal, show_overlay,
    };

    // ── geometry ──
    pub use crate::geometry::{Color, Point, Rect, Size};

    // ── flex / layout ──
    pub use crate::layout::{FlexAlign, FlexDirection, FlexWrap};

    // ── theme ──
    pub use crate::theme::{Theme, current, set};

    // ── widgets ──
    pub use crate::widget::{
        Button, Card, Checkbox, Column, Container, Divider, Draggable, Icon, IconButton,
        IconButtonVariant, IconName, Image, Input, LayoutAttr, Progress, Radio, Row, ScrollView,
        Slider, Switch, Tab, Text, Tooltip, VirtualList,
    };
}
