//! M5/M6 示例：**组件陈列馆** —— v3 全部 widget 的交互式展示。
//!
//! ```text
//! cargo run -p lieui --example gallery
//! ```
//!
//! 展示清单（全部真实交互）：
//! 1. **菜单栏**（文件/编辑 + 子菜单，声明式互斥 + 轻关闭）；
//! 2. **按钮**（点击改 Signal → 只重绘那一小块）；
//! 3. **主题切换**（明/暗，token 重新烘焙 + 整窗重绘）；
//! 4. **滑块**（双向绑定）+ **进度条**（随动）；
//! 5. **复选框 / 开关**（共享同一个 Signal，两个控件自动互相同步）；
//! 6. **单选组**（声明式互斥）；
//! 7. **输入框**（双向绑定：打字 / 方向键 / 选区 / Ctrl+A·C·V / 中文 IME）；
//! 8. **下拉选择**（锚定按钮 + 轻关闭弹层）；
//! 9. **滚动区**（滚轮 / 拖动滚动条）；
//! 10. **自绘波形**（`CustomNode`：自绘 + 自己的状态机）；
//! 11. **图片**（程序生成的渐变位图，contain 缩放）；
//! 12. **图标**（Material Icons 字体：`icon` / `icon_button`）；
//! 13. **Tab 焦点迁移**（框架默认行为，键盘聚焦画焦点框）。

use std::rc::Rc;

use lieui::prelude::*;
use lieui_layout::FlexAlign;

// ─────────────── 自定义节点（B+C 档：自绘 + 自己的状态机）───────────────

struct Wave {
    taps: u32,
}

impl CustomNode for Wave {
    fn intrinsic_size(&self) -> Size {
        Size::new(300.0, 44.0)
    }

    fn draw(&self, out: &mut Scene, rect: Rect, tr: Affine) {
        let n = 24usize;
        let w = rect.width / n as f32;
        for i in 0..n {
            let t = i as f32 / n as f32;
            let s = (t * 6.0 + self.taps as f32 * 0.9).sin() * 0.5 + 0.5;
            let h = rect.height * (0.2 + 0.8 * s);
            custom::fill_rect(
                out,
                Rect::new(rect.x + i as f32 * w, rect.bottom() - h, w * 0.7, h),
                1.5,
                Color::new(80, 140, 240),
                tr,
            );
        }
    }

    fn on_event(&mut self, id: NodeId, ev: &EventView, cmd: &mut CmdBuf) -> bool {
        if ev.kind == EventKind::Tapped {
            self.taps += 1;
            cmd.damage(id);
            true
        } else {
            false
        }
    }
}

// ─────────────── 演示模型 ───────────────

struct Gallery {
    rt: Runtime,
    clicks: Signal<i32>,
    volume: Signal<f32>,
    agree: Signal<bool>,
    flavor: Signal<String>,
    name: Signal<String>,
    combo_open: Signal<bool>,
    combo_choice: Signal<String>,
    open_menu: Signal<String>,
    sub_open: Signal<bool>,
    last_action: Signal<String>,
    wave: CustomCell,
    photo: std::sync::Arc<ImageData>,
}

impl Gallery {
    /// 菜单条按钮（`open_menu` 声明式互斥：只有一个菜单是打开的）
    fn menu_anchor(me: &Rc<Self>, v: &mut ViewBuf, name: &str) {
        let m = name.to_string();
        v.button(name)
            .key(format!("menu-{name}"))
            .on_tap(act1(me, |s, m: String| {
                s.sub_open.set(false); // 切换/关闭菜单 ⇒ 子菜单一并收起
                s.open_menu
                    .set(if s.open_menu.get() == m { String::new() } else { m });
            }, m));
    }

    fn menu_popup(me: &Rc<Self>, v: &mut ViewBuf, name: &str, items: &[&str]) {
        if me.open_menu.get() != name {
            return;
        }
        let key = format!("menu-{name}");
        v.popup_at(key.as_str(), Placement::Below, |p| {
            let mine = Rc::clone(me);
            let owned = name.to_string();
            // 轻关闭守卫：只关自己（不会清掉刚打开的兄弟菜单）
            p.on(EventKind::Dismissed, move |_| {
                if mine.open_menu.get() == owned {
                    mine.open_menu.set(String::new());
                    mine.sub_open.set(false);
                }
            });
            for item in items {
                let it = Rc::clone(me);
                let label = item.to_string();
                if item.ends_with("▸") {
                    // 子菜单锚点：本项是下一级弹层的锚（`item-{菜单}-{项}`）
                    let sk = format!("item-{name}-{item}");
                    p.text(item.to_string())
                        .hover_background(me.rt.theme().control_hover)
                        .radius(3.0)
                        .width(110.0)
                        .key(sk)
                        .on_tap(act(&it, |s| s.sub_open.set(!s.sub_open.get())));
                    continue;
                }
                p.text(item.to_string())
                    .hover_background(me.rt.theme().control_hover)
                    .radius(3.0)
                    .width(110.0)
                    .on_tap(move || {
                        it.last_action.set(label.clone());
                        it.open_menu.set(String::new());
                    });
            }
        });
    }

    /// 子菜单弹层：锚点在上层弹层内的项（嵌套锚定层，`Placement::RightOf`）
    fn submenu_popup(me: &Rc<Self>, v: &mut ViewBuf, menu: &str, item: &str, subs: &[&str]) {
        if me.open_menu.get() != menu || !me.sub_open.get() {
            return;
        }
        let key = format!("item-{menu}-{item}");
        v.popup_at(key.as_str(), Placement::RightOf, |p| {
            for sub in subs {
                let it = Rc::clone(me);
                let label = sub.to_string();
                p.text(sub.to_string())
                    .hover_background(me.rt.theme().control_hover)
                    .radius(3.0)
                    .width(110.0)
                    .on_tap(move || {
                        it.last_action.set(label.clone());
                        it.open_menu.set(String::new());
                        it.sub_open.set(false);
                    });
            }
        });
    }

    /// 下拉选择：锚定按钮（放在内容里）
    fn combo_anchor(me: &Rc<Self>, v: &mut ViewBuf) {
        let label = if me.combo_choice.get().is_empty() {
            "下拉选择…".to_string()
        } else {
            format!("已选：{}", me.combo_choice.get())
        };
        v.button(&label)
            .key("combo-btn")
            .width(150.0)
            .on_tap(act(me, |s| s.combo_open.set(!s.combo_open.get())));
    }

    /// 下拉弹层（内容之外声明；轻关闭）
    fn combo_popup(me: &Rc<Self>, v: &mut ViewBuf) {
        if !me.combo_open.get() {
            return;
        }
        v.popup_at("combo-btn", Placement::Below, |p| {
            let mine = Rc::clone(me);
            p.on(EventKind::Dismissed, move |_| mine.combo_open.set(false));
            for opt in ["樱桃", "猕猴桃", "水蜜桃"] {
                let it = Rc::clone(me);
                let label = opt.to_string();
                p.text(opt.to_string())
                    .hover_background(me.rt.theme().control_hover)
                    .radius(3.0)
                    .padding(6.0)
                    .width(150.0)
                    .on_tap(move || {
                        it.combo_choice.set(label.clone());
                        it.combo_open.set(false);
                    });
            }
        });
    }

    /// 分区分隔线（主题色细线，随内容列拉伸）
    fn divider(&self, v: &mut ViewBuf) {
        v.row(|r| {
            r.height(1.0);
            r.background(self.rt.theme().control_border);
        });
    }
}

impl ViewModel for Gallery {
    fn view(self: &Rc<Self>, v: &mut ViewBuf) {
        // 唯一的内容根：菜单栏（固定）+ 可滚动主体
        v.column(|c| {
            // ── 顶部：菜单栏 ──
            c.row(|r| {
                r.height(30.0);
                r.gap(0.0);
                r.align_items(FlexAlign::Center);
                Gallery::menu_anchor(self, r, "文件");
                Gallery::menu_anchor(self, r, "编辑");
                r.spacer();
                r.text(format!("最近操作：{}", {
                    let l = self.last_action.get();
                    if l.is_empty() { "（无）".to_string() } else { l }
                }))
                .font_size(13.0)
                .color(self.rt.theme().text_secondary);
            });

            // ── 可滚动主体：固定宽度内容列，水平居中 ──
            c.scroll(|s| {
                s.expand(true);
                s.align_items(FlexAlign::Center);
                s.column(|s| {
                s.width(360.0);
                s.gap(16.0);

            // ── 标题 + 主题切换 ──
            s.text("组件陈列馆").font_size(22.0);
            s.row(|r| {
                r.gap(10.0);
                r.button("切换深色主题").on_tap(act(self, |s| {
                    if s.rt.theme() == Theme::dark() {
                        s.rt.set_theme(Theme::light());
                    } else {
                        s.rt.set_theme(Theme::dark());
                    }
                }));
                r.text("或点这里看看 hover").font_size(13.0);
            });

            // ── 按钮 ──
            self.divider(s);
            s.text("按钮").font_size(16.0);
            s.row(|r| {
                r.gap(10.0);
                r.button(&format!("点了我 {} 次", self.clicks.get()))
                    .on_tap(act(self, |s| s.clicks.update(|v| *v += 1)));
                r.button("归零").on_tap(act(self, |s| s.clicks.set(0)));
            });

            // ── 滑块 + 进度条 ──
            self.divider(s);
            s.text(format!("音量 {:.1} / 10（拖我）", self.volume.get())).font_size(16.0);
            s.slider_bound(&self.volume, 0.0, 10.0);
            s.progress(self.volume.get() / 10.0);

            // ── 复选框 / 开关 / 单选（共享模型）──
            self.divider(s);
            s.text("选择控件（复选框与开关共享同一个值）").font_size(16.0);
            s.row(|r| {
                r.gap(10.0);
                r.checkbox_bound(&self.agree);
                r.switch_bound(&self.agree);
                r.text(if self.agree.get() { "已启用" } else { "已禁用" }).font_size(14.0);
            });
            s.row(|r| {
                r.gap(10.0);
                r.radio_bound(&self.flavor, "apple");
                r.text("苹果").font_size(14.0);
                r.radio_bound(&self.flavor, "banana");
                r.text("香蕉").font_size(14.0);
                r.radio_bound(&self.flavor, "cherry");
                r.text("樱桃").font_size(14.0);
            });

            // ── 下拉选择 ──
            self.divider(s);
            s.text("下拉选择").font_size(16.0);
            Gallery::combo_anchor(self, s);

            // ── 图标（Material Icons 字体，名称即 codepoints 表里的名字）──
            self.divider(s);
            s.text("图标（icon / icon_button）").font_size(16.0);
            s.row(|r| {
                r.gap(6.0);
                r.align_items(FlexAlign::Center);
                for name in ["home", "search", "settings", "add", "delete", "close", "refresh"] {
                    r.icon_button(name)
                        .on_tap(act1(self, |s, n: String| s.last_action.set(format!("图标 {n}")), name.to_string()));
                }
            });
            s.row(|r| {
                r.gap(16.0);
                r.align_items(FlexAlign::Center);
                r.icon("favorite").color(self.rt.theme().accent).font_size(28.0);
                r.icon("star").font_size(24.0);
                r.icon("info").font_size(20.0);
                r.icon("menu").font_size(16.0);
                r.text("不同字号（28/24/20/16），可 .color 变色")
                    .font_size(13.0)
                    .color(self.rt.theme().text_secondary);
            });

            // ── 输入框 ──
            self.divider(s);
            s.text("输入框（中文 IME / Ctrl+A·C·V / 方向键选区）").font_size(16.0);
            s.input_bound(&self.name).placeholder("在这里输入…");
            s.text(format!("正在输入：{}", self.name.get())).font_size(14.0);

            // ── 图片 ──
            self.divider(s);
            s.text("图片（程序生成，contain 缩放）").font_size(16.0);
            s.image(self.photo.clone()).width(240.0).height(120.0);

            // ── 自绘波形 ──
            self.divider(s);
            s.text("自绘波形（CustomNode，点它）").font_size(16.0);
            s.custom(&self.wave);

            // ── 滚动区（嵌套滚动）──
            self.divider(s);
            s.text("嵌套滚动区").font_size(16.0);
            s.scroll(|inner| {
                inner.width(320.0);
                inner.height(140.0);
                inner.gap(6.0);
                inner.align_items(FlexAlign::Start);
                for i in 1..=16 {
                    inner.text(format!("内滚内容第 {i} 行 —— 滚轮或拖右侧滚动条"));
                }
            });

            s.text("Tab 可在按钮 / 滑块 / 复选框 / 开关 / 单选 / 输入框之间移动焦点").font_size(13.0);
                });
            });
        });

        // ── 弹层（锚点在内容里，层根在内容之外声明）──
        Gallery::menu_popup(self, v, "文件", &["新建", "打开", "保存"]);
        Gallery::menu_popup(self, v, "编辑", &["撤销", "重做", "查找 ▸"]);
        Gallery::submenu_popup(self, v, "编辑", "查找 ▸", &["查找内容", "查找下一个", "替换…"]);
        Gallery::combo_popup(self, v);
    }
}

/// 程序生成一张 64×64 渐变图（对角渐变 + 圆点），避免外部资源依赖
fn demo_image() -> ImageData {
    let (w, h) = (64u32, 64u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - 32.0;
            let dy = y as f32 - 32.0;
            let d = (dx * dx + dy * dy).sqrt();
            let ring = ((d - 18.0).abs() < 6.0) as u8 * 255;
            rgba.push((x * 3 + ring as u32) as u8);
            rgba.push((y * 3) as u8);
            rgba.push(200);
            rgba.push(255);
        }
    }
    ImageData { width: w, height: h, rgba }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = Runtime::new();
    let mut app = App::new(rt.clone());

    let vm = Rc::new(Gallery {
        clicks: Signal::new(&rt, 0),
        volume: Signal::new(&rt, 3.0),
        agree: Signal::new(&rt, false),
        flavor: Signal::new(&rt, "apple".to_string()),
        name: Signal::new(&rt, String::new()),
        combo_open: Signal::new(&rt, false),
        combo_choice: Signal::new(&rt, String::new()),
        open_menu: Signal::new(&rt, String::new()),
        sub_open: Signal::new(&rt, false),
        last_action: Signal::new(&rt, String::new()),
        wave: custom::cell(Wave { taps: 0 }),
        rt: rt.clone(),
        photo: std::sync::Arc::new(demo_image()),
    });
    let id = app.window_erased(
        WindowConfig::new()
            .title("lieui v3 · 组件陈列馆")
            .size(480.0, 640.0)
            .min_size(380.0, 420.0),
        lieui::app::erased(Rc::clone(&vm)),
    );
    let _ = id;

    app.run()
}
