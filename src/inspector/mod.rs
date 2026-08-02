//! LieUI Inspector —— 通过 Chrome DevTools Protocol (CDP) 暴露 **Widget 树**
//!
//! 一期（只读、单向）：实现 CDP 的 `Runtime` / `DOM` 域子集，使 Chrome 的
//! Elements 面板能直接浏览用户编写的 **Widget 树**（而非运行时 ViewNode），
//! 无需任何自定义前端 UI——全部复用 Chrome 原生 DevTools。
//!
//! 交互模型：仅支持在 Chrome 的 Elements 面板中点选节点查看结构。
//! 不做原生 UI 上的鼠标拾取 / 反向高亮（后续版本可扩展）。
//!
//! 数据通道：
//! - `BuildContext` 在 `child()` 调用链中收集 widget 描述树（零侵入，不改 Widget）。
//! - `build_and_render` 每帧把描述树写入 `InspectorShared`（Arc<Mutex>）。
//! - 独立线程跑 WebSocket server（tungstenite，同步，无 async 依赖），
//!   按 CDP JSON-RPC 协议应答 DevTools 的请求。
//!
//! 启用方式：`Application::new().inspector(true)`（需 feature `inspector`）。

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

mod server;

// ============================================================================
// Widget 描述树（在 BuildContext 层收集，零侵入）
// ============================================================================

/// 单个 widget 的只读描述（构建期由 BuildContext 收集）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetDesc {
    /// widget 类型名（如 `Column`/`Button`/`Text`）
    pub name: String,
    /// 稳定路径（如 `Column/0/Button/1`），作为 CDP nodeId 来源
    pub path: String,
    /// 可选文本（Text widget 的内容）
    pub text: Option<String>,
    /// 子 widget
    pub children: Vec<WidgetDesc>,
}

impl WidgetDesc {
    /// 在描述树中按 path 查找节点
    pub fn find_by_path(&self, path: &str) -> Option<&WidgetDesc> {
        if self.path == path {
            return Some(self);
        }
        for c in &self.children {
            if let Some(n) = c.find_by_path(path) {
                return Some(n);
            }
        }
        None
    }
}

/// 构建期收集用的可变节点（BuildContext 内部使用）
pub(crate) struct WidgetDescBuilder {
    pub(crate) node: Rc<RefCell<WidgetDescNode>>,
    pub(crate) stack: Vec<Rc<RefCell<WidgetDescNode>>>,
}

pub(crate) struct WidgetDescNode {
    pub name: String,
    pub path: String,
    pub text: Option<String>,
    pub children: Vec<Rc<RefCell<WidgetDescNode>>>,
}

impl WidgetDescNode {
    /// 冻结为可序列化 `WidgetDesc`
    pub fn freeze(&self) -> WidgetDesc {
        WidgetDesc {
            name: self.name.clone(),
            path: self.path.clone(),
            text: self.text.clone(),
            children: self.children.iter().map(|c| c.borrow().freeze()).collect(),
        }
    }
}

// ============================================================================
// 共享状态（主线程写、server 线程读）
// ============================================================================

pub(crate) struct InspectorShared {
    /// 最新 widget 描述树（None 表示尚未构建）
    pub snapshot: Option<WidgetDesc>,
    /// server 是否已就绪
    pub ready: bool,
}

impl InspectorShared {
    fn new() -> Self {
        Self {
            snapshot: None,
            ready: false,
        }
    }
}

/// Inspector 句柄
pub struct Inspector {
    pub(crate) shared: Arc<Mutex<InspectorShared>>,
    /// 监听端口（供用户复制 chrome-devtools:// 链接）
    pub port: u16,
    thread: Option<std::thread::JoinHandle<()>>,
    /// 停止标志：置位后 server 线程退出
    stop: Arc<AtomicBool>,
}

impl Inspector {
    /// 启动 Inspector server（绑定 127.0.0.1:0 自动分配端口）
    pub fn start() -> Self {
        let shared = Arc::new(Mutex::new(InspectorShared::new()));
        let (port, thread, stop) = server::spawn(shared.clone());
        eprintln!(
            "[lieui-inspector] DevTools URL:\n  devtools://devtools/bundled/inspector.html?ws=127.0.0.1:{}",
            port
        );
        Self {
            shared,
            port,
            thread: Some(thread),
            stop,
        }
    }

    /// 主线程调用：写入最新 widget 描述树
    pub fn set_snapshot(&self, desc: WidgetDesc) {
        if let Ok(mut g) = self.shared.lock() {
            g.snapshot = Some(desc);
        }
    }

    /// 构造 DevTools 连接链接
    pub fn devtools_url(&self) -> String {
        format!(
            "devtools://devtools/bundled/inspector.html?ws=127.0.0.1:{}",
            self.port
        )
    }
}

impl Drop for Inspector {
    fn drop(&mut self) {
        // 置停止标志，server 线程在下一次轮询（100ms 内）检查并退出。
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        // 此时 server 线程已可退出，安全地 join（不会永久挂起）。
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
