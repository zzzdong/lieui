//! Gallery — 展示 LieUI 内置 Widget
//!
//! 以顶层 `Tab` 分页组织：基础控件 / 布局 / 交互控件 / 容器组件 / 虚拟列表 / 关于，
//! 涵盖 Button、Checkbox、Input、Text、Divider、Flex 布局、Image（fit 模式），
//! 以及 Slider、Progress、Switch、Radio、Tooltip、Tab、Draggable、Icon/IconButton、
//! Card、VirtualList、ScrollView 等控件。
//!
//! 构建方式：先按 Tab 拆出 `page_*` 页面函数，每个页面内再按 Section 拆出
//! `section_*` 区块函数（每个区块用 `card()` 包成一个卡片），避免单一巨型链式调用。

use lieui::event::Event;
use lieui::geometry::Color;
use lieui::layout::types::FlexAlign;
use lieui::prelude::*;
use lieui::state::{State, request_rebuild};
use lieui::theme::{self, Mode, set_mode};
use lieui::view::paint::ImageFit;
use lieui::widget::menu::{ContextMenu, Menu, MenuButton, MenuItem, Submenu};
use lieui::widget::{
    ButtonSize, ButtonVariant, CardVariant, InputSize, InputStatus, ScrollBar, Widget,
};

/// 区块标题。
fn section_title(title: &str) -> impl Widget {
    Text::new(title)
        .font_size(18.0)
        .font_weight(600)
        .color(theme::current().text.brand_default)
}

/// 把任意内容包成一张卡片（白底 + 1px 边框 + 投影 + 圆角，对齐 PatternFly Card）。
fn card(children: impl Widget + 'static) -> impl Widget {
    let t = theme::current();
    Container::new()
        .layout(LayoutAttr::new().padding(t.spacer.md))
        .border_radius(t.radius.medium)
        .background(t.background.primary_default)
        .border(1.0, t.border.default)
        .shadow(t.shadow.sm)
        .child(children)
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

/// 一个带标签的 fit 模式演示框。
fn fit_box(label: &str, fit: ImageFit) -> impl Widget {
    Column::new()
        .spacing(6.0)
        .align_items(FlexAlign::Start)
        .child(
            Text::new(label)
                .font_size(12.0)
                .color(theme::current().text.subtle_default),
        )
        .child(
            Container::new()
                .width(120.0)
                .height(120.0)
                .background(Color::from_hex("#101418"))
                .border_radius(6.0)
                .child(
                    Image::from_rgba(checkerboard(), 64, 64)
                        .width(120.0)
                        .height(120.0)
                        .fit(fit)
                        .radius(6.0),
                ),
        )
}

// ───────────────────────────── 基础控件 ─────────────────────────────

fn section_button(count: State<i32>) -> impl Widget {
    card(
        Column::new()
            .spacing(12.0)
            .align_items(FlexAlign::Start)
            .child(section_title("Button · 变体（variant）"))
            .child(
                Row::new()
                    .spacing(12.0)
                    .align_items(FlexAlign::Center)
                    .child(Button::new("Primary").variant(ButtonVariant::Primary))
                    .child(Button::new("Secondary").variant(ButtonVariant::Secondary))
                    .child(Button::new("Tertiary").variant(ButtonVariant::Tertiary)),
            )
            .child(
                Row::new()
                    .spacing(12.0)
                    .align_items(FlexAlign::Center)
                    .child(Button::new("Danger").variant(ButtonVariant::Danger))
                    .child(Button::new("Warning").variant(ButtonVariant::Warning))
                    .child(Button::new("Link").variant(ButtonVariant::Link))
                    .child(Button::new("Plain").variant(ButtonVariant::Plain)),
            )
            .child(section_title("Button · 尺寸（size）"))
            .child(
                Row::new()
                    .spacing(12.0)
                    .align_items(FlexAlign::Center)
                    .child(
                        Button::new("Small")
                            .variant(ButtonVariant::Secondary)
                            .size(ButtonSize::Sm),
                    )
                    .child(
                        Button::new("Medium")
                            .variant(ButtonVariant::Secondary)
                            .size(ButtonSize::Md),
                    )
                    .child(
                        Button::new("Large")
                            .variant(ButtonVariant::Secondary)
                            .size(ButtonSize::Lg),
                    ),
            )
            .child(section_title("Button · 交互"))
            .child(
                Row::new()
                    .spacing(12.0)
                    .align_items(FlexAlign::Center)
                    .child(
                        Button::new("Counter")
                            .variant(ButtonVariant::Primary)
                            .on_click({
                                let c = count.clone();
                                move || c.update(|v| *v += 1)
                            }),
                    )
                    .child(Text::new(format!("Clicked: {}", count.get())).font_size(14.0)),
            ),
    )
}

fn section_checkbox(checked: State<bool>) -> impl Widget {
    card(
        Column::new()
            .spacing(12.0)
            .align_items(FlexAlign::Start)
            .child(section_title("Checkbox"))
            .child(
                Checkbox::new(*checked.get())
                    .label(format!(
                        "Agree to terms ({})",
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
    )
}

fn section_input(input_text: State<String>, multi_text: State<String>) -> impl Widget {
    let t = theme::current();
    card(
        Column::new()
            .spacing(12.0)
            .align_items(FlexAlign::Start)
            .child(section_title("Input"))
            .child(
                Input::new("Type here...")
                    .width(300.0)
                    .on_change({
                        let s = input_text.clone();
                        move |txt| s.set(txt)
                    })
                    .on_submit({
                        let s = input_text.clone();
                        move |txt| s.set(format!("submitted: {}", txt))
                    }),
            )
            .child(
                Text::new(format!("Value: {}", input_text.get()))
                    .font_size(13.0)
                    .color(t.text.subtle_default),
            )
            .child(
                Input::new("Multi-line input...")
                    .width(400.0)
                    .height(100.0)
                    .multiline(true)
                    .on_change({
                        let s = multi_text.clone();
                        move |txt| s.set(txt)
                    }),
            )
            .child(section_title("Input · 校验状态（status）"))
            .child(
                Row::new()
                    .spacing(12.0)
                    .align_items(FlexAlign::Center)
                    .child(
                        Input::new("错误示例")
                            .width(180.0)
                            .status(InputStatus::Danger),
                    )
                    .child(
                        Input::new("成功示例")
                            .width(180.0)
                            .status(InputStatus::Success),
                    )
                    .child(
                        Input::new("警告示例")
                            .width(180.0)
                            .status(InputStatus::Warning),
                    ),
            )
            .child(section_title("Input · 尺寸（size）"))
            .child(
                Row::new()
                    .spacing(12.0)
                    .align_items(FlexAlign::Center)
                    .child(Input::new("Small").width(160.0).size(InputSize::Sm))
                    .child(Input::new("Medium").width(160.0).size(InputSize::Md))
                    .child(Input::new("Large").width(160.0).size(InputSize::Lg)),
            ),
    )
}

fn section_text_divider() -> impl Widget {
    card(
        Column::new()
            .spacing(12.0)
            .align_items(FlexAlign::Start)
            .child(section_title("Text & Divider"))
            .child(
                Text::new(
                    "This is a long wrapped text demo. ".to_string()
                        + "It demonstrates how Text widget "
                        + "handles multi-line content within "
                        + "a fixed width.",
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
    )
}

fn section_image() -> impl Widget {
    card(
        Column::new()
            .spacing(12.0)
            .align_items(FlexAlign::Start)
            .child(section_title("Image · 图片与 fit"))
            .child(
                Text::new("64x64 棋盘格演示图，在 120x120 深色容器内展示四种 fit 模式。")
                    .font_size(12.0)
                    .color(theme::current().text.subtle_default),
            )
            .child(
                Row::new()
                    .spacing(16.0)
                    .align_items(FlexAlign::Start)
                    .child(fit_box("Contain", ImageFit::Contain))
                    .child(fit_box("Cover", ImageFit::Cover))
                    .child(fit_box("Fill", ImageFit::Fill))
                    .child(fit_box("None", ImageFit::None)),
            ),
    )
}

fn page_basic(
    count: State<i32>,
    checked: State<bool>,
    input_text: State<String>,
    multi_text: State<String>,
) -> impl Widget {
    ScrollView::expand().scrollbar(true).child(
        Column::new()
            .spacing(16.0)
            .align_items(FlexAlign::Stretch)
            .child(section_button(count))
            .child(section_checkbox(checked))
            .child(section_input(input_text, multi_text))
            .child(section_text_divider())
            .child(section_image()),
    )
}

// ───────────────────────────── 布局 ─────────────────────────────

fn page_layout() -> impl Widget {
    ScrollView::expand().scrollbar(true).child(card(
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
}

// ───────────────────────────── 交互控件 ─────────────────────────────

fn section_slider_progress(slider_val: State<f32>) -> impl Widget {
    card(
        Column::new()
            .spacing(12.0)
            .align_items(FlexAlign::Start)
            .child(section_title("Slider & Progress"))
            .child(Text::new("拖动滑块，进度条同步联动："))
            .child(Slider::new(slider_val.clone()))
            .child(Progress::new(*slider_val.get() as f64)),
    )
}

fn section_switch_radio(switch_val: State<bool>, radio_val: State<usize>) -> impl Widget {
    card(
        Column::new()
            .spacing(12.0)
            .align_items(FlexAlign::Start)
            .child(section_title("Switch & Radio"))
            .child(
                Row::new()
                    .spacing(16.0)
                    .align_items(FlexAlign::Center)
                    .child(Switch::new(switch_val.clone()).label("启用通知"))
                    .child(Text::new(if *switch_val.get() {
                        "已开启"
                    } else {
                        "已关闭"
                    })),
            )
            .child(
                Radio::new(radio_val.clone())
                    .option("选项 A")
                    .option("选项 B")
                    .option("选项 C"),
            ),
    )
}

fn section_tooltip() -> impl Widget {
    card(
        Column::new()
            .spacing(12.0)
            .align_items(FlexAlign::Start)
            .child(section_title("Tooltip"))
            .child(Tooltip::new(
                Box::new(Text::new("悬停我查看提示")),
                "这是一个 Tooltip 工具提示",
            )),
    )
}

fn section_nested_tab(nested: State<usize>) -> impl Widget {
    card(
        Column::new()
            .spacing(12.0)
            .align_items(FlexAlign::Start)
            .child(section_title("Tab (嵌套演示)"))
            .child(
                Tab::new(nested)
                    .header_height(32.0)
                    .tab("一", Text::new("嵌套 Tab 的第一页内容。"))
                    .tab("二", Text::new("嵌套 Tab 的第二页内容。"))
                    .tab("三", Text::new("嵌套 Tab 的第三页内容。")),
            ),
    )
}

fn section_draggable(pos: State<(f32, f32)>, active: State<bool>) -> impl Widget {
    card(
        Column::new()
            .spacing(12.0)
            .align_items(FlexAlign::Start)
            .child(section_title("Draggable · 通用拖拽"))
            .child(
                Text::new("按下卡片并移动超过 3px 触发拖拽；拖拽期间自动捕获鼠标，真实拖拽不会产生 Click。")
                    .font_size(12.0)
                    .color(theme::current().text.subtle_default),
            )
            .child(
                Draggable::new(
                    Container::new()
                        .width(280.0)
                        .height(120.0)
                        .background(theme::current().background.secondary_default)
                        .border(1.0, theme::current().border.default)
                        .border_radius(theme::current().radius.medium)
                        .child(
                            Column::new()
                                .expand(true)
                                .align_items(FlexAlign::Center)
                                .justify_content(FlexAlign::Center)
                                .spacing(6.0)
                                .child(Text::new("按住我拖动").font_size(15.0).font_weight(600))
                                .child(
                                    Text::new(format!(
                                        "offset: ({:.0}, {:.0})",
                                        pos.get().0, pos.get().1
                                    ))
                                    .font_size(12.0)
                                    .color(theme::current().text.subtle_default),
                                ),
                        ),
                )
                .on_drag_start({
                    let a = active.clone();
                    move |_| a.set(true)
                })
                .on_drag_move({
                    let p = pos.clone();
                    move |ctx| {
                        if let Some(Event::DragMove { offset_x, offset_y, .. }) = ctx.event() {
                            p.set((*offset_x, *offset_y));
                        }
                    }
                })
                .on_drag_end({
                    let a = active.clone();
                    move |_| a.set(false)
                }),
            )
            .child(
                Row::new()
                    .spacing(12.0)
                    .align_items(FlexAlign::Center)
                    .child(
                        Text::new(if *active.get() { "● 拖拽中" } else { "○ 空闲" })
                            .font_size(12.0)
                            .color(if *active.get() {
                                theme::current().status.info
                            } else {
                                theme::current().text.subtle_default
                            }),
                    )
                    .child(
                        Button::new("复位")
                            .variant(ButtonVariant::Secondary)
                            .on_click({
                                let p = pos.clone();
                                move || p.set((0.0, 0.0))
                            }),
                    ),
            ),
    )
}

fn section_icon_toolbar(clicks: State<i32>) -> impl Widget {
    card(
        Column::new()
            .spacing(12.0)
            .align_items(FlexAlign::Start)
            .child(section_title("IconButton · 工具栏图标按钮"))
            .child(
                Text::new("基于内置 Material Icons 字体（OFL），hover/pressed 时背景与图标变色。")
                    .font_size(12.0)
                    .color(theme::current().text.subtle_default),
            )
            .child(
                Row::new()
                    .spacing(4.0)
                    .align_items(FlexAlign::Center)
                    .child(IconButton::new(IconName::Add).on_click({
                        let c = clicks.clone();
                        move || c.update(|v| *v += 1)
                    }))
                    .child(IconButton::new(IconName::Remove))
                    .child(IconButton::new(IconName::Edit))
                    .child(IconButton::new(IconName::Delete))
                    .child(IconButton::new(IconName::Search))
                    .child(IconButton::new(IconName::Settings))
                    .child(IconButton::new(IconName::Star))
                    .child(IconButton::new(IconName::Favorite))
                    .child(IconButton::new(IconName::Menu))
                    .child(IconButton::new(IconName::Close).variant(IconButtonVariant::Outline))
                    .child(IconButton::new(IconName::Check).variant(IconButtonVariant::Primary)),
            )
            .child(
                Row::new()
                    .spacing(8.0)
                    .align_items(FlexAlign::Center)
                    .child(IconButton::new(IconName::ArrowUp))
                    .child(IconButton::new(IconName::ArrowDown))
                    .child(IconButton::new(IconName::ArrowBack))
                    .child(IconButton::new(IconName::ArrowForward))
                    .child(IconButton::new(IconName::Home))
                    .child(IconButton::new(IconName::Folder))
                    .child(IconButton::new(IconName::Person))
                    .child(IconButton::new(IconName::Mail))
                    .child(IconButton::new(IconName::Save))
                    .child(IconButton::new(IconName::Refresh))
                    .child(IconButton::new(IconName::Info))
                    .child(IconButton::new(IconName::Lock)),
            )
            .child(
                Row::new()
                    .spacing(4.0)
                    .align_items(FlexAlign::Center)
                    .child(IconButton::new(IconName::RotateLeft))
                    .child(IconButton::new(IconName::RotateRight))
                    .child(IconButton::new(IconName::Rotate90DegreesCcw))
                    .child(IconButton::new(IconName::Rotate90DegreesCw))
                    .child(IconButton::new(IconName::Flip))
                    .child(IconButton::new(IconName::FlipToBack))
                    .child(IconButton::new(IconName::FlipToFront)),
            )
            .child(
                Row::new()
                    .spacing(12.0)
                    .align_items(FlexAlign::Center)
                    .child(Text::new("Icon 字形:").font_size(13.0))
                    .child(Icon::new(IconName::Search, 20.0))
                    .child(Icon::new(IconName::Favorite, 20.0).color(Color::RED))
                    .child(Icon::new(IconName::Star, 20.0).color(Color::from_hex("#f0a000")))
                    .child(Icon::new(IconName::Check, 20.0).color(theme::current().status.success)),
            )
            .child(
                Text::new(format!("点击次数: {}", clicks.get()))
                    .font_size(13.0)
                    .color(theme::current().text.subtle_default),
            ),
    )
}

fn section_menu() -> impl Widget {
    Column::new()
        .spacing(20.0)
        .child(
            Row::new()
                .spacing(16.0)
                .align_items(FlexAlign::Center)
                .child(section_title("下拉菜单"))
                .child(
                    MenuButton::new("文件")
                        .variant(ButtonVariant::Secondary)
                        .menu(|_handle, _ctx| {
                            Menu::new()
                                .item(
                                    MenuItem::new("新建文件")
                                        .on_click(|_c| println!("[menu] 新建文件")),
                                )
                                .item(MenuItem::new("打开…").on_click(|_c| println!("[menu] 打开")))
                                .separator()
                                .item(
                                    MenuItem::new("保存")
                                        .hint("Ctrl+S")
                                        .on_click(|_c| println!("[menu] 保存")),
                                )
                                .item(
                                    MenuItem::new("另存为…")
                                        .on_click(|_c| println!("[menu] 另存为")),
                                )
                                .submenu(
                                    Submenu::new("最近打开")
                                        .item(
                                            MenuItem::new("report.pdf")
                                                .on_click(|_c| println!("[menu] 最近: report.pdf")),
                                        )
                                        .item(
                                            MenuItem::new("budget.xlsx").on_click(|_c| {
                                                println!("[menu] 最近: budget.xlsx")
                                            }),
                                        )
                                        .item(
                                            MenuItem::new("slides.pptx").on_click(|_c| {
                                                println!("[menu] 最近: slides.pptx")
                                            }),
                                        ),
                                )
                                .separator()
                                .item(
                                    MenuItem::new("退出")
                                        .hint("Alt+F4")
                                        .on_click(|_c| println!("[menu] 退出")),
                                )
                        }),
                ),
        )
        .child(
            Text::new("右键点击下面的卡片以弹出上下文菜单")
                .font_size(13.0)
                .color(Color::rgba(0x60, 0x60, 0x60, 255)),
        )
        .child({
            let card = Card::new()
                .variant(CardVariant::Default)
                .padding(20.0)
                .body(Text::new("右键我 →").font_size(15.0));
            ContextMenu::new(card)
                .close_on_leave(true)
                .menu(|_handle, _ctx| {
                    Menu::new()
                        .item(MenuItem::new("复制").on_click(|_ctx| {
                            println!("[ctx] 复制");
                        }))
                        .item(MenuItem::new("粘贴").on_click(|_ctx| {
                            println!("[ctx] 粘贴");
                        }))
                        .separator()
                        .item(MenuItem::new("重命名").on_click(|_ctx| {
                            println!("[ctx] 重命名");
                        }))
                        .item(MenuItem::new("删除").hint("Del").on_click(|_ctx| {
                            println!("[ctx] 删除");
                        }))
                })
        })
}

fn page_interactive(
    slider_val: State<f32>,
    switch_val: State<bool>,
    radio_val: State<usize>,
    nested: State<usize>,
    drag_pos: State<(f32, f32)>,
    drag_active: State<bool>,
    icon_clicks: State<i32>,
) -> impl Widget {
    ScrollView::expand().scrollbar(true).child(
        Column::new()
            .spacing(16.0)
            .align_items(FlexAlign::Stretch)
            .child(section_slider_progress(slider_val))
            .child(section_switch_radio(switch_val, radio_val))
            .child(section_tooltip())
            .child(section_nested_tab(nested))
            .child(section_draggable(drag_pos, drag_active))
            .child(section_icon_toolbar(icon_clicks))
            .child(section_title("菜单"))
            .child(section_menu()),
    )
}

// ───────────────────────────── 容器组件 ─────────────────────────────

fn page_containers(switch_val: State<bool>) -> impl Widget {
    ScrollView::expand().scrollbar(true).child(
        Column::new()
            .spacing(16.0)
            .align_items(FlexAlign::Stretch)
            .child(Text::new(
                "Card 容器：具名插槽 header / body / footer，等价于 Vue 的 slot。",
            ))
            .child(
                Card::new()
                    .title("用户信息")
                    .header(Text::new("副标题：header 插槽内的内容"))
                    .body(
                        Column::new()
                            .spacing(8.0)
                            .align_items(FlexAlign::Start)
                            .child(Text::new("body 插槽：卡片主体，可放任意 widget。"))
                            .child(Switch::new(switch_val).label("卡片内开关")),
                    )
                    .footer(
                        Row::new()
                            .spacing(8.0)
                            .align_items(FlexAlign::Center)
                            .child(Button::new("确定"))
                            .child(Button::new("取消").on_click(|| {})),
                    ),
            )
            .child(
                Text::new("也可以省略插槽：仅给 title + body。")
                    .font_size(12.0)
                    .color(theme::current().text.subtle_default),
            )
            .child(
                Card::new()
                    .title("简洁卡片")
                    .body(Text::new("只有标题与主体。")),
            )
            .child(
                Card::new()
                    .variant(CardVariant::Compact)
                    .title("紧凑卡片（Compact）")
                    .body(Text::new("内边距更小，适合在密集布局中复用。")),
            )
            .child(
                Card::new()
                    .variant(CardVariant::Selectable)
                    .title("可选中卡片（Selectable）")
                    .body(Text::new("hover 时高亮背景，用于可选中的卡片式列表项。")),
            ),
    )
}

// ───────────────────────────── 虚拟列表 / 滚动 ─────────────────────────────

fn section_virtual_list(scroll_y: State<(f32, f32)>) -> impl Widget {
    let total = 1000.0 * 34.0;
    card(
        Column::new()
            .spacing(12.0)
            .align_items(FlexAlign::Start)
            .child(section_title("Virtual List + ScrollBar"))
            .child(
                Text::new("1000 行仅渲染视口内子项，右侧为可以拖拽的滚动条。")
                    .font_size(12.0)
                    .color(theme::current().text.subtle_default),
            )
            .child(
                Row::new()
                    .spacing(0.0)
                    .align_items(FlexAlign::Stretch)
                    .child(
                        VirtualList::new(360.0, 1000, 34.0, scroll_y.clone()).item(|i| {
                            Box::new(
                                Row::new()
                                    .align_items(FlexAlign::Center)
                                    .spacing(10.0)
                                    .child(Text::new(format!("第 {i} 行")).font_size(13.0))
                                    .child(Progress::new(((i % 10) as f64) / 10.0)),
                            ) as Box<dyn Widget>
                        }),
                    )
                    .child(
                        ScrollBar::vertical(360.0, scroll_y.clone(), 360.0, total).thickness(10.0),
                    ),
            ),
    )
}

/// 构造 ScrollView 内的 30 行演示内容。
fn build_scroll_rows() -> Column {
    let mut col = Column::new().spacing(4.0).align_items(FlexAlign::Stretch);
    for i in 0..30 {
        col = col.child(
            Container::new()
                .height(36.0)
                .background(if i % 2 == 0 {
                    theme::current().background.secondary_default
                } else {
                    Color::new(0xe6, 0xe6, 0xe6)
                })
                .child(
                    Row::new()
                        .expand(true)
                        .align_items(FlexAlign::Center)
                        .child(Container::new().width(10.0))
                        .child(Text::new(format!("第 {i} 行内容")).font_size(13.0)),
                ),
        );
    }
    col
}

fn section_scroll_view() -> impl Widget {
    card(
        Column::new()
            .spacing(12.0)
            .align_items(FlexAlign::Start)
            .child(section_title("ScrollView（通用滚动容器）"))
            .child(
                Text::new("任意内容的整体滚动（非虚拟化），滚轮可滚动。")
                    .font_size(12.0)
                    .color(theme::current().text.subtle_default),
            )
            .child(ScrollView::new(200.0).child(build_scroll_rows())),
    )
}

fn page_lists(scroll_y: State<(f32, f32)>) -> impl Widget {
    ScrollView::expand().scrollbar(true).child(
        Column::new()
            .spacing(16.0)
            .align_items(FlexAlign::Stretch)
            .child(section_virtual_list(scroll_y))
            .child(section_scroll_view()),
    )
}

// ───────────────────────────── 关于 ─────────────────────────────

fn page_about() -> impl Widget {
    ScrollView::expand().scrollbar(true).child(
        Column::new()
            .spacing(16.0)
            .align_items(FlexAlign::Start)
            .child(card(
                Column::new()
                    .spacing(10.0)
                    .align_items(FlexAlign::Start)
                    .child(section_title("关于 LieUI"))
                    .child(Text::new("LieUI v2 是一个声明式、响应式的 Rust UI 框架。"))
                    .child(Text::new("当前 Gallery 即以顶层 Tab 展示各类内置控件。")),
            ))
            .child(
                Text::new("Tip: 在输入框中试试 Ctrl+A/C/V/X，滚轮可在虚拟列表内滚动。")
                    .font_size(12.0)
                    .color(theme::current().text.subtle_default),
            ),
    )
}

// ───────────────────────────── 设计系统（PatternFly 令牌展示） ─────────────────────────────

fn status_badge(text: &str, fg: Color, bg: Color) -> impl Widget {
    Container::new()
        .padding(theme::current().spacer.xs)
        .border_radius(theme::current().radius.small)
        .background(bg)
        .child(Text::new(text.to_string()).font_size(12.0).color(fg))
}

fn section_status() -> impl Widget {
    let t = theme::current();
    card(
        Column::new()
            .spacing(12.0)
            .align_items(FlexAlign::Start)
            .child(section_title("状态色（Status tokens）"))
            .child(
                Row::new()
                    .spacing(8.0)
                    .align_items(FlexAlign::Center)
                    .child(status_badge("Danger", t.status.danger, t.status.danger_bg))
                    .child(status_badge(
                        "Success",
                        t.status.success,
                        t.status.success_bg,
                    ))
                    .child(status_badge(
                        "Warning",
                        t.status.warning,
                        t.status.warning_bg,
                    ))
                    .child(status_badge("Info", t.status.info, t.status.info_bg)),
            )
            .child(
                Row::new()
                    .spacing(8.0)
                    .align_items(FlexAlign::Center)
                    .child(
                        Container::new()
                            .width(16.0)
                            .height(16.0)
                            .background(t.status.danger)
                            .border_radius(8.0),
                    )
                    .child(
                        Container::new()
                            .width(16.0)
                            .height(16.0)
                            .background(t.status.success)
                            .border_radius(8.0),
                    )
                    .child(
                        Container::new()
                            .width(16.0)
                            .height(16.0)
                            .background(t.status.warning)
                            .border_radius(8.0),
                    )
                    .child(
                        Container::new()
                            .width(16.0)
                            .height(16.0)
                            .background(t.status.info)
                            .border_radius(8.0),
                    ),
            ),
    )
}

fn section_typography() -> impl Widget {
    let t = theme::current();
    let mut col = Column::new()
        .spacing(8.0)
        .align_items(FlexAlign::Start)
        .child(section_title("字号量表（Font tokens）"));
    let sizes = [
        ("xs (12px)", t.font.xs),
        ("sm (14px)", t.font.sm),
        ("md (16px)", t.font.md),
        ("lg (18px)", t.font.lg),
        ("xl (24px)", t.font.xl),
        ("2xl (32px)", t.font.x2l),
        ("3xl (40px)", t.font.x3l),
    ];
    for (label, size) in sizes {
        col = col.child(
            Row::new()
                .spacing(12.0)
                .align_items(FlexAlign::Center)
                .child(
                    Container::new().width(64.0).child(
                        Text::new(label.to_string())
                            .font_size(12.0)
                            .color(t.text.subtle_default),
                    ),
                )
                .child(Text::new("LieUI 排版 Aa").font_size(size)),
        );
    }
    card(col)
}

fn page_design() -> impl Widget {
    ScrollView::expand().scrollbar(true).child(
        Column::new()
            .spacing(16.0)
            .align_items(FlexAlign::Stretch)
            .child(section_typography())
            .child(section_status()),
    )
}

// ───────────────────────────── 应用入口 ─────────────────────────────

fn main() {
    let count = State::new(0i32);
    let checked = State::new(false);
    let input_text = State::new("".to_string());
    let multi_text = State::new("第一行\n第二行".to_string());

    // 交互控件演示用状态
    let slider_val = State::new(0.4f32);
    let switch_val = State::new(true);
    let radio_val = State::new(1usize);
    let scroll_y = State::new((0.0f32, 0.0f32));
    let drag_pos = State::new((0.0f32, 0.0f32));
    let drag_active = State::new(false);
    let icon_clicks = State::new(0i32);

    // 顶层分页 + 嵌套 Tab 演示状态
    let page = State::new(0usize);
    let nested = State::new(0usize);

    // 明暗主题切换状态
    let theme_mode = State::new(Mode::Light);

    let app = Application::new(WindowConfig::new().size(900.0, 720.0), move |_ctx| {
        // 先分 tab：每个 tab 的内容由各 page_* 函数独立构建（其内部再分 section）。
        let tabs = Tab::new(page.clone())
            .header_height(38.0)
            .tab(
                "基础控件",
                page_basic(
                    count.clone(),
                    checked.clone(),
                    input_text.clone(),
                    multi_text.clone(),
                ),
            )
            .tab("布局", page_layout())
            .tab("设计系统", page_design())
            .tab(
                "交互控件",
                page_interactive(
                    slider_val.clone(),
                    switch_val.clone(),
                    radio_val.clone(),
                    nested.clone(),
                    drag_pos.clone(),
                    drag_active.clone(),
                    icon_clicks.clone(),
                ),
            )
            .tab("容器组件", page_containers(switch_val.clone()))
            .tab("虚拟列表", page_lists(scroll_y.clone()))
            .tab("关于", page_about());

        Box::new(
                Row::new()
                    .expand(true)
                    .justify_content(FlexAlign::Center)
                    .align_items(FlexAlign::Stretch)
                    .child(
                        Container::new()
                            .max_width(680.0)
                            .expand(true)
                            .padding(24.0)
                            .child(
                                Column::new()
                                    .expand(true)
                                    .spacing(20.0)
                                    .align_items(FlexAlign::Stretch)
                                    // Header：标题 + 明暗切换按钮
                                    .child(
                                        Row::new()
                                            .flex_shrink(0.0)
                                            .justify_content(FlexAlign::SpaceBetween)
                                            .align_items(FlexAlign::Center)
                                            .child(
                                                Column::new()
                                                    .spacing(4.0)
                                                    .align_items(FlexAlign::Start)
                                                    .child(Text::new("LieUI Gallery").font_size(36.0))
                                                    .child(
                                                        Text::new(
                                                            "Built-in widgets preview — 用 Tab 切换分类",
                                                        )
                                                        .font_size(14.0)
                                                        .color(theme::current().text.subtle_default),
                                                    ),
                                            )
                                            .child(
                                                Button::new(
                                                    if *theme_mode.get() == Mode::Dark {
                                                        "切换到亮色"
                                                    } else {
                                                        "切换到暗色"
                                                    },
                                                )
                                                .variant(ButtonVariant::Secondary)
                                                .on_click({
                                                    let m = theme_mode.clone();
                                                    move || {
                                                        let next = if *m.get() == Mode::Dark {
                                                            Mode::Light
                                                        } else {
                                                            Mode::Dark
                                                        };
                                                        m.set(next);
                                                        set_mode(next);
                                                        request_rebuild();
                                                    }
                                                }),
                                            ),
                                    )
                                    .child(tabs),
                            ),
                    ),
            )
    });

    app.run();
}
