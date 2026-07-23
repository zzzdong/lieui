//! Window 示例 — 诊断模式
//! 先画一大块红色矩形确认像素输出正常

use lieui::geometry::{Color, Size};
use lieui::layout::flex::AlignItems;
use lieui::layout::flex::JustifyContent;
use lieui::prelude::*;
use lieui::state::State;
use lieui::view::View;

fn main() {
    let count = State::new(0);
    let clicked = State::new(false);
    let c = count.clone();
    let ck = clicked.clone();

    let app = Application::new(
        move || {
            Row::new()
                .expand(true)
                .justify_content(JustifyContent::Center)
                .align_items(AlignItems::Center)
                .child(
                    Column::new()
                        .spacing(12.0)
                        .expand(true)
                        .justify_content(JustifyContent::Center)
                        .align_items(AlignItems::Center)
                        .child(
                            Text::new(format!("Count: {}", count.get()))
                                .font_size(24.0)
                                .color(Color::WHITE),
                        )
                        .child(Button::new("Click Me").on_click({
                            let c = c.clone();
                            let ck = ck.clone();
                            move || {
                                c.update(|v| *v += 1);
                                ck.set(true);
                            }
                        }))
                        .child(if *clicked.get() {
                            Container::new().child(
                                Text::new("Clicked!").font_size(18.0).color(Color::RED),
                            )
                        } else {
                            Container::new()
                        })
                        .child(
                            Text::new(format!("{}", count.get()))
                                .font_size(48.0)
                                .color(Color::new(255, 200, 0)),
                        ),
                )
                .build()
        },
        Size::new(600.0, 400.0),
    );

    app.run();
}
