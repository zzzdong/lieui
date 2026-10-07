//! 框架级 loading 遮罩（「忙碌」遮罩）。
//!
//! 由 [`Runtime`](crate::reactive::Runtime) 的忙碌项驱动（见 [`crate::task`]）：
//! `rt.begin_busy(..)` / `rt.spawn_task_busy(..)` ⇒ 窗口出现本遮罩；忙碌项清空 ⇒
//! 本帧不再声明这个层，`align` 的 stale 清理会把它删掉（**声明式**，无需手工增删）。
//!
//! 视觉分三层：
//!
//! ```text
//! Modal 层根（撑满窗口，backdrop 由层语义 + 主题 token 提供，阻断下层交互）
//!   └─ 卡片（居中）：标题行 = [spinner 动画] + 文案
//!                    确定进度 ⇒ 进度条 + "done / total"
//!                    可取消   ⇒ 右下角「取消」按钮
//! ```
//!
//! ## 动画怎么跑（重要的架构点）
//!
//! spinner 是 [`CustomNode`]（自绘）——**相位由挂钟计算**，所以动画帧只需要"重绘"
//! 不需要重跑 `view()`：
//!
//! - `WindowCtx::animate` 每个动画帧把**卡片矩形**标脏（`SPIN_PERIOD` ≈ 30fps）；
//! - `WindowCtx::next_wakeup` 在有遮罩时给出下一次唤醒时刻（否则空闲零功耗）；
//! - 定位卡片靠层根标签 [`BUSY_OVERLAY_TAG`]（`ViewBuf::modal_tagged` 打的）。
//!
//! ## 看得见才叫反馈
//!
//! 几毫秒就干完的活儿（小文件读盘）会在"下一帧还没出"时就结束，遮罩一闪而过、甚至
//! 完全看不见。用 [`crate::task::Runtime::set_busy_min_visible`] 设个最短可见时间
//! （推荐 300~400ms），这类快活儿就保证有反馈；真正耗时的任务不受影响。

use std::time::Duration;

use lieui_geom::{Color, Rect, Size};
use lieui_layout::FlexAlign;

use crate::custom::{self, CustomCell, CustomNode};
use crate::render::scene::Scene;
use crate::task::BusyItem;
use crate::transform::Affine;
use crate::view::ViewBuf;

/// 遮罩层根的标签（`WindowCtx` 靠它找回遮罩，做动画标脏）
pub(crate) const BUSY_OVERLAY_TAG: u64 = 0x6c69_6575_695f_6201;

/// spinner 动画帧间隔（约 30fps：够顺，且 CPU 友好）
pub(crate) const SPIN_PERIOD: Duration = Duration::from_millis(33);

/// 声明忙碌遮罩层（框架在 `view()` 之后追加；`items` 为空 ⇒ 不声明 ⇒ 旧层被清理）
pub(crate) fn push_busy_overlay(v: &mut ViewBuf, items: &[BusyItem], spinner: CustomCell) {
    let Some(top) = items.last() else {
        return;
    };
    let theme = *v.theme();
    let label = if items.len() > 1 {
        format!("{}（还有 {} 个任务）", top.label, items.len() - 1)
    } else {
        top.label.clone()
    };
    let ratio = top.ratio();
    // 明细优先用任务上报的人话（"第 2 / 3 个文件 · 正在合并 b.pdf"）；
    // 没上报就退回机器可读的 `done / total`。
    let detail = top
        .detail
        .clone()
        .or_else(|| top.progress.map(|(d, t)| format!("{d} / {t}")));
    let cancel = if top.is_cancellable() { top.cancel.clone() } else { None };

    v.modal_tagged(BUSY_OVERLAY_TAG, |m| {
        m.center(); // 层根撑满窗口 ⇒ 卡片居中
        m.container(|card| {
            card.width(300.0);
            card.padding(20.0);
            card.background(theme.input_background);
            card.border(1.0, theme.control_border);
            card.radius(theme.control_radius + 6.0);
            card.layout(|l| l.flex_shrink = 0.0);
            card.column(|col| {
                col.gap(14.0);
                col.row(|head| {
                    head.gap(10.0);
                    head.align_items(FlexAlign::Center);
                    head.custom(&spinner).width(18.0).height(18.0);
                    // 墨迹盒对齐：spinner 的视觉中心是**矩形中心**，文本若按行盒居中
                    // 会差 ~1px（ascent/descent 不对称）⇒ 两者视觉中心对不齐。
                    head.text(label).font_size(14.0).color(theme.text).optical_align(true);
                });
                if let Some(p) = ratio {
                    col.progress(p).width(260.0);
                    if let Some(d) = detail {
                        // 同理走墨迹盒：上下留白按墨迹算，卡片内的 14px 间距才均匀
                        col.text(d)
                            .font_size(12.0)
                            .color(theme.text_secondary)
                            .optical_align(true);
                    }
                }
                if let Some(cb) = cancel {
                    col.row(|row| {
                        row.justify_content(FlexAlign::End);
                        row.button("取消").on_tap(move || cb());
                    });
                }
            });
        });
    });
}

/// 三点脉冲 spinner（自绘；相位读挂钟 ⇒ 动画帧只需重绘）
pub(crate) struct Spinner {
    color: Color,
    /// 起始时刻（**单调时钟**，D62）。
    ///
    /// 此前相位取自 `SystemTime::now()`（挂钟）—— **挂钟会因 NTP 校时回拨**，
    /// 于是 `% (CYCLE_MS * N)` 的结果会突然倒退，spinner 表现为**倒转 / 跳帧**。
    /// `Instant` 是单调的，只有差值有意义，不受系统时间调整影响。
    started: std::time::Instant,
}

impl Spinner {
    pub(crate) const SIZE: f32 = 18.0;

    pub(crate) fn new(color: Color) -> Self {
        Self {
            color,
            started: std::time::Instant::now(),
        }
    }
}

impl CustomNode for Spinner {
    fn intrinsic_size(&self) -> Size {
        Size::new(Self::SIZE, Self::SIZE)
    }

    fn draw(&self, out: &mut Scene, rect: Rect, transform: Affine) {
        const N: usize = 3;
        const CYCLE_MS: u128 = 300; // 每个点亮起的时间

        // 点径随自己的矩形自适应（窄了也不会溢出）
        let gap = 3.0_f32;
        let dot = ((rect.width - gap * (N as f32 - 1.0)) / N as f32)
            .min(rect.height)
            .max(1.0);
        let total = N as f32 * dot + (N as f32 - 1.0) * gap;
        let x0 = rect.x + (rect.width - total) * 0.5;
        let y = rect.y + (rect.height - dot) * 0.5;
        // 相位用**单调时钟的 elapsed**（D62）。挂钟会被 NTP 校时回拨 ⇒ 取模结果倒退 ⇒ spinner 倒转/跳帧。
        let elapsed_ms = self.started.elapsed().as_millis();
        let phase = (elapsed_ms % (CYCLE_MS * N as u128)) as f32 / CYCLE_MS as f32;

        for i in 0..N {
            // 距离当前相位的"落后量"：0 = 刚亮起 ⇒ 最亮
            let delta = (phase - i as f32).rem_euclid(N as f32);
            let k = (1.0_f32 - delta).clamp(0.0_f32, 1.0_f32);
            let alpha = (70.0 + 185.0 * k) as u8;
            let c = Color::rgba(self.color.r, self.color.g, self.color.b, alpha);
            custom::fill_rect(
                out,
                Rect::new(x0 + i as f32 * (dot + gap), y, dot, dot),
                dot * 0.5,
                c,
                transform,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom::cell;
    use crate::layout::{layout, rect_of};
    use crate::reactive::Runtime;
    use crate::track::{Layer, Track};
    use crate::window::WindowId;
    use lieui_geom::Size as GSize;

    /// 明细文案优先：任务上报了 `detail` 就显示它，而不是机器味的 `done / total`
    #[test]
    fn overlay_prefers_the_reported_detail_line() {
        let rt = Runtime::new();
        let w = WindowId::new(1);
        rt.register_window(w);

        // ① 没上报明细 ⇒ 退回 `done / total`
        let busy = rt.begin_busy(w, "正在打开 2 个文件…");
        busy.set_progress(1, 6);
        let texts = overlay_texts(&rt, w);
        assert!(texts.contains(&"1 / 6".to_string()), "{texts:?}");

        // ② 上报明细 ⇒ 显示人话
        busy.set_detail("第 1 / 2 个文件 · 正在读取 a.pdf");
        let texts = overlay_texts(&rt, w);
        assert!(
            texts.contains(&"第 1 / 2 个文件 · 正在读取 a.pdf".to_string()),
            "显示明细：{texts:?}"
        );
        assert!(!texts.contains(&"1 / 6".to_string()), "不再显示 done / total");
        busy.finish();
    }

    /// 把遮罩声明 + 对齐一次，取回树里所有文本（断言遮罩文案用）
    fn overlay_texts(rt: &Runtime, w: WindowId) -> Vec<String> {
        let mut v = ViewBuf::new();
        v.begin();
        push_busy_overlay(&mut v, &rt.busy_items(w), cell(Spinner::new(Color::WHITE)));
        let mut track = Track::new();
        layout(&mut track, GSize::new(400.0, 300.0));
        crate::align::align(&mut track, &v);
        layout(&mut track, GSize::new(400.0, 300.0));
        track
            .node_ids()
            .filter_map(|n| match &track.get(n)?.kind {
                crate::track::Kind::Text(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn spinner_draws_three_dots_inside_its_rect() {
        let spinner = Spinner::new(Color::new(10, 20, 30));
        let mut scene = Scene::default();
        let rect = Rect::new(5.0, 7.0, 18.0, 18.0);
        spinner.draw(&mut scene, rect, Affine::IDENTITY);

        let dots: Vec<Rect> = scene
            .ops()
            .iter()
            .filter_map(|op| match op {
                crate::render::Op::Rect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(dots.len(), 3, "三个点");
        // 都在自己的矩形里，且不重叠
        for d in &dots {
            assert!(d.x >= rect.x - 0.5 && d.right() <= rect.right() + 0.5, "水平在框内");
            assert!(d.y >= rect.y - 0.5 && d.bottom() <= rect.bottom() + 0.5, "垂直在框内");
        }
        assert!(dots[0].right() <= dots[1].x + 0.01, "点之间有余量");
        assert!(dots[1].right() <= dots[2].x + 0.01);
    }

    /// 遮罩层被声明后能用 tag 找回来，且卡片是层根的第一个子节点
    /// （`WindowCtx` 的动画标脏依赖这个结构）。
    #[test]
    fn busy_overlay_is_a_tagged_modal_layer_with_a_card_child() {
        let rt = Runtime::new();
        let w = WindowId::new(1);
        rt.register_window(w);
        let busy = rt.begin_busy(w, "正在处理…");
        busy.set_progress(1, 4);

        let mut v = ViewBuf::new();
        v.begin();
        v.column(|c| {
            c.text("主界面");
        });
        push_busy_overlay(&mut v, &rt.busy_items(w), cell(Spinner::new(Color::WHITE)));

        let mut track = Track::new();
        layout(&mut track, GSize::new(400.0, 300.0));
        crate::align::align(&mut track, &v);
        layout(&mut track, GSize::new(400.0, 300.0));

        let root = track.root_by_tag(BUSY_OVERLAY_TAG).expect("遮罩层在");
        assert_eq!(root.layer, Layer::Modal);
        let card = track
            .children(root.node)
            .first()
            .copied()
            .expect("卡片是层根的第一个子节点");
        assert_eq!(track.get(card).unwrap().kind.tag(), crate::track::KindTag::Box);
        let card_rect = rect_of(&track, card);
        assert!(
            card_rect.width > 200.0 && card_rect.height > 40.0,
            "卡片有实际尺寸：{card_rect:?}"
        );
        // 层根撑满窗口（居中容器）
        let root_rect = rect_of(&track, root.node);
        assert!((root_rect.width - 400.0).abs() < 1.0, "层根撑满窗口");
    }

    /// 遮罩卡片里的文本走**墨迹盒**：
    ///
    /// - spinner 的视觉中心 = 它的矩形中心（三点画在框中央）；
    /// - 标题盒高 = 墨迹高 ⇒ 墨迹中心 = 盒中心 ⇒ 与 spinner 视觉中心重合。
    ///   （按行盒居中时盒中心对齐、但墨迹偏 ~1px，这正是要避免的。）
    #[test]
    fn overlay_texts_are_ink_aligned_with_the_spinner() {
        let rt = Runtime::new();
        let w = WindowId::new(1);
        rt.register_window(w);
        let busy = rt.begin_busy(w, "正在打开 3 个文件…");
        busy.set_progress(1, 6);
        busy.set_detail("第 1 / 3 个文件 · 正在读取 a.pdf");

        let mut v = ViewBuf::new();
        v.begin();
        v.column(|c| {
            c.text("主界面");
        });
        push_busy_overlay(&mut v, &rt.busy_items(w), cell(Spinner::new(Color::WHITE)));

        let mut track = Track::new();
        layout(&mut track, GSize::new(400.0, 300.0));
        crate::align::align(&mut track, &v);
        layout(&mut track, GSize::new(400.0, 300.0));

        let root = track.root_by_tag(BUSY_OVERLAY_TAG).expect("遮罩层在");
        let card = track.children(root.node)[0];
        let col = track.children(card)[0];
        let head = track.children(col)[0];
        let kids = track.children(head);
        assert_eq!(kids.len(), 2, "标题行 = [spinner, 文本]");
        let (spinner_node, label_node) = (kids[0], kids[1]);

        let sr = rect_of(&track, spinner_node);
        let lr = rect_of(&track, label_node);
        assert!(
            (sr.center().y - lr.center().y).abs() < 0.01,
            "标题与 spinner 的中心对齐：{} vs {}",
            lr.center().y,
            sr.center().y
        );

        let n = track.get(label_node).unwrap();
        assert!(n.text.spec.optical_align, "标题走墨迹盒");
        let ink = lieui_text::TextEngine::ink_bounds("正在打开 3 个文件…", &n.text.spec).expect("标题有墨迹");
        assert!(
            (lr.height - ink.height()).abs() < 0.01,
            "标题盒高 = 墨迹高（⇒ 墨迹中心就是盒中心）：{} vs {}",
            lr.height,
            ink.height()
        );

        // 明细行同理（间距按墨迹算，卡片里的 14px 才均匀）
        let detail_node = track
            .children(col)
            .iter()
            .copied()
            .find(|k| {
                track
                    .get(*k)
                    .is_some_and(|n| matches!(&n.kind, crate::track::Kind::Text(s) if s.contains("正在读取")))
            })
            .expect("明细行在");
        assert!(
            track.get(detail_node).unwrap().text.spec.optical_align,
            "明细行走墨迹盒"
        );
    }

    /// 忙碌项清空 ⇒ 不再声明该层 ⇒ 下一帧 tag 消失
    #[test]
    fn overlay_disappears_when_busy_items_are_gone() {
        let rt = Runtime::new();
        let w = WindowId::new(1);
        rt.register_window(w);
        {
            let busy = rt.begin_busy(w, "正在处理…");
            let mut v = ViewBuf::new();
            v.begin();
            push_busy_overlay(&mut v, &rt.busy_items(w), cell(Spinner::new(Color::WHITE)));
            let mut track = Track::new();
            layout(&mut track, GSize::new(400.0, 300.0));
            crate::align::align(&mut track, &v);
            assert!(track.root_by_tag(BUSY_OVERLAY_TAG).is_some());
            let _ = &busy;
            drop(busy);
        }

        let mut v = ViewBuf::new();
        v.begin();
        push_busy_overlay(&mut v, &rt.busy_items(w), cell(Spinner::new(Color::WHITE)));
        let mut track = Track::new();
        layout(&mut track, GSize::new(400.0, 300.0));
        crate::align::align(&mut track, &v);
        assert!(
            track.root_by_tag(BUSY_OVERLAY_TAG).is_none(),
            "忙碌项清空 ⇒ 遮罩层被 align 清理"
        );
    }

    /// 取消按钮：遮罩里真的有一个可点的按钮（描述层里挂上了 Tapped）
    #[test]
    fn cancellable_busy_item_adds_a_cancel_button() {
        let rt = Runtime::new();
        let w = WindowId::new(1);
        rt.register_window(w);
        let busy = rt.begin_busy(w, "正在导出…");
        busy.cancellable(|| {});

        let items = rt.busy_items(w);
        assert!(items[0].is_cancellable());

        let mut v = ViewBuf::new();
        v.begin();
        push_busy_overlay(&mut v, &items, cell(Spinner::new(Color::WHITE)));
        let has_button = v
            .nodes
            .iter()
            .any(|n| matches!(n.kind, crate::track::KindDesc::Button { .. }));
        assert!(has_button, "可取消的遮罩里应有按钮");
    }
}
