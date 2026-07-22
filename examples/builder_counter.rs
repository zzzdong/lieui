//! Builder 模式计数器，与 counter.rs 布局一致
//! 展示：text_with_style 配置文本、动态 widget、按钮回调、条件渲染

use lieui::prelude::*;
use lieui::text::TextStyle;

fn main() {
    let mut vc = ViewContext::new(Size::new(800.0, 600.0));
    let count = vc.state(0);

    let title_style = {
        let mut s = TextStyle::new();
        s.0.font_size = 48.0;
        s
    };
    let value_style = {
        let mut s = TextStyle::new();
        s.0.font_size = 72.0;
        s
    };

    vc.set_build_fn(move |bctx| {
        let c = *count.get();

        bctx.container_bg("#F5F5F5", |bctx| {
            // 外 Column：expand 填满窗口 + 垂直居中
            bctx.column(|bctx| {
                // 内 Column：不带 expand，justify=Start，spacing=16
                bctx.column_start(16.0, |bctx| {
                    bctx.text_with_style("Counter", &title_style);
                    bctx.text_with_style(&c.to_string(), &value_style);

                    bctx.row(|bctx| {
                        let dec = count.clone();
                        bctx.button("-", move |ectx| {
                            dec.update(|v| *v -= 1);
                            ectx.request_rebuild();
                        });

                        let inc = count.clone();
                        bctx.button("+", move |ectx| {
                            inc.update(|v| *v += 1);
                            ectx.request_rebuild();
                        });
                    });

                    let rst = count.clone();
                    bctx.button("Reset", move |ectx| {
                        rst.set(0);
                        ectx.request_rebuild();
                    });
                });
            });
        });
    });

    vc.build();
    vc.run_blocking();
}
