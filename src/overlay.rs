//! 框架级 loading 遮罩（「忙碌」遮罩）。
//!
//! 由 [`Runtime`](crate::reactive::Runtime) 的忙碌项驱动（见 [`crate::task`]）：
//! `rt.begin_busy(..)` ⇒ 窗口出现本遮罩；忙碌项清空 ⇒
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
#[path = "overlay_tests.rs"]
mod tests;
