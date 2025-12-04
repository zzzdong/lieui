use lieui::{
    application::Application,
    element::{DivElement, TextElement},
    event::world::EventResult,
    world::View,
};
use winit::{dpi::PhysicalSize, window::WindowAttributes};

fn main() {
    let state: i32 = 0;

    let view = View::builder().build();

    let attrs = WindowAttributes::default().with_inner_size(PhysicalSize::new(800, 600));

    let mut app = Application::new(view, attrs, state);

    app.run();
}
