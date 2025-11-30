use lieui::{application::Application, element::{DivElement, TextElement}, world::View};
use winit::{dpi::PhysicalSize, window::WindowAttributes};

fn main() {
    let state: i32 = 0;

    let root = DivElement::new()
        .with_width(100.0)
        .with_height(100.0)
        .with_background_color(color::palette::css::WHITE);

    let mut view = View::new();

    let root = view.add_root(root);

    let child = DivElement::new()
        .with_width(80.0)
        .with_height(32.0)
        .with_background_color(color::palette::css::SKY_BLUE);

    let button = view.add_child(root, child);

    let text = TextElement::new("Hello".to_string());
    let text = view.add_child(button, text);

    view.handle_pointer_pressed(button, |event, state| {
        println!("counter: {:?}", *state);
        *state += 1;
    });

    let attrs = WindowAttributes::default().with_inner_size(PhysicalSize::new(800, 600));

    let mut app = Application::new(view, attrs, state);

    app.run();
}
