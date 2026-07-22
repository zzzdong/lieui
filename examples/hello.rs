//! Hello v2 — 窗口版，诊断 UI 元素渲染

use lieui::geometry::{Color, Size};
use lieui::prelude::*;
use lieui::state::State;
use lieui::view::View;

fn main() {
    let count = State::new(0);

    let app = Application::new(
        move || {
            Column::new()
                .child(Text::new("Hello LieUI v2!").font_size(32.0).color(Color::RED))
                .child(Text::new(&format!("Count: {}", count.get())).font_size(24.0))
                .child(Button::new("+1").on_click({
                    let c = count.clone();
                    move || c.update(|v| *v += 1)
                }))
                .child(Text::new("(click button)").font_size(14.0))
                .build()
        },
        Size::new(600.0, 400.0),
    );

    app.run();
}
