// Servo offscreen renderer integration
// This module is only compiled when the servo-browser feature is enabled

use anyhow::{anyhow, Result};
use glutin::config::{Config, ConfigTemplateBuilder};
use glutin::context::{ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext};
use glutin::display::{Display, DisplayApiPreference};
use glutin::prelude::*;
use glutin::surface::{Surface, SurfaceAttributesBuilder, WindowSurface};
use libservo::compositing::windowing::{AnimationState, EmbedderCoordinates, EmbedderMethods, WindowMethods};
use libservo::compositing::CompositeTarget;
use libservo::embedder_traits::EventLoopWaker;
use libservo::servo_config::opts;
use libservo::servo_url::ServoUrl;
use libservo::{gl, Servo};
use log::{debug, error, info, warn};
use parking_lot::Mutex;
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// Size of the offscreen rendering surface
#[derive(Debug, Clone, Copy)]
pub struct RenderSize {
    pub width: u32,
    pub height: u32,
}

impl Default for RenderSize {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
        }
    }
}

/// EventLoopWaker implementation that does nothing
/// In a full integration, this would wake GPUI's event loop
struct NoOpEventLoopWaker;

impl EventLoopWaker for NoOpEventLoopWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(NoOpEventLoopWaker)
    }

    fn wake(&self) {
        // No-op: In a real implementation, this would notify GPUI to check for Servo events
    }
}

/// Embedder delegate for handling Servo callbacks
/// This receives notifications from Servo about page events
struct ServoEmbedder {
    // Store page state, notifications, etc.
}

impl ServoEmbedder {
    fn new() -> Self {
        Self {}
    }
}

// TODO: Uncomment and implement when compiling with Servo
// The ServoDelegate trait is defined by Servo and handles embedder callbacks
/*
impl libservo::servo_delegate::ServoDelegate for ServoEmbedder {
    fn notify_error(&self, msg: String) {
        error!("Servo error: {}", msg);
    }

    fn notify_devtools_server_started(&self, _port: u16) {
        debug!("DevTools server started");
    }

    fn notify_animating_changed(&self, _animating: bool) {
        // Handle animation state changes
    }

    fn load_web_resource(&self, _url: String) -> Option<Vec<u8>> {
        // Load web resources (fonts, etc)
        None
    }

    fn show_notification(&self, _title: String, _body: String) {
        // Show browser notifications in GPUI
    }
}
*/

/// Window implementation for Servo offscreen rendering
pub struct ServoWindow {
    gl: Rc<dyn gl::Gl>,
    size: RenderSize,
    hidpi_factor: f32,
}

impl ServoWindow {
    fn new(gl: Rc<dyn gl::Gl>, size: RenderSize) -> Self {
        Self {
            gl,
            size,
            hidpi_factor: 1.0,
        }
    }
}

impl WindowMethods for ServoWindow {
    fn get_coordinates(&self) -> EmbedderCoordinates {
        let size = self.size;
        EmbedderCoordinates {
            viewport: euclid::Rect::new(
                euclid::Point2D::new(0, 0),
                euclid::Size2D::new(size.width as i32, size.height as i32),
            ),
            framebuffer: euclid::Size2D::new(size.width as i32, size.height as i32),
            window: (euclid::Size2D::new(size.width as i32, size.height as i32), euclid::Point2D::new(0, 0)),
            screen: euclid::Size2D::new(size.width as i32, size.height as i32),
            screen_avail: euclid::Size2D::new(size.width as i32, size.height as i32),
            hidpi_factor: euclid::Scale::new(self.hidpi_factor),
        }
    }

    fn set_animation_state(&self, _state: AnimationState) {
        // No-op for offscreen rendering
    }

    fn get_gl_context(&self) -> Rc<dyn gl::Gl> {
        self.gl.clone()
    }

    fn get_gl_api(&self) -> gl::GlType {
        gl::GlType::Gl
    }
}

/// Servo offscreen renderer
pub struct ServoRenderer {
    servo: Option<Servo>,
    gl_context: Option<PossiblyCurrentContext>,
    gl_surface: Option<Surface<WindowSurface>>,
    gl_display: Option<Display>,
    gl: Option<Rc<dyn gl::Gl>>,
    size: RenderSize,
    current_url: Option<String>,
    // Framebuffer for offscreen rendering
    framebuffer_id: Option<u32>,
    texture_id: Option<u32>,
}

impl ServoRenderer {
    /// Create a new ServoRenderer
    pub fn new() -> Result<Self> {
        Ok(Self {
            servo: None,
            gl_context: None,
            gl_surface: None,
            gl_display: None,
            gl: None,
            size: RenderSize::default(),
            current_url: None,
            framebuffer_id: None,
            texture_id: None,
        })
    }

    /// Initialize the renderer with an OpenGL context
    /// This must be called before using the renderer
    pub fn initialize(&mut self, size: RenderSize) -> Result<()> {
        info!("Initializing Servo renderer with size {:?}", size);
        self.size = size;

        // Create glutin display for offscreen rendering
        let display = create_offscreen_display()?;

        // Create OpenGL context
        let (context, surface, config) = create_gl_context(&display, size)?;

        // Make context current
        let context = context.make_current(&surface)?;

        // Load GL functions
        let gl = load_gl_functions(&display, &config);

        self.gl_display = Some(display);
        self.gl_context = Some(context);
        self.gl_surface = Some(surface);
        self.gl = Some(gl.clone());

        // Create framebuffer and texture for offscreen rendering
        self.create_framebuffer(size)?;

        // Initialize Servo
        self.initialize_servo(gl.clone(), size)?;

        Ok(())
    }

    /// Create OpenGL framebuffer and texture for offscreen rendering
    fn create_framebuffer(&mut self, size: RenderSize) -> Result<()> {
        let gl = self.gl.as_ref().ok_or_else(|| anyhow!("GL not initialized"))?;

        unsafe {
            // Generate texture
            let mut texture_id: u32 = 0;
            gl.gen_textures(1, &mut texture_id);
            gl.bind_texture(gl::TEXTURE_2D, texture_id);

            // Set texture parameters
            gl.tex_parameter_i(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl.tex_parameter_i(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            gl.tex_parameter_i(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);

            // Allocate texture storage
            gl.tex_image_2d(
                gl::TEXTURE_2D,
                0,
                gl::RGBA as i32,
                size.width as i32,
                size.height as i32,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                None,
            );

            // Generate framebuffer
            let mut framebuffer_id: u32 = 0;
            gl.gen_framebuffers(1, &mut framebuffer_id);
            gl.bind_framebuffer(gl::FRAMEBUFFER, framebuffer_id);

            // Attach texture to framebuffer
            gl.framebuffer_texture_2d(
                gl::FRAMEBUFFER,
                gl::COLOR_ATTACHMENT0,
                gl::TEXTURE_2D,
                texture_id,
                0,
            );

            // Check framebuffer status
            let status = gl.check_framebuffer_status(gl::FRAMEBUFFER);
            if status != gl::FRAMEBUFFER_COMPLETE {
                return Err(anyhow!("Framebuffer not complete: {}", status));
            }

            // Unbind framebuffer
            gl.bind_framebuffer(gl::FRAMEBUFFER, 0);
            gl.bind_texture(gl::TEXTURE_2D, 0);

            self.texture_id = Some(texture_id);
            self.framebuffer_id = Some(framebuffer_id);

            info!("Created framebuffer {} with texture {}", framebuffer_id, texture_id);
        }

        Ok(())
    }

    /// Initialize Servo engine
    fn initialize_servo(&mut self, gl: Rc<dyn gl::Gl>, size: RenderSize) -> Result<()> {
        info!("Initializing Servo engine");

        // Set up Servo configuration
        let mut opts = opts::default_opts();
        opts::set_defaults(&mut opts);

        // Configure for offscreen rendering
        opts.output_file = None;
        opts.headless = true;

        // Set resources path (required for Servo)
        // In production, this should point to the Servo resources directory
        if let Ok(current_dir) = std::env::current_dir() {
            let resources_path = current_dir.join("resources").join("servo");
            if resources_path.exists() {
                libservo::servo_config::resource_files::set_resources_path(
                    Some(resources_path.to_string_lossy().to_string())
                );
            } else {
                warn!("Servo resources directory not found at {:?}", resources_path);
            }
        }

        // Create window for Servo
        let window = Rc::new(ServoWindow::new(gl, size));

        // Create event loop waker
        let waker = Box::new(NoOpEventLoopWaker);

        // Create embedder delegate
        let delegate = Box::new(ServoEmbedder::new());

        // Initialize Servo
        // Note: Servo::new API may have changed - adjust as needed when compiling
        info!("Creating Servo instance");

        // TODO: Uncomment and adjust API when compiling with Servo
        /*
        let servo = Servo::new(
            delegate,
            window.clone(),
            waker,
            None, // user agent
        );

        self.servo = Some(servo);

        // Load initial blank page
        let blank_url = ServoUrl::parse("about:blank")
            .expect("Failed to parse blank URL");
        servo.handle_events(vec![WindowEvent::NewBrowser(blank_url, BrowserId::new())]);
        */

        info!("Servo initialized (API calls commented out until compilation)");

        Ok(())
    }

    /// Navigate to a URL
    pub fn navigate(&mut self, url: &str) -> Result<()> {
        info!("Navigating to: {}", url);
        self.current_url = Some(url.to_string());

        if let Some(servo) = &mut self.servo {
            let servo_url = ServoUrl::parse(url)
                .map_err(|e| anyhow!("Invalid URL: {}", e))?;

            // TODO: Send navigation event to Servo
            // servo.handle_events(vec![WindowEvent::Navigation(servo_url)]);
        }

        Ok(())
    }

    /// Render a frame
    /// Returns true if rendering occurred
    pub fn render_frame(&mut self) -> Result<bool> {
        if let Some(servo) = &mut self.servo {
            // TODO: Implement actual rendering
            // servo.handle_events(vec![]);
            // servo.repaint_synchronously();

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get the current OpenGL texture ID
    /// This texture contains the rendered web page and can be displayed in GPUI
    pub fn get_texture_id(&self) -> Option<u32> {
        self.texture_id
    }

    /// Get the framebuffer ID
    pub fn get_framebuffer_id(&self) -> Option<u32> {
        self.framebuffer_id
    }

    /// Resize the rendering surface
    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        if width == self.size.width && height == self.size.height {
            return Ok(());
        }

        info!("Resizing renderer from {:?} to {}x{}", self.size, width, height);
        self.size = RenderSize { width, height };

        if let Some(surface) = &self.gl_surface {
            if let Some(context) = &self.gl_context {
                // Resize the surface
                surface.resize(
                    context,
                    width.try_into().unwrap(),
                    height.try_into().unwrap(),
                );
            }
        }

        // Recreate framebuffer with new size
        self.delete_framebuffer();
        self.create_framebuffer(self.size)?;

        // Notify Servo of size change
        if let Some(servo) = &mut self.servo {
            // TODO: Send resize event to Servo
            // servo.handle_events(vec![WindowEvent::Resize]);
        }

        Ok(())
    }

    /// Delete the current framebuffer and texture
    fn delete_framebuffer(&mut self) {
        if let Some(gl) = &self.gl {
            unsafe {
                if let Some(framebuffer_id) = self.framebuffer_id.take() {
                    gl.delete_framebuffers(1, &framebuffer_id);
                }
                if let Some(texture_id) = self.texture_id.take() {
                    gl.delete_textures(1, &texture_id);
                }
            }
        }
    }

    /// Get current size
    pub fn size(&self) -> RenderSize {
        self.size
    }
}

impl Drop for ServoRenderer {
    fn drop(&mut self) {
        // Clean up framebuffer and texture
        self.delete_framebuffer();

        // Clean up OpenGL resources
        if let Some(context) = self.gl_context.take() {
            if let Some(_surface) = self.gl_surface.take() {
                let _ = context.make_not_current();
            }
        }

        info!("ServoRenderer dropped");
    }
}

/// Create an offscreen OpenGL display
fn create_offscreen_display() -> Result<Display> {
    // Try to create a display using EGL (works on Linux)
    let preference = DisplayApiPreference::Egl;

    unsafe {
        Display::new(
            raw_window_handle::RawDisplayHandle::Xlib(
                raw_window_handle::XlibDisplayHandle::empty()
            ),
            preference,
        )
    }
    .map_err(|e| anyhow!("Failed to create OpenGL display: {}", e))
}

/// Create OpenGL context and surface for offscreen rendering
fn create_gl_context(
    display: &Display,
    size: RenderSize,
) -> Result<(glutin::context::NotCurrentContext, Surface<WindowSurface>, Config)> {
    // Create config template
    let template = ConfigTemplateBuilder::new()
        .with_alpha_size(8)
        .with_transparency(false)
        .build();

    // Find suitable config
    let config = unsafe {
        display
            .find_configs(template)?
            .reduce(|accum, config| {
                if config.num_samples() > accum.num_samples() {
                    config
                } else {
                    accum
                }
            })
            .ok_or_else(|| anyhow!("No suitable GL config found"))?
    };

    // Create context attributes
    let context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::OpenGl(Some(glutin::context::Version::new(3, 3))))
        .build(None);

    // Create context
    let context = unsafe {
        display.create_context(&config, &context_attributes)?
    };

    // Create surface for offscreen rendering (pbuffer)
    let surface_attributes = SurfaceAttributesBuilder::<WindowSurface>::new()
        .build(
            raw_window_handle::RawWindowHandle::Xlib(raw_window_handle::XlibWindowHandle::empty()),
            size.width.try_into().unwrap(),
            size.height.try_into().unwrap(),
        );

    let surface = unsafe {
        display.create_window_surface(&config, &surface_attributes)?
    };

    Ok((context, surface, config))
}

/// Load OpenGL function pointers
fn load_gl_functions(display: &Display, config: &Config) -> Rc<dyn gl::Gl> {
    let gl = unsafe {
        gl::GlFns::load_with(|symbol| {
            let symbol = std::ffi::CString::new(symbol).unwrap();
            display.get_proc_address(&symbol) as *const _
        })
    };

    Rc::new(gl)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_creation() {
        let renderer = ServoRenderer::new();
        assert!(renderer.is_ok());
    }

    #[test]
    fn test_default_size() {
        let size = RenderSize::default();
        assert_eq!(size.width, 800);
        assert_eq!(size.height, 600);
    }
}
