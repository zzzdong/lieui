//! Hello — 展示 Column + Button + 文字 + Flex 布局

use lieui::geometry::{Color, Size};
use lieui::layout::flex::{AlignItems, JustifyContent};
use lieui::prelude::*;
use lieui::state::State;
use lieui::view::View;

fn main() {
    let count = State::new(0);

    let app = Application::new(
        move || {
            Column::new()
                .spacing(12.0)
                .justify_content(JustifyContent::Center)
                .align_items(AlignItems::Center)
                .child(Text::new("LieUI v2").font_size(48.0).color(Color::RED))
                .child(Text::new(&format!("Count: {}", count.get())).font_size(32.0))
                .child(
                    Row::new().spacing(16.0).align_items(AlignItems::Center)
                        .child(Button::new("-1").on_click({
                            let c = count.clone();
                            move || c.update(|v| *v -= 1)
                        }))
                        .child(Button::new("Reset").on_click({
                            let c = count.clone();
                            move || c.set(0)
                        }))
                        .child(Button::new("+1").on_click({
                            let c = count.clone();
                            move || c.update(|v| *v += 1)
                        }))
                )
                .child(Text::new("(click buttons to change count)").font_size(14.0).color(Color::new(128, 128, 128)))
                .build()
        },
        Size::new(600.0, 400.0),
    );

    app.run();
}
