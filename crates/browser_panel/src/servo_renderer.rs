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

        // Initialize Servo
        self.initialize_servo(gl, size)?;

        Ok(())
    }

    /// Initialize Servo engine
    fn initialize_servo(&mut self, gl: Rc<dyn gl::Gl>, size: RenderSize) -> Result<()> {
        // Set up Servo options
        let mut opts = opts::default_opts();
        opts::set_defaults(&mut opts);

        // Configure for offscreen rendering
        opts.output_file = None;
        opts.headless = true;

        // Create window for Servo
        let window = Rc::new(ServoWindow::new(gl, size));

        // Create event loop waker
        let waker = Box::new(NoOpEventLoopWaker);

        // Create embedder methods (not yet implemented)
        // let embedder = Box::new(ServoEmbedder::new());

        // Initialize Servo
        // Note: This is a simplified initialization
        // A full implementation would need proper embedder methods
        info!("Servo initialization would happen here");

        // TODO: Uncomment when Servo compiles
        // let servo = Servo::new(
        //     embedder,
        //     window.clone(),
        //     None, // user agent
        //     CompositeTarget::Window,
        // );

        // self.servo = Some(servo);

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
    /// This texture contains the rendered web page
    pub fn get_texture_id(&self) -> Option<u32> {
        // TODO: Return the actual texture ID from Servo's framebuffer
        None
    }

    /// Resize the rendering surface
    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
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

        // Notify Servo of size change
        if let Some(servo) = &mut self.servo {
            // TODO: Send resize event to Servo
            // servo.handle_events(vec![WindowEvent::Resize]);
        }

        Ok(())
    }

    /// Get current size
    pub fn size(&self) -> RenderSize {
        self.size
    }
}

impl Drop for ServoRenderer {
    fn drop(&mut self) {
        // Clean up OpenGL resources
        if let Some(context) = self.gl_context.take() {
            if let Some(surface) = self.gl_surface.take() {
                let _ = context.make_not_current();
            }
        }
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
