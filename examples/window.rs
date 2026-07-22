//! Window 示例 — 诊断模式
//! 先画一大块红色矩形确认像素输出正常

use lieui::geometry::{Color, Size};
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
            let mut col = Column::new();

            // 诊断：直接画两个色块确认 pixmap 输出正常
            // 方法：利用 Container 的背景色 + 固定宽高
            col = col.child(
                Container::new().child(
                    Text::new(format!("Count: {}", count.get()))
                        .font_size(24.0)
                        .color(Color::WHITE),
                ),
            );

            col = col.child(Button::new("Click Me").on_click({
                let c = c.clone();
                let ck = ck.clone();
                move || {
                    c.update(|v| *v += 1);
                    ck.set(true);
                }
            }));

            if *clicked.get() {
                col = col.child(Text::new("Clicked!").font_size(18.0).color(Color::RED));
            }

            col = col.child(
                Text::new(format!("{}", count.get()))
                    .font_size(48.0)
                    .color(Color::new(255, 200, 0)),
            );

            col.build()
        },
        Size::new(600.0, 400.0),
    );

    app.run();
}
