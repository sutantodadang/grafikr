//! Native window shell: winit event loop, wgpu surfaces, clipboard, IME, AccessKit.

use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};

use accesskit_winit::Adapter;
use grafikr_core::{Clipboard, Key, Modifiers, MouseButton, Size, Theme, UiEvent, View};
use grafikr_render::Renderer;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, KeyEvent, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{CursorIcon as WinitCursorIcon, Window, WindowId};

type Factory = Box<dyn Fn() -> Box<dyn View>>;

struct WindowSpec {
    title: String,
    size: (f64, f64),
    factory: Factory,
}

/// Application entry point. Create with [`App::new`], then [`App::run`].
pub struct App {
    windows: Vec<WindowSpec>,
    theme: Theme,
}

impl App {
    pub fn new<V: View + 'static>(factory: impl Fn() -> V + 'static) -> Self {
        Self {
            windows: vec![WindowSpec {
                title: "Grafikr".to_owned(),
                size: (960.0, 600.0),
                factory: Box::new(move || Box::new(factory())),
            }],
            theme: Theme::dark(),
        }
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Title of the first window.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        if let Some(window) = self.windows.first_mut() {
            window.title = title.into();
        }
        self
    }

    /// Logical size of the first window.
    pub fn size(mut self, width: f64, height: f64) -> Self {
        if let Some(window) = self.windows.first_mut() {
            window.size = (width, height);
        }
        self
    }

    /// Add another top-level window with its own view tree and state.
    pub fn window<V: View + 'static>(
        mut self,
        title: impl Into<String>,
        width: f64,
        height: f64,
        factory: impl Fn() -> V + 'static,
    ) -> Self {
        self.windows.push(WindowSpec {
            title: title.into(),
            size: (width, height),
            factory: Box::new(move || Box::new(factory())),
        });
        self
    }

    pub fn run(self) -> Result<(), Box<dyn Error>> {
        let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
        event_loop.set_control_flow(ControlFlow::Wait);
        let proxy = event_loop.create_proxy();
        let mut shell = Shell::new(self, proxy);
        event_loop.run_app(&mut shell)?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum UserEvent {
    AccessKit(accesskit_winit::Event),
    DeviceLost,
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::AccessKit(event)
    }
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
}

impl Gpu {
    fn new(
        window: &Arc<Window>,
        proxy: &EventLoopProxy<UserEvent>,
    ) -> Result<Self, Box<dyn Error>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            ..Default::default()
        }))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("grafikr"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            }))?;
        let proxy = proxy.clone();
        device.set_device_lost_callback(move |reason, message| {
            if reason == wgpu::DeviceLostReason::Destroyed {
                return;
            }
            log::error!("grafikr: device lost ({reason:?}): {message}");
            let _ = proxy.send_event(UserEvent::DeviceLost);
        });
        let physical = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, physical.width.max(1), physical.height.max(1))
            .ok_or("surface is not supported by the selected adapter")?;
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);
        let renderer = Renderer::new(&device, config.format);
        Ok(Self {
            device,
            queue,
            surface,
            config,
            renderer,
        })
    }
}

struct WindowState {
    window: Arc<Window>,
    gpu: Gpu,
    proxy: EventLoopProxy<UserEvent>,
    ui: grafikr_core::Ui,
    a11y: Adapter,
    modifiers: Modifiers,
    pointer: grafikr_core::Point,
    scale: f32,
    cursor: WinitCursorIcon,
    ime_active: bool,
}

impl WindowState {
    fn new(
        event_loop: &ActiveEventLoop,
        spec: WindowSpec,
        theme: &Theme,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Result<Self, Box<dyn Error>> {
        let attributes = Window::default_attributes()
            .with_title(spec.title.clone())
            .with_inner_size(LogicalSize::new(spec.size.0, spec.size.1))
            .with_visible(false);
        let window = Arc::new(event_loop.create_window(attributes)?);
        let scale = window.scale_factor() as f32;
        let gpu = Gpu::new(&window, &proxy)?;
        let physical = window.inner_size();
        let logical = physical.to_logical::<f32>(scale as f64);
        let mut ui = grafikr_core::Ui::new(
            theme.clone(),
            scale,
            Size::new(logical.width, logical.height),
            spec.title.clone(),
        );
        ui.clipboard = Box::new(SystemClipboard::new());
        let view = (spec.factory)();
        ui.mount(view.as_ref());
        let a11y = Adapter::with_event_loop_proxy(event_loop, &window, proxy.clone());
        window.set_visible(true);
        Ok(Self {
            window,
            gpu,
            proxy,
            ui,
            a11y,
            modifiers: Modifiers::default(),
            pointer: grafikr_core::Point::ZERO,
            scale,
            cursor: WinitCursorIcon::Default,
            ime_active: false,
        })
    }

    /// Rebuild the swapchain, device, and renderer after device/GPU loss.
    fn recover_gpu(&mut self) {
        match Gpu::new(&self.window, &self.proxy) {
            Ok(gpu) => {
                self.gpu = gpu;
                self.resize();
            }
            Err(error) => {
                log::error!("grafikr: gpu recovery failed: {error}");
                self.window.request_redraw();
            }
        }
    }

    fn resize(&mut self) {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.gpu.config.width = size.width;
        self.gpu.config.height = size.height;
        self.gpu
            .surface
            .configure(&self.gpu.device, &self.gpu.config);
        let logical = size.to_logical::<f32>(self.scale as f64);
        self.ui
            .set_viewport(Size::new(logical.width, logical.height));
        self.window.request_redraw();
    }

    fn apply_outcome(&mut self, outcome: grafikr_core::Outcome) {
        let icon = match outcome.cursor {
            grafikr_core::CursorIcon::Default => WinitCursorIcon::Default,
            grafikr_core::CursorIcon::Pointer => WinitCursorIcon::Pointer,
            grafikr_core::CursorIcon::Text => WinitCursorIcon::Text,
        };
        if icon != self.cursor {
            self.window.set_cursor(icon);
            self.cursor = icon;
        }
        if outcome.ime_allowed != self.ime_active {
            self.window.set_ime_allowed(outcome.ime_allowed);
            self.ime_active = outcome.ime_allowed;
        }
        if outcome.ime_allowed
            && let Some(rect) = self.ui.ime_cursor_rect()
        {
            self.window.set_ime_cursor_area(
                PhysicalPosition::new((rect.x * self.scale) as i32, (rect.y * self.scale) as i32),
                PhysicalSize::new(
                    (rect.width * self.scale).max(1.0) as u32,
                    (rect.height * self.scale).max(1.0) as u32,
                ),
            );
        }
        if outcome.repaint {
            self.window.request_redraw();
        }
    }

    fn update_a11y(&mut self, force: bool) {
        if !force && !self.ui.a11y_dirty {
            return;
        }
        self.a11y.update_if_active(|| self.ui.a11y_update());
        self.ui.a11y_dirty = false;
    }

    fn redraw(&mut self) {
        self.ui.prepare_frame();
        self.update_a11y(false);
        let frame = match self.gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.gpu
                    .surface
                    .configure(&self.gpu.device, &self.gpu.config);
                self.window.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return,
            wgpu::CurrentSurfaceTexture::Validation => {
                self.recover_gpu();
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.gpu.renderer.render(
            &self.gpu.device,
            &self.gpu.queue,
            &view,
            (self.gpu.config.width, self.gpu.config.height),
            self.scale,
            self.ui.theme.background,
            &self.ui.display,
        );
        self.gpu.queue.present(frame);
    }

    fn handle_key(&mut self, event: &KeyEvent) {
        let key = match &event.logical_key {
            WinitKey::Named(named) => match named {
                NamedKey::Tab => Some(Key::Tab),
                NamedKey::Enter => Some(Key::Enter),
                NamedKey::Space => Some(Key::Space),
                NamedKey::Escape => Some(Key::Escape),
                NamedKey::Backspace => Some(Key::Backspace),
                NamedKey::Delete => Some(Key::Delete),
                NamedKey::ArrowLeft => Some(Key::Left),
                NamedKey::ArrowRight => Some(Key::Right),
                NamedKey::ArrowUp => Some(Key::Up),
                NamedKey::ArrowDown => Some(Key::Down),
                NamedKey::PageUp => Some(Key::PageUp),
                NamedKey::PageDown => Some(Key::PageDown),
                NamedKey::Home => Some(Key::Home),
                NamedKey::End => Some(Key::End),
                _ => None,
            },
            WinitKey::Character(chars) => chars
                .chars()
                .next()
                .map(|c| Key::Character(c.to_ascii_lowercase())),
            _ => None,
        };
        let Some(key) = key else {
            return;
        };

        if event.state == ElementState::Pressed {
            let outcome = self.ui.handle_event(UiEvent::KeyDown {
                key,
                modifiers: self.modifiers,
                repeat: event.repeat,
            });
            let printable = event.text.as_ref().is_some_and(|text| {
                !text.is_empty()
                    && text.chars().all(|c| !c.is_control())
                    && !self.modifiers.ctrl
                    && !self.modifiers.alt
                    && !self.modifiers.super_key
            });
            if printable {
                let text = event
                    .text
                    .as_ref()
                    .map(|text| text.to_string())
                    .unwrap_or_default();
                let outcome = self.ui.handle_event(UiEvent::Text(text));
                self.apply_outcome(outcome);
            }
            self.apply_outcome(outcome);
        } else {
            let outcome = self.ui.handle_event(UiEvent::KeyUp {
                key,
                modifiers: self.modifiers,
            });
            self.apply_outcome(outcome);
        }
    }
}

struct Shell {
    app: App,
    proxy: EventLoopProxy<UserEvent>,
    windows: HashMap<WindowId, WindowState>,
    initialized: bool,
}

impl Shell {
    fn new(app: App, proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            app,
            proxy,
            windows: HashMap::new(),
            initialized: false,
        }
    }
}

impl ApplicationHandler<UserEvent> for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        let specs = std::mem::take(&mut self.app.windows);
        for spec in specs {
            let title = spec.title.clone();
            match WindowState::new(event_loop, spec, &self.app.theme, self.proxy.clone()) {
                Ok(state) => {
                    self.windows.insert(state.window.id(), state);
                }
                Err(error) => {
                    eprintln!("grafikr: failed to create window {title:?}: {error}");
                    event_loop.exit();
                    return;
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::CloseRequested | WindowEvent::Destroyed) {
            self.windows.remove(&window_id);
            if self.windows.is_empty() {
                event_loop.exit();
            }
            return;
        }

        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        state.a11y.process_event(&state.window, &event);

        match event {
            WindowEvent::Resized(_) => state.resize(),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                state.scale = scale_factor as f32;
                state.ui.set_scale(state.scale);
                state.resize();
            }
            WindowEvent::RedrawRequested => state.redraw(),
            WindowEvent::CursorMoved { position, .. } => {
                state.pointer = grafikr_core::Point::new(
                    (position.x / state.scale as f64) as f32,
                    (position.y / state.scale as f64) as f32,
                );
                let outcome = state
                    .ui
                    .handle_event(UiEvent::PointerMoved { pos: state.pointer });
                state.apply_outcome(outcome);
            }
            WindowEvent::CursorLeft { .. } => {
                let outcome = state.ui.handle_event(UiEvent::PointerLeft);
                state.apply_outcome(outcome);
            }
            WindowEvent::MouseInput {
                state: element,
                button,
                ..
            } => {
                let button = match button {
                    winit::event::MouseButton::Left => MouseButton::Left,
                    winit::event::MouseButton::Right => MouseButton::Right,
                    winit::event::MouseButton::Middle => MouseButton::Middle,
                    _ => return,
                };
                let event = match element {
                    ElementState::Pressed => UiEvent::PointerDown {
                        pos: state.pointer,
                        button,
                    },
                    ElementState::Released => UiEvent::PointerUp {
                        pos: state.pointer,
                        button,
                    },
                };
                let outcome = state.ui.handle_event(event);
                state.apply_outcome(outcome);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        grafikr_core::Point::new(x * 20.0, -y * 20.0)
                    }
                    MouseScrollDelta::PixelDelta(pos) => {
                        grafikr_core::Point::new(pos.x as f32, pos.y as f32)
                    }
                };
                let outcome = state.ui.handle_event(UiEvent::Wheel { delta });
                state.apply_outcome(outcome);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                state.modifiers = from_modifiers(modifiers.state());
            }
            WindowEvent::KeyboardInput { event, .. } => state.handle_key(&event),
            WindowEvent::Ime(ime) => {
                let event = match ime {
                    Ime::Preedit(text, cursor) => UiEvent::ImePreedit { text, cursor },
                    Ime::Commit(text) => UiEvent::ImeCommit(text),
                    Ime::Enabled | Ime::Disabled => return,
                };
                let outcome = state.ui.handle_event(event);
                state.apply_outcome(outcome);
            }
            WindowEvent::Focused(false) => {
                let outcome = state.ui.handle_event(UiEvent::FocusLost);
                state.apply_outcome(outcome);
                state.window.set_ime_allowed(false);
                state.ime_active = false;
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::AccessKit(event) => {
                let Some(state) = self.windows.get_mut(&event.window_id) else {
                    return;
                };
                match event.window_event {
                    accesskit_winit::WindowEvent::InitialTreeRequested => state.update_a11y(true),
                    accesskit_winit::WindowEvent::ActionRequested(request) => {
                        if state.ui.handle_a11y_action(&request) {
                            let outcome = state.ui.current_outcome();
                            state.apply_outcome(outcome);
                            state.update_a11y(false);
                            state.window.request_redraw();
                        }
                    }
                    accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
                }
            }
            UserEvent::DeviceLost => {
                for state in self.windows.values_mut() {
                    state.recover_gpu();
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let mut animating = false;
        for state in self.windows.values() {
            if state.ui.runtime.needs_frame() {
                state.window.request_redraw();
            }
            if state.ui.runtime.is_animating() {
                animating = true;
            }
        }
        event_loop.set_control_flow(if animating {
            ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(16))
        } else {
            ControlFlow::Wait
        });
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            for state in self.windows.values() {
                if state.ui.runtime.is_animating() {
                    state.window.request_redraw();
                }
            }
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.windows.clear();
    }
}

fn from_modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        ctrl: state.control_key(),
        alt: state.alt_key(),
        super_key: state.super_key(),
    }
}

struct SystemClipboard {
    inner: Option<arboard::Clipboard>,
}

impl SystemClipboard {
    fn new() -> Self {
        Self {
            inner: arboard::Clipboard::new().ok(),
        }
    }
}

impl Clipboard for SystemClipboard {
    fn get_text(&mut self) -> Option<String> {
        self.inner.as_mut()?.get_text().ok()
    }

    fn set_text(&mut self, text: String) {
        if let Some(clipboard) = &mut self.inner {
            let _ = clipboard.set_text(text);
        }
    }
}
