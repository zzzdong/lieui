//! Builder 示例：模拟 pdfkit 的页面选择功能
//! 展示动态 widget 列表（for 循环中的 checkbox）和重建

use std::cell::RefCell;
use std::rc::Rc;

use lieui::prelude::*;

struct PdfModel {
    page_count: usize,
    current_page: usize,
    selected: Vec<bool>,
}

impl PdfModel {
    fn new(page_count: usize) -> Self {
        Self {
            page_count,
            current_page: 0,
            selected: vec![false; page_count],
        }
    }
}

fn main() {
    let mut vc = ViewContext::new(Size::new(600.0, 500.0));
    let model = Rc::new(RefCell::new(PdfModel::new(20)));

    vc.set_build_fn(move |bctx| {
        let m = model.borrow();
        let page_count = m.page_count;
        let current_page = m.current_page;
        let checked_count = m.selected.iter().filter(|&&v| v).count();
        drop(m);

        bctx.container_bg("#FAFBFC", |bctx| {
            // 外 Column：expand + 垂直居中
            bctx.column(|bctx| {
                // 工具按钮行
                bctx.row(|bctx| {
                    bctx.button("Open PDF...", |_| {});
                    bctx.button("Extract Selected", |_| {});
                    bctx.button("Delete Selected", |_| {});
                });

                // 正文行：页面列表 + 预览
                bctx.row(|bctx| {
                    // 左侧：页面 checkbox 列表（不 expand，从头排列）
                    bctx.column_start(2.0, |bctx| {
                        for i in 0..page_count {
                            let checked = {
                                let m = model.borrow();
                                m.selected.get(i).copied().unwrap_or(false)
                            };
                            let cb_model = model.clone();
                            bctx.checkbox(
                                &format!("page_{}", i),
                                &format!("Page {}", i + 1),
                                checked,
                                move |is_checked| {
                                    let mut m = cb_model.borrow_mut();
                                    if i < m.selected.len() {
                                        m.selected[i] = is_checked;
                                    }
                                    // 选中变化后不触发 rebuild（文字不变），
                                    // 但取消注释下一行可以实时更新状态文本
                                    // let _ = is_checked;
                                },
                            );
                        }
                    });

                    // 右侧：预览区域（不 expand）
                    bctx.column_start(0.0, |bctx| {
                        bctx.text(&format!(
                            "Preview: Page {} of {}  [{} selected]",
                            current_page + 1,
                            page_count,
                            checked_count,
                        ));
                    });
                });

                // 翻页导航行
                bctx.row(|bctx| {
                    let prev_model = model.clone();
                    bctx.button("‹ Prev", move |ectx| {
                        let mut m = prev_model.borrow_mut();
                        if m.current_page > 0 {
                            m.current_page -= 1;
                        }
                        ectx.request_rebuild();
                    });

                    let next_model = model.clone();
                    bctx.button("Next ›", move |ectx| {
                        let mut m = next_model.borrow_mut();
                        if m.current_page + 1 < m.page_count {
                            m.current_page += 1;
                        }
                        ectx.request_rebuild();
                    });
                });
            });
        });
    });

    vc.build();
    vc.run_blocking();
}
