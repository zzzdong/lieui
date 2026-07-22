//! Window 示例 — 窗口化 GUI（winit + Application）

use lieui::geometry::{Color, Size};
use lieui::prelude::*;
use lieui::state::State;
use lieui::view::View;

fn main() {
    let count = State::new(0);

    let app = Application::new(
        move || {
            Column::new()
                .child(Text::new("LieUI v2 — Click a button!").font_size(18.0))
                .child(Text::new(&format!("Count: {}", count.get())).font_size(32.0).color(Color::RED))
                .child(Button::new("Increment").on_click({
                    let c = count.clone();
                    move || c.update(|v| *v += 1)
                }))
                .child(Text::new("(click the button to increment)").font_size(12.0))
                .build()
        },
        Size::new(600.0, 400.0),
    );

    app.run();
}
