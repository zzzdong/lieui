use lieui::core::ViewContext;
use lieui::geometry::Size;
use lieui::prelude::Color;
use lieui::widgets::Container;

fn main() {
    // Create a simple hello world app
    let mut ctx = ViewContext::new(Size::new(800.0, 600.0));

    let root =
        ctx.create(Container::new().background(Color::from_hex("#FFFFFF").unwrap_or(Color::BLACK)));
    ctx.set_root(root);

    println!("LieUI Hello Example - Render Tree:");
    ctx.debug_render_tree = true;

    if let Some(render_tree) = ctx.render() {
        println!("{}", render_tree.to_xml(0));
    }
}
