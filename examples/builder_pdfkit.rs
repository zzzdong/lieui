//! PDFKit 小工具 UI 原型 — 展示 PDF 页面管理界面
//!
//! 运行: cargo run --example builder_pdfkit
//!
//! 这是一个纯 UI 展示 demo：
//! - 左侧勾选页面
//! - 右侧预览当前页面信息
//! - 工具栏按钮仅更新状态文本，不实际读写文件

use lieui::geometry::{Color, Size};
use lieui::layout::flex::AlignItems;
use lieui::prelude::*;
use lieui::state::State;
use lieui::view::View;

#[derive(Clone, Default)]
struct PageInfo {
    index: usize,
    label: String,
    dimensions: String,
    text_preview: String,
}

fn sample_pages() -> Vec<PageInfo> {
    (1..=8)
        .map(|i| PageInfo {
            index: i - 1,
            label: format!("Page {}", i),
            dimensions: "612 x 792 pt".to_string(),
            text_preview: format!(
                "Sample text content for page {}. In the real tool this area \
                 would show the extracted text layer or a rendered thumbnail \
                 of the selected PDF page.",
                i
            ),
        })
        .collect()
}

fn color(hex: u32) -> Color {
    let r = ((hex >> 16) & 0xff) as u8;
    let g = ((hex >> 8) & 0xff) as u8;
    let b = (hex & 0xff) as u8;
    Color::new(r, g, b)
}

fn main() {
    let pages = State::new(Vec::<PageInfo>::new());
    let selected = State::new(Vec::<bool>::new());
    let current = State::new(None::<usize>);
    let status = State::new("Ready — click Load Sample".to_string());

    let load_sample = {
        let pages = pages.clone();
        let selected = selected.clone();
        let current = current.clone();
        let status = status.clone();
        move || {
            let sample = sample_pages();
            let n = sample.len();
            pages.set(sample);
            selected.set(vec![false; n]);
            current.set(if n > 0 { Some(0) } else { None });
            status.set(format!("Loaded {} sample pages", n));
        }
    };

    let toggle_select = {
        let selected = selected.clone();
        move |idx: usize| {
            selected.update(|v| {
                if let Some(b) = v.get_mut(idx) {
                    *b = !*b;
                }
            });
        }
    };

    let set_current = {
        let current = current.clone();
        move |idx: usize| current.set(Some(idx))
    };

    let delete_selected = {
        let pages = pages.clone();
        let selected = selected.clone();
        let current = current.clone();
        let status = status.clone();
        move || {
            let sel = selected.get();
            let count = sel.iter().filter(|&&b| b).count();
            if count == 0 {
                status.set("No pages selected".to_string());
                return;
            }
            let mut list = pages.get().clone();
            let mut kept = Vec::new();
            for (i, info) in list.into_iter().enumerate() {
                if !sel.get(i).copied().unwrap_or(false) {
                    kept.push(info);
                }
            }
            let n = kept.len();
            list = kept
                .into_iter()
                .enumerate()
                .map(|(i, mut info)| {
                    info.index = i;
                    info.label = format!("Page {}", i + 1);
                    info
                })
                .collect();
            pages.set(list);
            selected.set(vec![false; n]);
            current.set(if n > 0 { Some(0) } else { None });
            status.set(format!(
                "Would delete {} selected pages ({} remaining)",
                count, n
            ));
        }
    };

    let extract_selected = {
        let selected = selected.clone();
        let status = status.clone();
        move || {
            let count = selected.get().iter().filter(|&&b| b).count();
            if count == 0 {
                status.set("No pages selected".to_string());
            } else {
                status.set(format!("Would extract {} pages to extracted.pdf", count));
            }
        }
    };

    let save_doc = {
        let status = status.clone();
        move || {
            status.set("Would save current document to saved.pdf".to_string());
        }
    };

    let app = Application::new(
        move || {
            let page_list = pages.get();
            let sel = selected.get();
            let cur = *current.get();

            // ---- 左侧页面列表 ----
            let mut sidebar_content = Column::new().spacing(6.0);
            sidebar_content =
                sidebar_content.child(Text::new("Pages").font_size(16.0).color(color(0x334155)));
            sidebar_content = sidebar_content.child(Divider);

            if page_list.is_empty() {
                sidebar_content = sidebar_content.child(
                    Text::new("No PDF loaded")
                        .font_size(12.0)
                        .color(color(0x94a3b8)),
                );
            } else {
                for (idx, info) in page_list.iter().enumerate() {
                    let checked = *sel.get(idx).unwrap_or(&false);
                    let is_current = cur == Some(idx);
                    let label = format!("{}  ·  {}", info.label, info.dimensions);
                    let row_bg = if is_current {
                        color(0xe0f2fe)
                    } else {
                        color(0xffffff)
                    };
                    let t = toggle_select.clone();
                    let c = set_current.clone();
                    let row = Container::new().background(row_bg).padding(8.0).child(
                        Row::new()
                            .spacing(8.0)
                            .align_items(AlignItems::Center)
                            .child(
                                Checkbox::new(checked)
                                    .label(&label)
                                    .on_click(move || t(idx)),
                            )
                            .child(Button::new("View").on_click(move || c(idx))),
                    );
                    sidebar_content = sidebar_content.child(row);
                }
            }

            let sidebar = Container::new()
                .width(260.0)
                .background(color(0xf8fafc))
                .padding(12.0)
                .child(sidebar_content);

            // ---- 顶部工具栏 ----
            let mut toolbar = Row::new().spacing(12.0).align_items(AlignItems::Center);
            let l = load_sample.clone();
            toolbar = toolbar.child(Button::new("Load Sample").on_click(l));
            let d = delete_selected.clone();
            toolbar = toolbar.child(Button::new("Delete Selected").on_click(d));
            let e = extract_selected.clone();
            toolbar = toolbar.child(Button::new("Extract Selected").on_click(e));
            let s = save_doc.clone();
            toolbar = toolbar.child(Button::new("Save").on_click(s));

            // ---- 右侧预览区 ----
            let mut preview_content = Column::new().spacing(10.0);
            preview_content =
                preview_content.child(Text::new("Preview").font_size(16.0).color(color(0x334155)));
            preview_content = preview_content.child(Divider);

            if let Some(idx) = cur {
                if let Some(info) = page_list.get(idx) {
                    preview_content = preview_content.child(
                        Text::new(format!("{} / {}", info.label, page_list.len()))
                            .font_size(14.0)
                            .color(color(0x0f172a)),
                    );
                    preview_content = preview_content.child(
                        Text::new(format!("Dimensions: {}", info.dimensions))
                            .font_size(12.0)
                            .color(color(0x475569)),
                    );
                    preview_content = preview_content.child(
                        Text::new("Text preview:")
                            .font_size(12.0)
                            .color(color(0x64748b)),
                    );
                    preview_content = preview_content.child(
                        Text::new(&info.text_preview)
                            .font_size(12.0)
                            .color(color(0x334155)),
                    );
                }
            } else {
                preview_content = preview_content.child(
                    Text::new("Select a page to preview")
                        .font_size(12.0)
                        .color(color(0x94a3b8)),
                );
            }

            let preview = Container::new()
                .background(color(0xf1f5f9))
                .padding(16.0)
                .expand(true)
                .child(preview_content);

            let main = Column::new()
                .spacing(12.0)
                .expand(true)
                .child(
                    Container::new()
                        .background(Color::WHITE)
                        .padding(12.0)
                        .child(toolbar),
                )
                .child(preview);

            let body = Row::new()
                .spacing(16.0)
                .expand(true)
                .child(sidebar)
                .child(main);

            let header = Container::new()
                .background(color(0x1e293b))
                .padding(16.0)
                .child(Row::new().child(Text::new("PDFKit").font_size(22.0).color(Color::WHITE)));

            Container::new()
                .background(color(0xe2e8f0))
                .expand(true)
                .child(
                    Column::new().spacing(0.0).expand(true).child(header).child(
                        Container::new()
                            .background(color(0xe2e8f0))
                            .padding(16.0)
                            .expand(true)
                            .child(body),
                    ),
                )
                .build()
        },
        Size::new(900.0, 600.0),
    );

    app.run();
}
