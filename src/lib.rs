use core::str;
use std::{ffi::c_void, marker::PhantomData};

use glow::HasContext;
use glutin::{
    config::{Config, ConfigTemplateBuilder, GlConfig as _},
    context::ContextAttributesBuilder,
    display::GetGlDisplay as _,
    prelude::{GlDisplay as _, NotCurrentGlContext as _},
    surface::{GlSurface as _, WindowSurface},
};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use winit::dpi::PhysicalSize;
use winit::{event_loop::ActiveEventLoop, window::Window};

macro_rules! check {
    // () => {};
    ($gl:expr) => {{
        let (file, line) = (file!(), line!());
        // Check for OpenGL errors
        let err = $gl.get_error();
        if err != glow::NO_ERROR {
            eprintln!(
                "OpenGL Error ({}): {} at {}:{}",
                err,
                match err {
                    glow::INVALID_ENUM => "GL_INVALID_ENUM",
                    glow::INVALID_VALUE => "GL_INVALID_VALUE",
                    glow::INVALID_OPERATION => "GL_INVALID_OPERATION",
                    glow::STACK_OVERFLOW => "GL_STACK_OVERFLOW",
                    glow::STACK_UNDERFLOW => "GL_STACK_UNDERFLOW",
                    glow::OUT_OF_MEMORY => "GL_OUT_OF_MEMORY",
                    glow::INVALID_FRAMEBUFFER_OPERATION => "GL_INVALID_FRAMEBUFFER_OPERATION",
                    _ => "Unknown Error",
                },
                file,
                line
            );
        }
    }};
}

struct PixelBuffer<Format> {
    raw_buffer: glow::Buffer,
    length: usize,
    format: PhantomData<Format>,
}

struct MMap<'ctx, 'buffer, Format> {
    buffer: &'buffer PixelBuffer<Format>,
    gl: &'ctx glow::Context,
    mapped_memory: *mut c_void,
}

impl<'ctx, 'buffer, Format> AsMut<[Format]> for MMap<'ctx, 'buffer, Format> {
    fn as_mut(&mut self) -> &mut [Format] {
        unsafe {
            std::slice::from_raw_parts_mut(self.mapped_memory as *mut Format, self.buffer.length)
        }
    }
}

impl<'ctx, 'buffer, Format> MMap<'ctx, 'buffer, Format> {
    // Constructor for creating the guard
    pub fn new(gl: &'ctx glow::Context, buffer: &'buffer PixelBuffer<Format>) -> Self {
        let mapped_memory;
        unsafe {
            gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, Some(buffer.raw_buffer));
            check!(gl);

            // map the buffer to client memory
            mapped_memory = gl.map_buffer_range(
                glow::PIXEL_UNPACK_BUFFER,
                0,
                (buffer.length * std::mem::size_of::<Format>()) as _,
                glow::MAP_READ_BIT | glow::MAP_WRITE_BIT,
            ) as *mut c_void;
            check!(gl);
        }

        if mapped_memory.is_null() {
            panic!("failed to map buffer to client memory");
        }

        MMap {
            buffer,
            mapped_memory,
            gl,
        }
    }
}

impl<Format> Drop for MMap<'_, '_, Format> {
    fn drop(&mut self) {
        unsafe {
            self.gl
                .bind_buffer(glow::PIXEL_UNPACK_BUFFER, Some(self.buffer.raw_buffer));
            // this will sync the data with the GPU
            self.gl.unmap_buffer(glow::PIXEL_UNPACK_BUFFER);
            check!(self.gl);
        }
    }
}

pub struct Framebuffer<PixelFormat> {
    // window surface handles
    surface: glutin::surface::Surface<WindowSurface>,
    ctx_handle: glutin::context::PossiblyCurrentContext,

    // opengl state
    width: usize,
    height: usize,
    gl: glow::Context,
    pixel_buffer: PixelBuffer<PixelFormat>,
    texture: glow::Texture,
    vao: glow::VertexArray,
    program: glow::Program,
}

pub struct FramebufferConfigExt {
    pub clear_color: Option<[f32; 4]>,
}

impl<Format> Framebuffer<Format> {
    fn gl_config_picker(configs: Box<dyn Iterator<Item = Config> + '_>) -> Config {
        configs
            .reduce(|accum, config| {
                let transparency_check = config.supports_transparency().unwrap_or(false)
                    & !accum.supports_transparency().unwrap_or(false);

                if transparency_check || config.num_samples() < accum.num_samples() {
                    config
                } else {
                    accum
                }
            })
            .unwrap()
    }

    #[allow(unused)]
    pub fn init(
        event_loop: &ActiveEventLoop,
        width: usize,
        height: usize,
    ) -> (winit::window::Window, Self) {
        Self::init_with_ext(event_loop, width, height, None)
    }

    pub fn init_with_ext(
        event_loop: &ActiveEventLoop,
        width: usize,
        height: usize,
        ext: Option<FramebufferConfigExt>,
    ) -> (winit::window::Window, Self) {
        let size = width * height;

        let (window, surface, ctx_handle, gl) = {
            let window_attributes = Window::default_attributes().with_inner_size(PhysicalSize {
                width: width as u32,
                height: height as u32,
            });

            let template = ConfigTemplateBuilder::new().with_alpha_size(8);

            let display_builder =
                DisplayBuilder::new().with_window_attributes(Some(window_attributes));

            let (window, gl_config) = display_builder
                .build(event_loop, template, Self::gl_config_picker)
                .unwrap();

            let window = window.expect("can create OpenGL window");
            let display = gl_config.display();

            let surface = {
                let attrs = window
                    .build_surface_attributes(<_>::default())
                    .expect("Failed to build surface attributes");
                unsafe { display.create_window_surface(&gl_config, &attrs).unwrap() }
            };

            let ctx_handle = {
                let raw_window_handle = window
                    .window_handle()
                    .expect("can obtain a raw window handle")
                    .as_raw();

                let context_attributes =
                    ContextAttributesBuilder::new().build(Some(raw_window_handle));

                let not_current_gl_context = unsafe {
                    display
                        .create_context(&gl_config, &context_attributes)
                        .expect("failed to create context")
                };

                not_current_gl_context
                    .make_current(&surface)
                    .expect("can make context current")
            };

            let gl = unsafe {
                glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s))
            };

            (window, surface, ctx_handle, gl)
        };

        let program = {
            let vertex_shader =
                Self::compile_shader(&gl, Self::VERTEX_SHADER_SRC, glow::VERTEX_SHADER);
            let fragment_shader =
                Self::compile_shader(&gl, Self::FRAGMENT_SHADER_SRC, glow::FRAGMENT_SHADER);
            Self::create_shader_program(&gl, vertex_shader, fragment_shader)
        };

        let vao;
        let pixel_buffer;
        let texture;

        unsafe {
            vao = gl.create_vertex_array().unwrap();
            check!(gl);
            gl.bind_vertex_array(Some(vao));
            check!(gl);
            // Create the PBO and the texture
            pixel_buffer = Self::create_pixel_buffer(&gl, size);
            check!(gl);
            texture = Self::create_texture(&gl, width, height);
            check!(gl);
        }

        unsafe {
            let [r, g, b, a] = ext
                .map(|ext| ext.clear_color.unwrap_or_default())
                .unwrap_or_default();

            gl.clear_color(r, g, b, a);
            check!(gl);
        }

        (
            window,
            Self {
                surface,
                ctx_handle,
                width,
                height,
                gl,
                pixel_buffer,
                texture,
                vao,
                program,
            },
        )
    }

    fn compile_shader(gl: &glow::Context, source: &str, shader_type: u32) -> glow::Shader {
        unsafe {
            let shader = gl.create_shader(shader_type).unwrap();
            gl.shader_source(shader, source);
            gl.compile_shader(shader);

            if !gl.get_shader_compile_status(shader) {
                let error_msg = gl.get_shader_info_log(shader);
                println!("ERROR::SHADER::COMPILATION_FAILED\n{}", error_msg);
            }
            shader
        }
    }

    fn create_shader_program(
        gl: &glow::Context,
        vertex_shader: glow::Shader,
        fragment_shader: glow::Shader,
    ) -> glow::Program {
        unsafe {
            let program = gl.create_program().unwrap();
            gl.attach_shader(program, vertex_shader);
            gl.attach_shader(program, fragment_shader);
            gl.link_program(program);

            if !gl.get_program_link_status(program) {
                println!(
                    "ERROR::PROGRAM::LINKING_FAILED\n{}",
                    gl.get_program_info_log(program)
                );
            }
            program
        }
    }

    const VERTEX_SHADER_SRC: &str = r#"
#version 330 core
out vec2 TexCoord;

void main()
{
    // Define the quad vertices directly
    vec3 vertices[4] = vec3[4] (
        vec3(-1.0, -1.0, 0.0),  // bottom-left  (0)
        vec3( 1.0, -1.0, 0.0),  // bottom-right (1)
        vec3(-1.0,  1.0, 0.0),  // top-left     (2)
        vec3( 1.0,  1.0, 0.0)   // top-right    (3)
    );

    // Texture coordinates corresponding to each vertex
    vec2 texCoords[4] = vec2[4](
        vec2(0.0, 1.0),
        vec2(1.0, 1.0),
        vec2(0.0, 0.0),
        vec2(1.0, 0.0) 
    );

    // Set gl_Position and pass the texture coordinates
    gl_Position = vec4(vertices[gl_VertexID].xyz, 1.0);
    TexCoord = texCoords[gl_VertexID];
}
"#;

    const FRAGMENT_SHADER_SRC: &str = r#"
#version 330 core
out vec4 FragColor;
in vec2 TexCoord;
uniform sampler2D texture1;
void main()
{
    FragColor = texture(texture1, TexCoord);
}
"#;

    fn create_pixel_buffer(gl: &glow::Context, length: usize) -> PixelBuffer<Format> {
        unsafe {
            let pbo = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, Some(pbo));
            check!(gl);
            gl.buffer_data_size(
                glow::PIXEL_UNPACK_BUFFER,
                (length * std::mem::size_of::<Format>()) as _,
                glow::STREAM_DRAW,
            );
            check!(gl);
            PixelBuffer {
                raw_buffer: pbo,
                length,
                format: PhantomData,
            }
        }
    }

    fn create_texture(gl: &glow::Context, width: usize, height: usize) -> glow::Texture {
        unsafe {
            let texture = gl.create_texture().unwrap();
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            check!(gl);
            gl.texture_storage_2d(texture, 1, glow::RGBA32F, width as i32, height as i32);
            check!(gl);
            gl.texture_parameter_i32(texture, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
            check!(gl);
            gl.texture_parameter_i32(texture, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            check!(gl);
            texture
        }
    }
}

impl Framebuffer<[u8; 4]> {
    pub fn draw(&self, painter: &mut impl Painter<Pixel = [u8; 4]>) {
        let gl = &self.gl;

        unsafe {
            gl.clear(glow::COLOR_BUFFER_BIT);
            check!(gl);
        }

        // repaint the window - note: scope here is important as it unmaps pixel buffer from CPU memory
        {
            let mut guard = MMap::new(gl, &self.pixel_buffer);

            painter.paint(guard.as_mut());
        }

        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            check!(gl);
            gl.bind_buffer(
                glow::PIXEL_UNPACK_BUFFER,
                Some(self.pixel_buffer.raw_buffer),
            );
            check!(gl);

            // Transfer data from the PBO to the texture
            gl.texture_sub_image_2d(
                self.texture,
                0, // mip level
                0, // x offset
                0, // y offset
                self.width as _,
                self.height as _,
                glow::RGBA,          // TODO: Format needs to provide these values
                glow::UNSIGNED_BYTE, // TODO: Format needs to provide these values
                glow::PixelUnpackData::BufferOffset(0),
            );
            check!(gl);

            gl.use_program(Some(self.program));
            check!(gl);
            gl.bind_vertex_array(Some(self.vao));
            check!(gl);
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
            check!(gl);
        }

        self.surface
            .swap_buffers(&self.ctx_handle)
            .expect("can swap buffers");
    }
}

pub trait Painter {
    type Pixel;

    fn paint(&mut self, pixels: &mut [Self::Pixel]);
}
