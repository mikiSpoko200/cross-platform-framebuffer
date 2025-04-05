use cpf::{Framebuffer, FramebufferConfigExt, Painter};
use winit::event_loop::ActiveEventLoop;
use winit::event_loop::EventLoop;

#[derive(Clone, Copy)]
pub struct Config {
    width: usize,
    height: usize,
}

struct Surface<Format> {
    framebuffer: Framebuffer<Format>,
    window: winit::window::Window,
}

// NOTE: [u8; 4] is the only supported pixel format as of now.
pub struct App<P: Painter<Pixel = [u8; 4]>> {
    // application logic
    config: Config,
    painter: P,

    // window drawing
    surface: Option<Surface<[u8; 4]>>,
}

impl<P: Painter<Pixel = [u8; 4]>> App<P> {
    pub fn new(width: usize, height: usize, painter: P) -> Self {
        Self {
            config: Config { width, height },
            painter,
            surface: None,
        }
    }
}

impl<P: Painter<Pixel = [u8; 4]>> winit::application::ApplicationHandler for App<P> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.surface.is_none() {
            let (window, framebuffer) = {
                Framebuffer::init_with_ext(
                    event_loop,
                    self.config.width,
                    self.config.height,
                    Some(FramebufferConfigExt {
                        clear_color: Some([0.3, 0.4, 0.7, 1.0]),
                    }),
                )
            };

            self.surface = Some(Surface {
                framebuffer,
                window,
            });
            println!("application initialized");
        }
    }

    fn window_event(
        &mut self,
        _: &ActiveEventLoop,
        _: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        match event {
            winit::event::WindowEvent::RedrawRequested => {
                self.surface.as_mut().map(
                    |Surface {
                         framebuffer,
                         window,
                     }| {
                        framebuffer.draw(&mut self.painter);

                        // TODO: draw ui

                        window.request_redraw();
                    },
                );
            }
            winit::event::WindowEvent::CloseRequested => {
                std::process::exit(0);
            }
            _ => {}
        };
    }
}

pub struct BasicPainter;

impl Painter for BasicPainter {
    type Pixel = [u8; 4];

    fn paint(&mut self, pixels: &mut [Self::Pixel]) {
        let mut index = 0;
        for pixel in pixels {
            *pixel = [index + 1, index + 2, index + 3, 255];
            index = index.wrapping_add(4);
        }
    }
}

pub struct RGBPainter {
    width: usize,
    height: usize,
}

impl From<Config> for RGBPainter {
    fn from(Config { width, height }: Config) -> Self {
        Self { width, height }
    }
}

impl Painter for RGBPainter {
    type Pixel = [u8; 4];

    fn paint(&mut self, pixels: &mut [Self::Pixel]) {
        let mut counter = 0;
        const RED: [u8; 4] = [255, 0, 0, 0];
        const GREEN: [u8; 4] = [0, 255, 0, 0];
        const BLUE: [u8; 4] = [0, 0, 255, 0];

        for pixel in pixels {
            let color = match (counter / (self.width / 4 * self.height)) % 4 {
                0 => RED,
                1 => GREEN,
                2 => BLUE,
                3 => [0, 0, 0, 0],
                _ => unreachable!(),
            };
            counter += 1;
            *pixel = color;
        }
    }
}

pub struct CheckAlignment {
    width: usize,
    height: usize,
}

impl From<Config> for CheckAlignment {
    fn from(Config { width, height }: Config) -> Self {
        Self { width, height }
    }
}

impl Painter for CheckAlignment {
    type Pixel = [u8; 4];

    fn paint(&mut self, pixels: &mut [Self::Pixel]) {
        for (index, pixel) in pixels.iter_mut().enumerate() {
            if index < 8 {
                *pixel = [255, 0, 0, 0]
            }
            if self.width - 8 < index && index < self.width {
                *pixel = [255, 255, 0, 0]
            }

            if self.width * (self.height - 1) < index && index < self.width * (self.height - 1) + 8
            {
                *pixel = [0, 255, 0, 0]
            }

            if self.width * self.height - 8 < index && index < self.width * self.height {
                *pixel = [0, 255, 255, 0]
            }
        }
    }
}

pub struct LinePainter {
    width: usize,
    height: usize,
}

impl From<Config> for LinePainter {
    fn from(Config { width, height }: Config) -> Self {
        Self { width, height }
    }
}

impl Painter for LinePainter {
    type Pixel = [u8; 4];

    fn paint(&mut self, pixels: &mut [Self::Pixel]) {
        for y in 0..self.height {
            for x in 0..self.width {
                // Calculate the distance from the main diagonal (y = x)
                let dist = (y as isize - x as isize).abs();

                // If the pixel is within the 8-pixel thickness of the diagonal, draw it
                if dist <= 8 as isize {
                    // Access the pixel at (x, y)
                    let pixel = &mut pixels[y * self.width + x];

                    pixel[0] = 255; // Red
                    pixel[1] = 255; // Green
                    pixel[2] = 255; // Blue
                    pixel[3] = 255; // Alpha (fully opaque)
                }
            }
        }
    }
}

pub fn run<P: Painter<Pixel = [u8; 4]> + From<Config>>(
    width: usize,
    height: usize,
) -> anyhow::Result<()> {
    let config = Config { width, height };
    let mut app = App::new(width, height, P::from(config));

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    Ok(event_loop.run_app(&mut app)?)
}

pub fn main() -> anyhow::Result<()> {
    run::<LinePainter>(640, 640)
}
