use lieui::{application::Application, world::View};
use winit::{dpi::PhysicalSize, window::WindowAttributes};

fn main() {
    let state = String::new();

    let view = View::new();

    let attrs = WindowAttributes::default().with_inner_size(PhysicalSize::new(800, 600));

    let mut app = Application::new(view, attrs, state);

    app.run();
}
