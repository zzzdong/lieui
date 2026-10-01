//! 主题 —— design token → 组件配色（设计 §3.10）。
//!
//! ## 机制（刻意简单）
//!
//! - `Theme` 是 [`Runtime`](crate::reactive::Runtime) 的普通字段（无 thread_local、无属性表/继承）；
//! - `view()` 前，框架把主题**快照**注入 `ViewBuf`，DSL 构造 widget 时把 token **烘焙**进节点样式
//!   （用户链式覆盖仍可赢）；
//! - 少数绘制期颜色（光标/选区/滚动条/未覆盖的 accent 兜底）走 `SceneOptions.theme`，
//!   由渲染管线每帧带上；
//! - 切换主题 = `Runtime::set_theme` ⇒ 所有窗口重跑 `view()` + 全屏重绘。
//!   （设计稿 §3.10 的原文方案，无额外机制。）
//!
//! ## 用法
//!
//! ```ignore
//! let rt = Runtime::new();                 // 默认 light
//! rt.set_theme(Theme::dark());             // 全局切换：重跑 view + 整窗重绘
//! ```

use lieui_geom::Color;

/// design token 集。字段就是语义（不是颜色名）：改主题 = 换这份结构。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Theme {
    /// 窗口/容器底色（新建窗口与主题切换时应用）
    pub window_background: Color,
    /// 正文文字
    pub text: Color,
    /// 次要文字（占位提示等）
    pub text_secondary: Color,
    /// 强调色（滑块/进度条/复选框勾选，取自组件未覆盖时的兜底）
    pub accent: Color,
    /// 控件底色（按钮）
    pub control: Color,
    pub control_hover: Color,
    pub control_pressed: Color,
    /// 控件边框（输入框、复选框描边）
    pub control_border: Color,
    /// 输入框底色
    pub input_background: Color,
    /// 文本选区高亮（半透明）
    pub selection: Color,
    /// 输入光标
    pub caret: Color,
    /// 滚动条 thumb
    pub scrollbar_thumb: Color,
    pub scrollbar_thumb_drag: Color,
    /// 悬停提示（tooltip）底色与文字
    pub tooltip_background: Color,
    pub tooltip_text: Color,
    /// 控件圆角半径
    pub control_radius: f32,
}

impl Theme {
    /// 浅色主题（默认）
    pub fn light() -> Self {
        Self {
            window_background: Color::new(245, 245, 245),
            text: Color::new(32, 32, 32),
            text_secondary: Color::new(140, 140, 140),
            accent: Color::new(90, 120, 220),
            control: Color::new(228, 228, 228),
            control_hover: Color::new(212, 212, 212),
            control_pressed: Color::new(196, 196, 196),
            control_border: Color::new(180, 180, 180),
            input_background: Color::WHITE,
            selection: Color::rgba(90, 140, 240, 90),
            caret: Color::new(30, 30, 30),
            scrollbar_thumb: Color::rgba(120, 120, 120, 110),
            scrollbar_thumb_drag: Color::rgba(90, 90, 90, 170),
            tooltip_background: Color::rgba(0x1f, 0x29, 0x37, 0xf2),
            tooltip_text: Color::WHITE,
            control_radius: 4.0,
        }
    }

    /// 深色主题
    pub fn dark() -> Self {
        Self {
            window_background: Color::new(32, 32, 36),
            text: Color::new(230, 230, 230),
            text_secondary: Color::new(140, 140, 148),
            accent: Color::new(110, 150, 250),
            control: Color::new(58, 58, 64),
            control_hover: Color::new(70, 70, 78),
            control_pressed: Color::new(48, 48, 54),
            control_border: Color::new(90, 90, 98),
            input_background: Color::new(48, 48, 54),
            selection: Color::rgba(90, 140, 240, 120),
            caret: Color::new(230, 230, 230),
            scrollbar_thumb: Color::rgba(180, 180, 180, 110),
            scrollbar_thumb_drag: Color::rgba(200, 200, 200, 170),
            tooltip_background: Color::rgba(0x0b, 0x0f, 0x14, 0xf5),
            tooltip_text: Color::new(230, 230, 230),
            control_radius: 4.0,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_differ_meaningfully() {
        let l = Theme::light();
        let d = Theme::dark();
        assert_ne!(l.window_background, d.window_background);
        assert_ne!(l.text, d.text);
        assert_ne!(l.input_background, d.input_background);
        // 半透明 token 保持半透明
        assert!(l.selection.a < 255 && d.selection.a < 255);
        assert!(l.scrollbar_thumb.a < 255);
    }

    #[test]
    fn default_is_light() {
        assert_eq!(Theme::default(), Theme::light());
    }
}
