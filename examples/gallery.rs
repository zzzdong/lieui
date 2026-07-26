//! Gallery — 展示 LieUI 内置 Widget
//!
//! 包含 Button、Checkbox、Input（单行/多行）、Text、Divider、Container/Flex 布局。

use lieui::geometry::{Color, Size};
use lieui::layout::types::FlexAlign;
use lieui::prelude::*;
use lieui::state::State;
use lieui::theme;
use lieui::widget::Widget;

fn section_title(title: &str) -> impl Widget {
    Text::new(title)
        .font_size(18.0)
        .font_weight(600)
        .color(theme::current().text.brand_default)
}

fn card(children: impl Widget + 'static) -> impl Widget {
    Container::new()
        .padding(16.0)
        .border_radius(6.0)
        .background(theme::current().background.secondary_default)
        .child(children)
}

fn main() {
    let count = State::new(0i32);
    let checked = State::new(false);
    let input_text = State::new("".to_string());
    let multi_text = State::new("第一行\n第二行".to_string());

    let app = Application::new(
        move |_ctx| {
            Box::new(
                Row::new()
                    .expand(true)
                    .justify_content(FlexAlign::Center)
                    .align_items(FlexAlign::Start)
                    .child(
                        Container::new().width(560.0).padding(24.0).child(
                            Column::new()
                                .spacing(24.0)
                                .align_items(FlexAlign::Stretch)
                                // Header
                                .child(
                                    Column::new()
                                        .spacing(4.0)
                                        .align_items(FlexAlign::Center)
                                        .child(Text::new("LieUI Gallery").font_size(36.0))
                                        .child(
                                            Text::new("Built-in widgets preview")
                                                .font_size(14.0)
                                                .color(theme::current().text.subtle_default),
                                        ),
                                )
                                // Button
                                .child(card(
                                    Column::new()
                                        .spacing(12.0)
                                        .align_items(FlexAlign::Start)
                                        .child(section_title("Button"))
                                        .child(
                                            Row::new()
                                                .spacing(12.0)
                                                .align_items(FlexAlign::Center)
                                                .child(Button::new("Primary"))
                                                .child(Button::new("Counter").on_click({
                                                    let c = count.clone();
                                                    move || c.update(|v| *v += 1)
                                                }))
                                                .child(
                                                    Text::new(format!("Clicked: {}", count.get()))
                                                        .font_size(14.0),
                                                ),
                                        ),
                                ))
                                // Checkbox
                                .child(card(
                                    Column::new()
                                        .spacing(12.0)
                                        .align_items(FlexAlign::Start)
                                        .child(section_title("Checkbox"))
                                        .child(
                                            Checkbox::new(*checked.get())
                                                .label(format!(
                                                    "Agree to terms ({}",
                                                    if *checked.get() {
                                                        "checked"
                                                    } else {
                                                        "unchecked"
                                                    }
                                                ))
                                                .on_click({
                                                    let c = checked.clone();
                                                    move || c.update(|v| *v = !*v)
                                                }),
                                        )
                                        .child(
                                            Checkbox::new(true)
                                                .label("Disabled-like checked box")
                                                .on_click(|| {}),
                                        ),
                                ))
                                // Input
                                .child(card(
                                    Column::new()
                                        .spacing(12.0)
                                        .align_items(FlexAlign::Start)
                                        .child(section_title("Input"))
                                        .child(
                                            Input::new("Type here...")
                                                .width(300.0)
                                                .on_change({
                                                    let s = input_text.clone();
                                                    move |t| s.set(t)
                                                })
                                                .on_submit({
                                                    let s = input_text.clone();
                                                    move |t| s.set(format!("submitted: {}", t))
                                                }),
                                        )
                                        .child(
                                            Text::new(format!("Value: {}", input_text.get()))
                                                .font_size(13.0)
                                                .color(theme::current().text.subtle_default),
                                        )
                                        .child(
                                            Input::new("Multi-line input...")
                                                .width(400.0)
                                                .height(100.0)
                                                .multiline(true)
                                                .on_change({
                                                    let s = multi_text.clone();
                                                    move |t| s.set(t)
                                                }),
                                        ),
                                ))
                                // Text & Divider
                                .child(card(
                                    Column::new()
                                        .spacing(12.0)
                                        .align_items(FlexAlign::Start)
                                        .child(section_title("Text & Divider"))
                                        .child(
                                            Text::new(
                                                "This is a long wrapped text demo. ".to_string()
                                                    + "It demonstrates how Text widget handles "
                                                    + "multi-line content within a fixed width.",
                                            )
                                            .font_size(14.0)
                                            .max_width(480.0),
                                        )
                                        .child(Divider::new())
                                        .child(
                                            Text::new("Colored & aligned text")
                                                .color(Color::new(0, 128, 0))
                                                .font_size(14.0),
                                        ),
                                ))
                                // Layout
                                .child(card(
                                    Column::new()
                                        .spacing(12.0)
                                        .align_items(FlexAlign::Start)
                                        .child(section_title("Flex Layout"))
                                        .child(
                                            Row::new()
                                                .spacing(8.0)
                                                .align_items(FlexAlign::Center)
                                                .child(
                                                    Container::new()
                                                        .width(60.0)
                                                        .height(60.0)
                                                        .background(Color::new(255, 100, 100))
                                                        .border_radius(4.0),
                                                )
                                                .child(
                                                    Container::new()
                                                        .width(60.0)
                                                        .height(60.0)
                                                        .background(Color::new(100, 255, 100))
                                                        .border_radius(4.0),
                                                )
                                                .child(
                                                    Container::new()
                                                        .width(60.0)
                                                        .height(60.0)
                                                        .background(Color::new(100, 100, 255))
                                                        .border_radius(4.0),
                                                ),
                                        )
                                        .child(
                                            Row::new()
                                                .spacing(8.0)
                                                .justify_content(FlexAlign::SpaceBetween)
                                                .child(
                                                    Container::new()
                                                        .width(80.0)
                                                        .height(32.0)
                                                        .background(theme::current().border.strong)
                                                        .border_radius(4.0),
                                                )
                                                .child(
                                                    Container::new()
                                                        .width(80.0)
                                                        .height(32.0)
                                                        .background(theme::current().border.strong)
                                                        .border_radius(4.0),
                                                )
                                                .child(
                                                    Container::new()
                                                        .width(80.0)
                                                        .height(32.0)
                                                        .background(theme::current().border.strong)
                                                        .border_radius(4.0),
                                                ),
                                        ),
                                ))
                                .child(
                                    Text::new("Tip: try Ctrl+A/C/V/X in the inputs above.")
                                        .font_size(12.0)
                                        .color(theme::current().text.subtle_default),
                                ),
                        ),
                    ),
            )
        },
        Size::new(640.0, 900.0),
    );

    app.run();
}
