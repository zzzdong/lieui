use std::num::NonZeroU32;
use std::rc::Rc;

use vello_cpu::Pixmap;
use winit::application::ApplicationHandler;
use winit::event::{
    ElementState, KeyEvent, Modifiers as WinitModifiers, MouseButton as WinitMouseButton,
    MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey};
use winit::window::{Window, WindowId};

use crate::core::ViewContext;
use crate::event::{Key, Modifiers, MouseButton};
use crate::geometry::{Point, Size};
use crate::render::VelloRenderer;

pub struct App {
    view: ViewContext,
    window: Option<Rc<Window>>,
    pixmap: Pixmap,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    renderer: VelloRenderer,
    /// 当前鼠标位置
    mouse_position: Point,
    /// 当前键盘修饰键状态
    modifiers: WinitModifiers,
}

impl App {
    pub fn new(view: ViewContext) -> Self {
        // 先使用默认大小创建渲染器，后续会在 init_window 中重新创建
        let renderer = VelloRenderer::new(800, 600);

        Self {
            view,
            window: None,
            pixmap: Pixmap::new(800, 600),
            surface: None,
            renderer,
            mouse_position: Point::zero(),
            modifiers: WinitModifiers::default(),
        }
    }

    fn init_window(&mut self, event_loop: &ActiveEventLoop) {
        let window = Rc::new(
            event_loop
                .create_window(
                    winit::window::WindowAttributes::default()
                        .with_title("LieUI App")
                        .with_inner_size(winit::dpi::LogicalSize::new(800, 600)),
                )
                .unwrap(),
        );

        // 启用 IME 支持
        window.set_ime_allowed(true);

        let context = softbuffer::Context::new(window.clone()).unwrap();
        let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

        let size = window.inner_size();
        surface
            .resize(
                NonZeroU32::new(size.width).unwrap(),
                NonZeroU32::new(size.height).unwrap(),
            )
            .unwrap();

        self.window = Some(window);
        self.surface = Some(surface);

        self.view
            .set_viewport(Size::new(size.width as f32, size.height as f32));
        self.pixmap = Pixmap::new(size.width as u16, size.height as u16);

        // 使用正确的窗口大小重新创建渲染器
        self.renderer = VelloRenderer::new(size.width as u16, size.height as u16);

        // Initial render
        self.render_and_present();
    }

    fn render_and_present(&mut self) {
        if self.window.is_none() || self.surface.is_none() {
            return;
        }

        // 确保 pixmap 尺寸与 surface 匹配（在借用 surface 之前）
        self.ensure_pixmap_size();

        let _window = self.window.as_ref().unwrap();
        let surface = self.surface.as_mut().unwrap();

        // Render to pixmap using new VisualElement API
        let elements = self.view.render();

        if self.view.debug_render_tree {
            // 暂时禁用 to_xml，因为现在返回的是 Vec<VisualElement>
            // log::debug!("{}", ...);
        }

        // 使用新的渲染器 API
        if !elements.is_empty() {
            self.pixmap = self.renderer.render(&elements);
        }

        // Present to window - use pixmap dimensions to ensure correct mapping
        let width = self.pixmap.width() as u32;
        let height = self.pixmap.height() as u32;

        if width > 0 && height > 0 {
            let mut buffer = surface.buffer_mut().unwrap();
            let pixmap_data = self.pixmap.data();

            // Convert RGBA to ARGB for softbuffer
            for y in 0..height {
                for x in 0..width {
                    let idx = (y * width + x) as usize;
                    if idx < pixmap_data.len() {
                        let pixel = pixmap_data[idx];
                        let r = pixel.r as u32;
                        let g = pixel.g as u32;
                        let b = pixel.b as u32;
                        let a = pixel.a as u32;
                        // ARGB format for softbuffer
                        let argb_pixel = (a << 24) | (r << 16) | (g << 8) | b;
                        buffer[idx] = argb_pixel;
                    }
                }
            }

            buffer.present().unwrap();
        }
    }

    fn handle_resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        if size.width > 0 && size.height > 0 {
            if let Some(surface) = self.surface.as_mut() {
                surface
                    .resize(
                        NonZeroU32::new(size.width).unwrap(),
                        NonZeroU32::new(size.height).unwrap(),
                    )
                    .unwrap();
            }
            self.pixmap = Pixmap::new(size.width as u16, size.height as u16);
            self.view
                .set_viewport(Size::new(size.width as f32, size.height as f32));
            self.render_and_present();
        }
    }

    pub fn run(mut self, event_loop: EventLoop<()>) {
        let _ = event_loop.run_app(&mut self);
    }

    /// 更新 IME 位置到当前焦点 widget
    fn update_ime_position(&self) {
        if let Some(window) = self.window.as_ref()
            && let Some(focused) = self.view.focused_widget()
            && let Some(bounds) = self.view.widget_bounds(focused)
        {
            // 设置 IME 位置到 widget 的左下角
            let position = winit::dpi::LogicalPosition::new(
                bounds.x as f64,
                (bounds.y + bounds.height) as f64,
            );
            window.set_ime_cursor_area(
                position,
                winit::dpi::LogicalSize::new(bounds.width as f64, bounds.height as f64),
            );
        }
    }

    /// 确保 pixmap 尺寸与 surface 匹配
    fn ensure_pixmap_size(&mut self) {
        if let Some(surface) = self.surface.as_mut() {
            let buffer = surface.buffer_mut().unwrap();
            let (width, height) = (buffer.width().get(), buffer.height().get());
            drop(buffer);

            let expected_width = width as u16;
            let expected_height = height as u16;

            if self.pixmap.width() != expected_width || self.pixmap.height() != expected_height {
                self.pixmap = Pixmap::new(expected_width, expected_height);
                self.renderer = VelloRenderer::new(expected_width, expected_height);
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            self.init_window(event_loop);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.as_ref().map(|w| w.id()) != Some(window_id) {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::Resized(size) => {
                self.handle_resize(size);
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_position = Point::new(position.x as f32, position.y as f32);
                self.view.handle_mouse_move(self.mouse_position);
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    WinitMouseButton::Left => MouseButton::Left,
                    WinitMouseButton::Right => MouseButton::Right,
                    WinitMouseButton::Middle => MouseButton::Middle,
                    _ => return,
                };
                match state {
                    ElementState::Pressed => {
                        self.view.handle_mouse_down(self.mouse_position, button);
                        // 焦点变化后，更新 IME 位置
                        self.update_ime_position();
                    }
                    ElementState::Released => {
                        self.view.handle_mouse_up(self.mouse_position, button)
                    }
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let (delta_x, delta_y) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x * 20.0, y * 20.0),
                    MouseScrollDelta::PixelDelta(pos) => (pos.x as f32, pos.y as f32),
                };
                self.view
                    .handle_mouse_wheel(delta_x, delta_y, self.mouse_position);
            }

            WindowEvent::ModifiersChanged(new_modifiers) => {
                self.modifiers = new_modifiers;
            }

            WindowEvent::KeyboardInput { event, .. } => {
                let key = winit_key_to_key(&event);
                let modifiers = winit_modifiers_to_modifiers(self.modifiers);
                match event.state {
                    ElementState::Pressed => self.view.handle_key_down(key, modifiers),
                    ElementState::Released => self.view.handle_key_up(key, modifiers),
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }

            WindowEvent::Ime(ime) => {
                use winit::event::Ime;
                match ime {
                    Ime::Preedit(text, cursor) => {
                        let (start, end) = cursor
                            .map(|(s, e)| (Some(s), Some(e)))
                            .unwrap_or((None, None));
                        self.view.handle_ime_preedit(text.to_string(), start, end);
                    }
                    Ime::Commit(text) => {
                        self.view.handle_ime_commit(text.to_string());
                    }
                    Ime::Disabled => {
                        self.view.handle_ime_disabled();
                    }
                    _ => {}
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }

            WindowEvent::RedrawRequested => {
                self.render_and_present();
            }

            _ => {}
        }
    }
}

fn winit_key_to_key(event: &KeyEvent) -> Key {
    match &event.logical_key {
        WinitKey::Named(NamedKey::Enter) => Key::Enter,
        WinitKey::Named(NamedKey::Escape) => Key::Escape,
        WinitKey::Named(NamedKey::Backspace) => Key::Backspace,
        WinitKey::Named(NamedKey::Delete) => Key::Delete,
        WinitKey::Named(NamedKey::Tab) => Key::Tab,
        WinitKey::Named(NamedKey::Space) => Key::Space,
        WinitKey::Named(NamedKey::Home) => Key::Home,
        WinitKey::Named(NamedKey::End) => Key::End,
        WinitKey::Named(NamedKey::PageUp) => Key::PageUp,
        WinitKey::Named(NamedKey::PageDown) => Key::PageDown,
        WinitKey::Named(NamedKey::ArrowUp) => Key::ArrowUp,
        WinitKey::Named(NamedKey::ArrowDown) => Key::ArrowDown,
        WinitKey::Named(NamedKey::ArrowLeft) => Key::ArrowLeft,
        WinitKey::Named(NamedKey::ArrowRight) => Key::ArrowRight,
        WinitKey::Named(NamedKey::Shift) => Key::Shift,
        WinitKey::Named(NamedKey::Control) => Key::Ctrl,
        WinitKey::Named(NamedKey::Alt) => Key::Alt,
        WinitKey::Named(NamedKey::Meta) => Key::Meta,
        WinitKey::Character(c) => Key::Character(c.chars().next().unwrap_or('\0')),
        _ => Key::Unknown,
    }
}

fn winit_modifiers_to_modifiers(modifiers: WinitModifiers) -> Modifiers {
    Modifiers {
        shift: modifiers.state().shift_key(),
        ctrl: modifiers.state().control_key(),
        alt: modifiers.state().alt_key(),
        meta: modifiers.state().super_key(),
    }
}
