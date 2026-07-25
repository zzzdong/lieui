//! Hello — 展示 Column/Row Flex 居中 + 按钮交互

use lieui::geometry::{Color, Size};
use lieui::layout::types::FlexAlign;
use lieui::prelude::*;
use lieui::state::State;

fn main() {
    let count = State::new(0);

    let app = Application::new(
        move |_ctx| {
            // Row 撑满视口宽度，Column 撑满高度，各自居中子节点
            Box::new(
                Row::new()
                    .expand(true)
                    .justify_content(FlexAlign::Center)
                    .align_items(FlexAlign::Center)
                    .child(
                        Column::new()
                            .spacing(16.0)
                            .expand(true)
                            .justify_content(FlexAlign::Center)
                            .align_items(FlexAlign::Center)
                            .child(Text::new("LieUI v2").font_size(48.0).color(Color::RED))
                            .child(Text::new(format!("Count: {}", count.get())).font_size(32.0))
                            .child(
                                Row::new()
                                    .spacing(12.0)
                                    .align_items(FlexAlign::Center)
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
                                    })),
                            )
                            .child(
                                Text::new("(click buttons)")
                                    .font_size(14.0)
                                    .color(Color::new(128, 128, 128)),
                            ),
                    ),
            )
        },
        Size::new(600.0, 400.0),
    );

    app.run();
}
