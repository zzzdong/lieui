//! Application — Builder 驱动的应用封装
//! 连接用户 Builder 闭包与 Runtime 的桥梁。

use crate::geometry::Size;
use crate::render::visual::LayeredElement;
use crate::runtime::Runtime;
use crate::state;
use crate::view::node::ViewNode;
use crate::runtime::DebugStats;

pub struct Application {
    runtime: Runtime,
}

impl Application {
    pub fn new(viewport: Size) -> Self {
        Self { runtime: Runtime::new(viewport) }
    }

    pub fn set_viewport(&mut self, size: Size) { self.runtime.set_viewport(size); }
    pub fn runtime(&mut self) -> &mut Runtime { &mut self.runtime }

    /// 运行一次 Builder → Runtime 帧循环
    pub fn run_once<F: Fn() -> ViewNode>(&mut self, build_fn: F) -> Vec<LayeredElement> {
        let view_tree = build_fn();
        self.runtime.submit_view_tree(view_tree);
        self.runtime.frame()
    }

    /// 只在状态变化时重建
    pub fn rebuild_if_needed<F: Fn() -> ViewNode>(&mut self, build_fn: F) -> Vec<LayeredElement> {
        if state::take_rebuild_requested() { self.run_once(build_fn) } else { Vec::new() }
    }

    pub fn debug_stats(&self) -> &DebugStats { &self.runtime.debug_stats }
}
