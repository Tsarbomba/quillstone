//! wgpu backend: uploads images, rasterized text and quads, and draws a [`Frame`].
//!
//! Written against wgpu 30 (see the docs' hello_triangle / cube examples). Everything
//! is a textured quad: images, solid panels (a 1x1 white texture) and text bitmaps.
//! Video frames will be one more texture that is rewritten every frame.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use quillstone_render::pixels::{bleed_alpha, mip_chain, Level};
use quillstone_render::{Frame, Tex};
use wgpu::CurrentSurfaceTexture;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::text::TextRenderer;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    /// Blur radius in window pixels.
    blur: f32,
}

const VERTEX_ATTRS: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32];

const VERTS_PER_QUAD: usize = 6;
const INITIAL_QUADS: usize = 256;

struct GpuTexture {
    bind_group: wgpu::BindGroup,
    // Kept alive for the bind group's view; video frames are written into it.
    texture: wgpu::Texture,
}

pub struct Gfx {
    instance: wgpu::Instance,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    vertex_buffer: wgpu::Buffer,
    vertex_capacity: usize, // in quads
    white: GpuTexture,
    images: HashMap<String, GpuTexture>,
    /// Size of each video texture (in `images`), to reuse it for the next frame.
    frame_sizes: HashMap<String, (u32, u32)>,
    texts: HashMap<String, GpuTexture>,
    text_renderer: TextRenderer,
}

impl Gfx {
    /// `fonts`: the theme's font files, regular first (see `TextRenderer::new`).
    pub fn new(event_loop: &ActiveEventLoop, window: Arc<Window>, fonts: Vec<Vec<u8>>) -> Gfx {
        let display_handle = event_loop.owned_display_handle();
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(display_handle)),
        );
        let surface = instance.create_surface(window.clone()).expect("failed to create surface");

        // A real GPU if there is one, else the system's software renderer (virtual
        // machines, remote desktops, very old drivers).
        let request = |force_fallback_adapter| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter,
                compatible_surface: Some(&surface),
                ..Default::default()
            }))
        };
        let adapter = request(false).or_else(|_| request(true)).unwrap_or_else(|e| {
            panic!(
                "This game needs graphics support (DirectX 12, Vulkan or Metal; OpenGL on older systems), \
                 and none was found ({e}). Updating the graphics driver usually fixes this."
            )
        });

        // The usual limits (8192-pixel images), or what an older GPU can do.
        let device = |required_limits| {
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("quillstone"),
                required_limits,
                ..Default::default()
            }))
        };
        let (device, queue) = device(wgpu::Limits::default())
            .or_else(|_| device(wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())))
            .unwrap_or_else(|e| panic!("The graphics driver refused to start ({e}). Updating it usually fixes this."));

        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("surface is not supported by this adapter");
        let caps = surface.get_capabilities(&adapter);
        // Blend in sRGB space, like image editors and other VN engines: a theme's
        // `#000000e0` then hides the picture behind it as much as it looks like it
        // should. (An sRGB surface blends in linear light, which lets a dark
        // overlay show the background through at several times the brightness.)
        if let Some(format) = caps.formats.iter().copied().find(|f| !f.is_srgb()) {
            config.format = format;
        }
        surface.configure(&device, &config);

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quad bind layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear clamp"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quad shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad pipeline layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });

        let vertex_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VERTEX_ATTRS,
        })];

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quad pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &vertex_buffers,
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let vertex_buffer = make_vertex_buffer(&device, INITIAL_QUADS);
        let white = upload_texture(&device, &queue, &bind_layout, &sampler, 1, 1, &[&[255, 255, 255, 255]]);

        Gfx {
            instance,
            window,
            surface,
            config,
            device,
            queue,
            pipeline,
            bind_layout,
            sampler,
            vertex_buffer,
            vertex_capacity: INITIAL_QUADS,
            white,
            images: HashMap::new(),
            frame_sizes: HashMap::new(),
            texts: HashMap::new(),
            text_renderer: TextRenderer::new(fonts),
        }
    }

    /// Current surface size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    /// Upload a decoded image under the script's path. `chain` is its mip chain
    /// from [`prepare_image`], largest level first. `false` if the GPU can't hold it.
    pub fn add_image(&mut self, key: String, chain: &[Level]) -> bool {
        let Some(base) = chain.first() else { return false };
        let max = self.device.limits().max_texture_dimension_2d;
        if base.width > max || base.height > max {
            eprintln!("warning: image {key} is {}x{}; this GPU allows at most {max}x{max}", base.width, base.height);
            return false;
        }
        let levels: Vec<&[u8]> = chain.iter().map(|l| l.rgba.as_slice()).collect();
        let tex = upload_texture(&self.device, &self.queue, &self.bind_layout, &self.sampler, base.width, base.height, &levels);
        self.images.insert(key, tex);
        true
    }

    /// Free an image's texture (it is loaded again if it is wanted again).
    pub fn remove_image(&mut self, key: &str) {
        self.images.remove(key);
    }

    /// Show a video frame (RGBA8) under `key`, as if it were an image. The
    /// texture is rewritten in place while the size stays the same. A frame the GPU can't
    /// hold is not shown (as with images); its size is remembered so the warning comes once.
    pub fn set_frame(&mut self, key: &str, width: u32, height: u32, rgba: &[u8]) {
        let max = self.device.limits().max_texture_dimension_2d;
        if width > max || height > max {
            if self.frame_sizes.insert(key.to_string(), (width, height)) != Some((width, height)) {
                eprintln!("warning: video {key} is {width}x{height}; this GPU allows at most {max}x{max}");
            }
            self.images.remove(key);
            return;
        }
        if self.frame_sizes.get(key) == Some(&(width, height)) {
            if let Some(tex) = self.images.get(key) {
                self.queue.write_texture(
                    tex.texture.as_image_copy(),
                    rgba,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * width), rows_per_image: Some(height) },
                    wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                );
                return;
            }
        }
        let tex = self.make_texture(width, height, rgba);
        self.images.insert(key.to_string(), tex);
        self.frame_sizes.insert(key.to_string(), (width, height));
    }

    /// Forget a video's texture once it stops playing.
    pub fn remove_frame(&mut self, key: &str) {
        if self.frame_sizes.remove(key).is_some() {
            self.images.remove(key);
        }
    }

    fn make_texture(&self, width: u32, height: u32, rgba: &[u8]) -> GpuTexture {
        upload_texture(&self.device, &self.queue, &self.bind_layout, &self.sampler, width, height, &[rgba])
    }

    pub fn render(&mut self, frame: &Frame) {
        // 1. Rasterize any text we haven't seen, and forget text no longer shown.
        // A forgotten texture of the right size is rewritten instead of making a
        // new one: a typing line changes every character, at the same size.
        let text_keys: Vec<String> = frame.texts.iter().map(|t| t.key()).collect();
        let live: HashSet<&String> = text_keys.iter().collect();
        let mut spare: Vec<GpuTexture> = Vec::new();
        self.texts.retain(|k, tex| {
            let keep = live.contains(k);
            if !keep {
                spare.push(GpuTexture { bind_group: tex.bind_group.clone(), texture: tex.texture.clone() });
            }
            keep
        });
        for (job, key) in frame.texts.iter().zip(&text_keys) {
            if self.texts.contains_key(key) {
                continue;
            }
            let Some(bitmap) = self.text_renderer.rasterize(job) else { continue };
            let (width, height) = (bitmap.width, bitmap.height);
            let tex = match spare.iter().position(|t| t.texture.width() == width && t.texture.height() == height) {
                Some(i) => {
                    let tex = spare.swap_remove(i);
                    self.queue.write_texture(
                        tex.texture.as_image_copy(),
                        &bitmap.rgba,
                        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * width), rows_per_image: Some(height) },
                        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    );
                    tex
                }
                None => self.make_texture(width, height, &bitmap.rgba),
            };
            self.text_renderer.recycle(bitmap);
            self.texts.insert(key.clone(), tex);
        }

        // 2. Build vertices for the quads we can actually draw.
        let mut vertices: Vec<Vertex> = Vec::with_capacity(frame.quads.len() * VERTS_PER_QUAD);
        let mut draws: Vec<&wgpu::BindGroup> = Vec::with_capacity(frame.quads.len());
        for quad in &frame.quads {
            let texture = match &quad.texture {
                Tex::Solid => Some(&self.white),
                Tex::Image(src) => self.images.get(src),
                Tex::Text(i) => text_keys.get(*i).and_then(|k| self.texts.get(k)),
            };
            let Some(texture) = texture else { continue };

            let [l, t, r, b] = quad.rect;
            let [u0, v0, u1, v1] = quad.uv;
            let (color, blur) = (quad.color, quad.blur);
            let v = |x: f32, y: f32, u: f32, w: f32| Vertex { pos: [x, y], uv: [u, w], color, blur };
            vertices.extend_from_slice(&[
                v(l, t, u0, v0),
                v(l, b, u0, v1),
                v(r, t, u1, v0),
                v(r, t, u1, v0),
                v(l, b, u0, v1),
                v(r, b, u1, v1),
            ]);
            draws.push(&texture.bind_group);
        }

        if draws.len() > self.vertex_capacity {
            self.vertex_capacity = draws.len().next_power_of_two();
            self.vertex_buffer = make_vertex_buffer(&self.device, self.vertex_capacity);
        }
        self.queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));

        // 3. Acquire the frame (arms follow wgpu's own example).
        let surface_texture = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(texture) => texture,
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => return,
            CurrentSurfaceTexture::Suboptimal(texture) => {
                drop(texture);
                self.surface.configure(&self.device, &self.config);
                return;
            }
            CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            CurrentSurfaceTexture::Lost => {
                self.surface = self
                    .instance
                    .create_surface(self.window.clone())
                    .expect("failed to recreate surface");
                self.surface.configure(&self.device, &self.config);
                return;
            }
            CurrentSurfaceTexture::Validation => {
                eprintln!("wgpu: surface validation error");
                return;
            }
        };

        let view = surface_texture.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            for (i, bind_group) in draws.iter().enumerate() {
                pass.set_bind_group(0, Some(*bind_group), &[]);
                let first = (i * VERTS_PER_QUAD) as u32;
                pass.draw(first..first + VERTS_PER_QUAD as u32, 0..1);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        self.window.pre_present_notify();
        self.queue.present(surface_texture);
    }
}

/// Turn a decoded image (RGBA8, straight alpha) into a mip chain with clean
/// transparent edges. CPU-only, so it can run before the GPU exists.
pub fn prepare_image(width: u32, height: u32, mut rgba: Vec<u8>) -> Vec<Level> {
    bleed_alpha(&mut rgba, width, height);
    mip_chain(width, height, rgba)
}

fn make_vertex_buffer(device: &wgpu::Device, quads: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("quad vertices"),
        size: (quads * VERTS_PER_QUAD * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Create an RGBA8 texture, upload `levels` into it and wrap it in a bind group.
/// `levels[0]` is `width` x `height`; each further mip level halves the one before
/// (rounding down, never below 1), as `pixels::mip_chain` produces them.
fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    width: u32,
    height: u32,
    levels: &[&[u8]],
) -> GpuTexture {
    let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm, // sampled as stored: sRGB values, blended as such
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (mip, rgba) in levels.iter().enumerate() {
        let (w, h) = ((width >> mip).max(1), (height >> mip).max(1));
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: mip as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    });
    GpuTexture { bind_group, texture }
}
