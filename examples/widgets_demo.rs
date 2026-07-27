//! widgets_demo — 演示新增/优化的控件与能力
//!
//! 运行：`cargo run --example widgets_demo`
//!
//! 覆盖：自定义字体加载、图片文件加载与 fit、Progress、Slider、Switch、
//! Radio、Tooltip，以及等高分项的虚拟滚动列表。

use lieui::geometry::Color;
use lieui::layout::types::FlexAlign;
use lieui::prelude::*;
use lieui::state::State;
use lieui::text::register_font_file;
use lieui::theme;
use lieui::view::paint::ImageFit;
use lieui::widget::Widget;

fn card(children: impl Widget + 'static) -> impl Widget {
    Container::new()
        .padding(16.0)
        .border_radius(6.0)
        .background(theme::current().background.secondary_default)
        .child(children)
}

fn title(text: &str) -> impl Widget {
    Text::new(text)
        .font_size(18.0)
        .font_weight(600)
        .color(theme::current().text.brand_default)
}

/// 生成一张 64x64 的棋盘格 RGBA 图，用于演示图片控件（无需外部文件）。
fn checkerboard() -> Vec<u8> {
    let n = 64;
    let mut data = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let on = ((x / 8) + (y / 8)) % 2 == 0;
            let (r, g, b) = if on { (80, 140, 230) } else { (230, 120, 80) };
            let i = (y * n + x) * 4;
            data[i] = r;
            data[i + 1] = g;
            data[i + 2] = b;
            data[i + 3] = 255;
        }
    }
    data
}

fn main() {
    let slider_val = State::new(0.5f32);
    let checked = State::new(false);
    let radio_val = State::new(0usize);
    let scroll_y = State::new((0f32, 0f32));

    // 注册一个自定义字体（若文件存在则可用 family 名 "Roboto"）。
    let _ = register_font_file("assets/Roboto-Regular.ttf");

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
                                .child(
                                    Column::new()
                                        .spacing(4.0)
                                        .align_items(FlexAlign::Center)
                                        .child(Text::new("LieUI Widgets Demo").font_size(32.0))
                                        .child(
                                            Text::new("New & optimized widgets")
                                                .font_size(14.0)
                                                .color(theme::current().text.subtle_default),
                                        ),
                                )
                                // 自定义字体
                                .child(card(
                                    Column::new()
                                        .spacing(8.0)
                                        .align_items(FlexAlign::Start)
                                        .child(title("Custom Font"))
                                        .child(
                                            Text::new("This text uses the loaded Roboto font")
                                                .font_size(16.0)
                                                .font_family("Roboto"),
                                        ),
                                ))
                                // Progress + Slider
                                .child(card(
                                    Column::new()
                                        .spacing(12.0)
                                        .align_items(FlexAlign::Stretch)
                                        .child(title("Progress & Slider"))
                                        .child(Progress::new(*slider_val.get() as f64))
                                        .child(Slider::new(slider_val.clone()).track_height(8.0))
                                        .child(
                                            Text::new(format!("Value: {:.2}", *slider_val.get()))
                                                .font_size(14.0),
                                        ),
                                ))
                                // Switch + Radio
                                .child(card(
                                    Column::new()
                                        .spacing(12.0)
                                        .align_items(FlexAlign::Start)
                                        .child(title("Switch & Radio"))
                                        .child(Switch::new(checked.clone()))
                                        .child(
                                            Text::new(format!(
                                                "Switch is {}",
                                                if *checked.get() { "ON" } else { "OFF" }
                                            ))
                                            .font_size(14.0),
                                        )
                                        .child(
                                            Radio::new(radio_val.clone())
                                                .option("Apple")
                                                .option("Banana")
                                                .option("Cherry"),
                                        )
                                        .child(
                                            Text::new(format!("Selected: {}", *radio_val.get()))
                                                .font_size(14.0),
                                        ),
                                ))
                                // Tooltip
                                .child(card(
                                    Column::new()
                                        .spacing(12.0)
                                        .align_items(FlexAlign::Start)
                                        .child(title("Tooltip"))
                                        .child(Tooltip::new(
                                            Box::new(
                                                Text::new("Hover over me")
                                                    .font_size(14.0)
                                                    .color(theme::current().text.brand_default),
                                            ),
                                            "Tooltip text shown on hover",
                                        )),
                                ))
                                // Image + fit
                                .child(card(
                                    Column::new()
                                        .spacing(12.0)
                                        .align_items(FlexAlign::Start)
                                        .child(title("Image (fit=Contain)"))
                                        .child(
                                            Container::new()
                                                .width(120.0)
                                                .height(120.0)
                                                .background(Color::from_hex("#101418"))
                                                .child(
                                                    Image::from_rgba(checkerboard(), 64, 64)
                                                        .width(120.0)
                                                        .height(120.0)
                                                        .fit(ImageFit::Contain)
                                                        .radius(8.0),
                                                ),
                                        ),
                                ))
                                // Virtual List
                                .child(card(
                                    Column::new()
                                        .spacing(12.0)
                                        .align_items(FlexAlign::Stretch)
                                        .child(title("Virtual List (1000 rows)"))
                                        .child(
                                            VirtualList::new(240.0, 1000, 36.0, scroll_y.clone())
                                                .overscan(4)
                                                .item(|i| {
                                                    Box::new(
                                                        Container::new().padding(8.0).child(
                                                            Text::new(format!("Row item #{i}")),
                                                        ),
                                                    )
                                                }),
                                        ),
                                )),
                        ),
                    ),
            )
        },
        Size::new(900.0, 700.0),
    )
    .with_font("assets/Roboto-Regular.ttf");

    app.run();
}
