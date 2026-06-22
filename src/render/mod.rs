// src/render/mod.rs

pub mod engine;
pub mod renderer;
pub mod visual;

pub use engine::VelloRenderer;
pub use renderer::Renderer;
pub use visual::{
    BoxShadowDef, FillStrokeStyle, GradientDef, Stroke, StrokeStyle, Transform, VisualElement,
};
