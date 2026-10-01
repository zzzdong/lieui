//! 窗口身份。
//!
//! M1 只定义框架自己的 `WindowId`（不引入 winit）；M4 接 winit 时在其
//! `WindowId` 与我们的 id 之间做一次映射，其余代码不感知。

/// 框架内的窗口身份（`u32`，由 App 分配，从 1 开始）
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct WindowId(pub u32);

impl WindowId {
    pub const fn new(v: u32) -> Self {
        Self(v)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for WindowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WindowId({})", self.0)
    }
}
