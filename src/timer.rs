//! 定时器与动画帧（"一个时钟"）。
//!
//! 帧唤醒的来源本来有三处硬编码（光标闪烁、tooltip 计时、loading spinner），每加一种
//! 动画就要改 `WindowCtx` 与平台层两处。这里把**时钟**收敛成一份表：
//!
//! | 需求 | API | 语义 |
//! |---|---|---|
//! | 延迟一次 | [`Runtime::set_timeout`] | `dur` 之后回调一次（回调拿 `&mut Ctx`） |
//! | 周期性 | [`Runtime::set_interval`] | 每 `dur` 回调一次，直到 [`TimerHandle::cancel`] |
//! | 逐帧动画 | [`Runtime::request_animation`] | 下一帧调用 `ViewModel::on_animation`；想继续就再请求 |
//!
//! 平台层只问一个问题："下次什么时候醒？"——答案是
//! [`WindowCtx::next_wakeup`](crate::app::WindowCtx::next_wakeup)，它把框架内部
//! （闪烁 / tooltip / 忙碌 spinner）与本模块（定时器 / 动画帧）取最早值。
//! 没有唤醒源时 `ControlFlow::Wait`（空闲零功耗）。
//!
//! ## 为什么回调不是 `Send`
//!
//! 定时器在 **UI 线程**执行（回调拿 `&mut Ctx`，可以改 `Signal`、开窗、起任务），
//! 所以闭包是 `!Send` 的普通 `FnMut`。跨线程的周期活儿请让工作线程自己循环，
//! 用 [`crate::task::Poster::post`] 把每轮结果投递回来。

use std::time::{Duration, Instant};

use crate::event::Ctx;
use crate::reactive::Runtime;
use crate::window::WindowId;

/// 动画帧间隔（约 60fps）：`request_animation` 的最小唤醒粒度
pub const FRAME_PERIOD: Duration = Duration::from_millis(16);

/// 定时器回调（UI 线程执行，可拿 `Ctx` 改状态 / 开窗 / 起任务）
type TimerCallback = Box<dyn FnMut(&mut Ctx)>;

/// 一条定时器记录（框架内部；用户拿到的是 [`TimerHandle`]）
pub(crate) struct Timer {
    pub(crate) id: u64,
    pub(crate) window: WindowId,
    pub(crate) deadline: Instant,
    /// `Some` = 周期定时器（到点后按它重排）
    pub(crate) interval: Option<Duration>,
    pub(crate) cb: Option<TimerCallback>,
}

impl Timer {
    /// 取出回调（到点时调用；`interval` 由调用方决定是否重排）
    pub(crate) fn take_cb(&mut self) -> Option<TimerCallback> {
        self.cb.take()
    }
}

/// 定时器句柄：用来取消 / 查询（`!Send`，留在 UI 线程）。
///
/// 注意：**丢掉句柄不会取消定时器**（`let _ = rt.set_timeout(..)` 是常见写法，
/// 不该自杀）。要取消就显式 [`Self::cancel`]。
pub struct TimerHandle {
    rt: Runtime,
    id: u64,
    window: WindowId,
}

impl TimerHandle {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn window(&self) -> WindowId {
        self.window
    }

    /// 该定时器是否还在表里（一次性定时器触发后即消失）
    pub fn is_active(&self) -> bool {
        self.rt.inner.timers.borrow().iter().any(|t| t.id == self.id)
    }

    /// 取消（幂等）
    ///
    /// **在定时器自己的回调里调用也有效**（D9）：回调执行期间该定时器已被
    /// `take_due_timers` 移出表，所以这里额外打一份"已取消"标记，
    /// 由 `reschedule_timer` 放回前消费。
    pub fn cancel(&self) {
        let mut timers = self.rt.inner.timers.borrow_mut();
        let before = timers.len();
        timers.retain(|t| t.id != self.id);
        if timers.len() == before {
            // 不在表里 ⇒ 正在回调执行中（或已消失）。必须打标记，
            // 否则执行完后 `reschedule_timer` 会把它放回表 ⇒ 周期定时器停不下来。
            drop(timers);
            self.rt.inner.cancelled_timers.borrow_mut().insert(self.id);
        }
    }
}

impl std::fmt::Debug for TimerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimerHandle")
            .field("id", &self.id)
            .field("active", &self.is_active())
            .finish_non_exhaustive()
    }
}

impl Runtime {
    /// 延迟 `dur` 后回调一次（`f` 在 **UI 线程**执行，可拿 `&mut Ctx`）
    pub fn set_timeout<F: FnMut(&mut Ctx) + 'static>(&self, window: WindowId, dur: Duration, f: F) -> TimerHandle {
        self.push_timer(window, dur, None, Box::new(f))
    }

    /// 每 `dur` 回调一次（直到 [`TimerHandle::cancel`]，或窗口关闭）
    pub fn set_interval<F: FnMut(&mut Ctx) + 'static>(&self, window: WindowId, dur: Duration, f: F) -> TimerHandle {
        self.push_timer(window, dur, Some(dur), Box::new(f))
    }

    fn push_timer(
        &self,
        window: WindowId,
        dur: Duration,
        interval: Option<Duration>,
        cb: TimerCallback,
    ) -> TimerHandle {
        let id = {
            let n = self.inner.next_timer_id.get() + 1;
            self.inner.next_timer_id.set(n);
            n
        };
        self.inner.timers.borrow_mut().push(Timer {
            id,
            window,
            deadline: Instant::now() + dur,
            interval,
            cb: Some(cb),
        });
        // 新定时器 ⇒ 唤醒一次，让平台层重算 ControlFlow
        self.wake();
        TimerHandle {
            rt: self.clone(),
            id,
            window,
        }
    }

    /// 请求**下一帧**调用 `ViewModel::on_animation`（经典 RAF 语义：想继续就在回调里再请求）
    pub fn request_animation(&self, window: WindowId) {
        let mut list = self.inner.animating.borrow_mut();
        if !list.contains(&window) {
            list.push(window);
        }
        drop(list);
        self.wake();
    }

    /// 下一帧是否已排了动画回调
    pub fn animation_pending(&self, window: WindowId) -> bool {
        self.inner.animating.borrow().contains(&window)
    }

    /// 取走"下一帧要跑动画"的标记（`WindowCtx::tick` 调用）
    pub(crate) fn take_animation_request(&self, window: WindowId) -> bool {
        let mut list = self.inner.animating.borrow_mut();
        match list.iter().position(|w| *w == window) {
            Some(i) => {
                list.remove(i);
                true
            }
            None => false,
        }
    }

    /// 取走已到期的定时器（回调在表外执行 ⇒ 回调里能安全地再设定时器）
    pub(crate) fn take_due_timers(&self, window: WindowId, now: Instant) -> Vec<Timer> {
        let mut timers = self.inner.timers.borrow_mut();
        let mut due = Vec::new();
        let mut i = 0;
        while i < timers.len() {
            if timers[i].window == window && timers[i].deadline <= now {
                due.push(timers.remove(i));
            } else {
                i += 1;
            }
        }
        due
    }

    /// 周期定时器回到表里（一次性定时器到此结束）
    ///
    /// 放回**之前**做两道检查（D9）：
    /// 1. **回调执行期间被 `cancel()`** ⇒ 丢弃（否则周期定时器永远停不下来）；
    /// 2. **所属窗口已注销** ⇒ 丢弃（否则关窗后它被放回表里，既永不触发
    ///    又一直持有闭包捕获 ⇒ 泄漏）。
    pub(crate) fn reschedule_timer(&self, mut timer: Timer, now: Instant) {
        // ① 取消名单（消费即移除，保持集合小）
        if self.inner.cancelled_timers.borrow_mut().remove(&timer.id) {
            return;
        }
        // ② 窗口还在吗？（关窗后放回 ⇒ 孤儿）
        if !self.windows().contains(&timer.window) {
            return;
        }
        let Some(interval) = timer.interval else {
            return; // 一次性：跑完即消失
        };
        let Some(cb) = timer.take_cb() else {
            return; // 回调被 take 走后没还回来（不该发生）
        };
        timer.cb = Some(cb);
        // 按"上次计划时刻"累加，避免回调耗时导致漂移
        timer.deadline += interval.max(Duration::from_millis(1));
        if timer.deadline <= now {
            timer.deadline = now + interval.max(Duration::from_millis(1));
        }
        self.inner.timers.borrow_mut().push(timer);
    }

    /// 某窗口的下一个时钟事件时刻（定时器到期 / 动画帧），供帧调度取最早值
    pub fn next_deadline(&self, window: WindowId) -> Option<Instant> {
        let due = self
            .inner
            .timers
            .borrow()
            .iter()
            .filter(|t| t.window == window)
            .map(|t| t.deadline)
            .min();
        let anim = self.animation_pending(window).then(|| Instant::now() + FRAME_PERIOD);
        match (due, anim) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// 清掉某窗口的定时器与动画请求（窗口关闭时调用）
    pub(crate) fn cancel_timers_of(&self, window: WindowId) {
        // 先把该窗口所有定时器 id 记进"取消名单"，再从表里删。
        // 名单用于堵住这个洞：**正在执行回调**的那个定时器此刻不在表里，
        // `retain` 删不到它；回调返回后 `reschedule_timer` 会把它放回去
        // ⇒ 变成永不触发、却一直持有闭包捕获的孤儿（D9）。
        {
            let mut timers = self.inner.timers.borrow_mut();
            let mut cancelled = self.inner.cancelled_timers.borrow_mut();
            for t in timers.iter().filter(|t| t.window == window) {
                cancelled.insert(t.id);
            }
            timers.retain(|t| t.window != window);
        }
        self.inner.animating.borrow_mut().retain(|w| *w != window);
    }

    /// 当前定时器数量（测试 / 调试）
    pub fn timer_count(&self) -> usize {
        self.inner.timers.borrow().len()
    }
}

/// 便于测试：把该窗口的定时器**立即**置为到期（不改真实时钟）
#[cfg(test)]
pub(crate) fn force_due(rt: &Runtime, window: WindowId) {
    let mut timers = rt.inner.timers.borrow_mut();
    for t in timers.iter_mut() {
        if t.window == window {
            t.deadline = Instant::now() - Duration::from_millis(1);
        }
    }
}

#[cfg(test)]
#[path = "timer_tests.rs"]
mod tests;
