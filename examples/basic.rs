use lieui::{
    application::Application,
    element::{DivElement, TextElement},
    event::world::EventResult,
    world::View,
};
use winit::{dpi::PhysicalSize, window::WindowAttributes};

fn main() {
    let state: i32 = 0;

    let root = DivElement::new()
        .with_width(taffy::Dimension::percent(1.0))
        .with_height(taffy::Dimension::percent(1.0))
        .with_background_color(color::palette::css::WHITE)
        .with_display(taffy::Display::Flex)
        .with_align_items(taffy::AlignItems::Center)
        .with_justify_content(taffy::JustifyContent::Center);

    let mut view = View::new();

    let root = view.add_root(root);

    for i in 0..1 {
        let child = DivElement::new()
            .with_width(taffy::Dimension::length(80.0))
            .with_height(taffy::Dimension::length(48.0))
            .with_background_color(color::palette::css::SKY_BLUE)
            .with_display(taffy::Display::Flex)
            .with_align_items(taffy::AlignItems::Center)
            .with_justify_content(taffy::JustifyContent::Center);
        let button = view.add_child(root, child);

        let text = TextElement::new("Hello".to_string());
        let text = view.add_child(button, text);

        view.handle_pointer_pressed(button, move |event, state| {
            println!("counter{i}: {:?}", *state);
            *state += 1;
            EventResult::new()
        });
    }

    let attrs = WindowAttributes::default().with_inner_size(PhysicalSize::new(800, 600));

    let mut app = Application::new(view, attrs, state);

    app.run();
}
