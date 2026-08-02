//! WindowConfig — 窗口配置（标题、图标、尺寸、限制等）
//!
//! 通过 builder 模式设置，传递给 [`crate::app::Application::new`] 或
//! [`crate::app::Application::window`] 以配置窗口。

use std::path::PathBuf;

/// 窗口配置。
///
/// 提供标题、图标、初始尺寸、最小/最大尺寸、装饰等选项。
///
/// ```ignore
/// WindowConfig::new()
///     .title("我的应用")
///     .size(1024.0, 768.0)
///     .min_size(600.0, 400.0)
///     .max_size(1920.0, 1440.0)
///     .resizable(true)
///     .decorations(true)
///     .icon("assets/icon.png")
///     .always_on_top(false)
///     .position(100, 100);
/// ```
#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub title: String,
    pub icon: Option<PathBuf>,
    /// 初始宽度、高度（逻辑像素）。
    pub size: (f64, f64),
    pub min_size: Option<(f64, f64)>,
    pub max_size: Option<(f64, f64)>,
    pub resizable: bool,
    pub decorations: bool,
    pub always_on_top: bool,
    pub position: Option<(i32, i32)>,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "LieUI".to_string(),
            icon: None,
            size: (800.0, 600.0),
            min_size: None,
            max_size: None,
            resizable: true,
            decorations: true,
            always_on_top: false,
            position: None,
        }
    }
}

impl WindowConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// 窗口标题。
    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = t.into();
        self
    }

    /// 窗口图标文件路径（如 `.png`）。
    pub fn icon(mut self, path: impl Into<PathBuf>) -> Self {
        self.icon = Some(path.into());
        self
    }

    /// 初始窗口大小（逻辑像素）。
    pub fn size(mut self, w: f64, h: f64) -> Self {
        self.size = (w, h);
        self
    }

    /// 最小窗口尺寸。
    pub fn min_size(mut self, w: f64, h: f64) -> Self {
        self.min_size = Some((w, h));
        self
    }

    /// 最大窗口尺寸。
    pub fn max_size(mut self, w: f64, h: f64) -> Self {
        self.max_size = Some((w, h));
        self
    }

    /// 是否允许调整大小（默认 true）。
    pub fn resizable(mut self, v: bool) -> Self {
        self.resizable = v;
        self
    }

    /// 是否显示窗口装饰（标题栏、边框等，默认 true）。
    pub fn decorations(mut self, v: bool) -> Self {
        self.decorations = v;
        self
    }

    /// 是否置顶（默认 false）。
    pub fn always_on_top(mut self, v: bool) -> Self {
        self.always_on_top = v;
        self
    }

    /// 初始窗口位置（屏幕坐标）。
    pub fn position(mut self, x: i32, y: i32) -> Self {
        self.position = Some((x, y));
        self
    }
}
