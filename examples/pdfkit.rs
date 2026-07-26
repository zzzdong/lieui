//! PDFKit — PDF 页面管理小工具
//!
//! 功能原型: 加载 PDF → 缩略图列表 → 勾选/删除/提取为新的PDF
//!
//! 布局:
//!   ┌─ Header ──────────────────────────────────────┐
//!   │  PDFKit  |  sample.pdf  |           [Import]   │
//!   ├─ Sidebar ─┬─ Toolbar ─────────────────────────┤
//!   │  Pages(8) │  [Load] [Delete] [Extract] [Export]│
//!   │  ┌───┐    ├───────────────────────────────────┤
//!   │  │ 1 │   │     ┌─────────────────┐           │
//!   │  │ 2 │   │     │   Page Preview    │           │
//!   │  │ 3 │   │     │   612 × 792       │           │
//!   │  ...   │     │   Sample text...    │           │
//!   │        │     └─────────────────┘           │
//!   ├─ Status ──┴───────────────────────────────────┤
//!   │  Pages: 8  |  Selected: 1  |  Ready            │
//!   └───────────────────────────────────────────────┘

use lieui::geometry::{Color, Size};
use lieui::layout::types::FlexAlign;
use lieui::prelude::*;
use lieui::state::State;
use lieui::widget::Widget;

fn hex(c: u32) -> Color {
    Color::new(
        ((c >> 16) & 0xff) as u8,
        ((c >> 8) & 0xff) as u8,
        (c & 0xff) as u8,
    )
}

#[derive(Clone, Default)]
struct PageInfo {
    index: usize,
    label: String,
    dimensions: String,
    preview_text: String,
}

fn sample_pages() -> Vec<PageInfo> {
    (1..=8)
        .map(|i| PageInfo {
            index: i - 1,
            label: format!("Page {}", i),
            dimensions: "612 × 792 pt".to_string(),
            preview_text: format!("Text content extracted from page {}.", i),
        })
        .collect()
}

fn divider(border: Color) -> impl Widget {
    Container::new().height(1.0).background(border)
}

// ========== 应用 ==========

fn main() {
    let pages = State::new(Vec::<PageInfo>::new());
    let selected = State::new(Vec::<bool>::new());
    let current = State::new(None::<usize>);
    let status = State::new("Ready — click Import PDF... to begin".to_string());

    let load = {
        let p = pages.clone();
        let s = selected.clone();
        let c = current.clone();
        let st = status.clone();
        move || {
            let sm = sample_pages();
            let n = sm.len();
            p.set(sm);
            s.set(vec![false; n]);
            c.set(Some(0));
            st.set(format!("Loaded {} pages", n));
        }
    };

    let del = {
        let p = pages.clone();
        let s = selected.clone();
        let c = current.clone();
        let st = status.clone();
        move || {
            let sel = s.get();
            let cnt = sel.iter().filter(|&&b| b).count();
            if cnt == 0 {
                st.set("No pages selected".to_string());
                return;
            }
            let list = p.get();
            let kept: Vec<_> = list
                .iter()
                .enumerate()
                .filter(|(i, _)| !sel.get(*i).copied().unwrap_or(false))
                .map(|(_, info)| info.clone())
                .enumerate()
                .map(|(ni, mut inf)| {
                    inf.index = ni;
                    inf.label = format!("Page {}", ni + 1);
                    inf
                })
                .collect();
            let n = kept.len();
            p.set(kept);
            s.set(vec![false; n]);
            c.set(if n > 0 { Some(0) } else { None });
            st.set(format!("Deleted {} pages ({} remaining)", cnt, n));
        }
    };

    let ext = {
        let s = selected.clone();
        let st = status.clone();
        move || {
            let c = s.get().iter().filter(|&&b| b).count();
            st.set(if c == 0 {
                "No pages selected".to_string()
            } else {
                format!("Extract {} pages → extracted.pdf", c)
            });
        }
    };

    let exp = {
        let p = pages.clone();
        let st = status.clone();
        move || {
            let n = p.get().len();
            st.set(if n == 0 {
                "No document loaded".to_string()
            } else {
                format!("Export {} pages → output.pdf", n)
            });
        }
    };

    let tog = {
        let s = selected.clone();
        move |idx: usize| {
            s.update(|v| {
                if let Some(b) = v.get_mut(idx) {
                    *b = !*b;
                }
            })
        }
    };
    let set_cur = {
        let c = current.clone();
        move |idx: usize| c.set(Some(idx))
    };

    // ── Builder ──

    let app = Application::new(
        move |_ctx| {
            let c = |h| hex(h);
            let hbg = c(0x1e293b);
            let sbb = c(0xf1f5f9);
            let acc = c(0x3b82f6);
            let acl = c(0xdbeafe);
            let tp = c(0x0f172a);
            let ts = c(0x64748b);
            let tm = c(0x94a3b8);
            let bd = c(0xe2e8f0);
            let sbg = c(0xf8fafc);
            let cbgb = c(0xf8fafc);
            let tb = c(0xdbeafe);

            let pl = pages.get();
            let sl = selected.get();
            let cu = *current.get();
            let sc = sl.iter().filter(|&&b| b).count();
            let stxt: String = status.get().clone();

            // ── 左侧：缩略图列表 ──

            let mut side = Column::new().spacing(4.0).child(
                Text::new(format!("Pages ({})", pl.len()))
                    .font_size(13.0)
                    .color(ts),
            );

            if pl.is_empty() {
                side = side.child(Text::new("No pages loaded").font_size(11.0).color(tm));
            } else {
                for (idx, info) in pl.iter().enumerate() {
                    let chk = *sl.get(idx).unwrap_or(&false);
                    let curp = cu == Some(idx);
                    let t = tog.clone();
                    let c = set_cur.clone();
                    side = side.child(
                        Container::new()
                            .background(if curp { acl } else { Color::WHITE })
                            .child(
                                Row::new()
                                    .spacing(10.0)
                                    .align_items(FlexAlign::Center)
                                    .child(
                                        Container::new()
                                            .width(48.0)
                                            .height(64.0)
                                            .background(tb)
                                            .child(
                                                Column::new()
                                                    .expand(true)
                                                    .justify_content(FlexAlign::Center)
                                                    .align_items(FlexAlign::Center)
                                                    .child(
                                                        Text::new(format!("{}", info.index + 1))
                                                            .font_size(28.0)
                                                            .color(acc),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        Column::new()
                                            .spacing(2.0)
                                            .child(Text::new(&info.label).font_size(13.0).color(tp))
                                            .child(
                                                Text::new(&info.dimensions)
                                                    .font_size(10.0)
                                                    .color(tm),
                                            )
                                            .child(
                                                Checkbox::new(chk)
                                                    .label("Select")
                                                    .on_click(move || t(idx)),
                                            ),
                                    )
                                    .child(Button::new("View").on_click(move || c(idx))),
                            ),
                    );
                }
            }

            let sidebar = Container::new()
                .width(220.0)
                .background(sbb)
                .child(ScrollView::new(520.0).child(side));

            // ── 右侧 ──

            // 预览
            let preview: Container = if let Some(idx) = cu {
                if let Some(info) = pl.get(idx) {
                    let card = Container::new().expand(true).padding(24.0).child(
                        Column::new()
                            .spacing(12.0)
                            .child(
                                Column::new()
                                    .spacing(4.0)
                                    .child(Text::new(&info.label).font_size(24.0).color(tp))
                                    .child(Text::new(&info.dimensions).font_size(13.0).color(ts)),
                            )
                            .child(divider(bd))
                            .child(
                                Column::new()
                                    .spacing(6.0)
                                    .child(Text::new("Text preview:").font_size(11.0).color(tm))
                                    .child(Text::new(&info.preview_text).font_size(12.0).color(tp)),
                            ),
                    );
                    Container::new().expand(true).background(cbgb).child(
                        Column::new()
                            .expand(true)
                            .justify_content(FlexAlign::Center)
                            .align_items(FlexAlign::Stretch)
                            .child(card),
                    )
                } else {
                    Container::new().expand(true).background(cbgb)
                }
            } else {
                Container::new().expand(true).background(cbgb).child(
                    Column::new()
                        .expand(true)
                        .justify_content(FlexAlign::Center)
                        .align_items(FlexAlign::Center)
                        .child(
                            Text::new("Select a page to preview")
                                .font_size(14.0)
                                .color(tm),
                        ),
                )
            };

            // 工具栏
            let toolbar = Column::new()
                .spacing(0.0)
                .child(
                    Container::new()
                        .padding(12.0)
                        .background(Color::WHITE)
                        .child(
                            Row::new()
                                .expand(true)
                                .justify_content(FlexAlign::SpaceBetween)
                                .align_items(FlexAlign::Center)
                                .child(
                                    Row::new()
                                        .spacing(12.0)
                                        .align_items(FlexAlign::Center)
                                        .child(Button::new("Load Sample").on_click(load.clone()))
                                        .child(Button::new("Delete Sel").on_click(del.clone()))
                                        .child(Button::new("Extract Sel").on_click(ext.clone()))
                                        .child(Button::new("Export All").on_click(exp.clone())),
                                )
                                .child(
                                    Text::new(format!("{} selected", sc))
                                        .font_size(12.0)
                                        .color(if sc > 0 { acc } else { tm }),
                                ),
                        ),
                )
                .child(divider(bd));

            let right = Column::new()
                .spacing(0.0)
                .expand(true)
                .child(toolbar)
                .child(preview);

            // ── 底部状态栏 ──

            let status_bar = Container::new().background(sbg).child(
                Row::new()
                    .spacing(24.0)
                    .align_items(FlexAlign::Center)
                    .child(
                        Text::new(format!("Pages: {}", pl.len()))
                            .font_size(11.0)
                            .color(ts),
                    )
                    .child(
                        Text::new(format!("Selected: {}", sc))
                            .font_size(11.0)
                            .color(ts),
                    )
                    .child(Text::new(&stxt).font_size(11.0).color(tm)),
            );

            // ── 页面结构 ──
            // Container.expand(true) 填满视口宽高，
            // 内层 Column 填充高度且从 Container 的 loose 约束获得全宽

            Box::new(
                Container::new().expand(true).child(
                    Column::new()
                        .spacing(0.0)
                        .expand(true)
                        .child(
                            Container::new().background(hbg).child(
                                Row::new()
                                    .justify_content(FlexAlign::SpaceBetween)
                                    .align_items(FlexAlign::Center)
                                    .child(
                                        Row::new()
                                            .spacing(16.0)
                                            .align_items(FlexAlign::Center)
                                            .child(
                                                Text::new("PDFKit")
                                                    .font_size(20.0)
                                                    .color(Color::WHITE),
                                            )
                                            .child(
                                                Text::new("|").font_size(16.0).color(c(0x475569)),
                                            )
                                            .child(
                                                Text::new("sample.pdf")
                                                    .font_size(13.0)
                                                    .color(c(0x94a3b8)),
                                            ),
                                    )
                                    .child(Button::new("Import PDF...").on_click(load.clone())),
                            ),
                        )
                        .child(
                            Row::new()
                                .spacing(0.0)
                                .expand(true)
                                .align_items(FlexAlign::Stretch)
                                .child(sidebar)
                                .child(right),
                        )
                        .child(divider(bd))
                        .child(status_bar),
                ),
            )
        },
        Size::new(960.0, 640.0),
    );

    app.run();
}
