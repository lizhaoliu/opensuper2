//! GPU presentation (wgpu): the indexed 640x480 frame is converted to RGBA
//! on the CPU, uploaded as a texture and drawn into the view rectangle by
//! the shader of the chosen filter (`present.wgsl`). The CPU path in
//! `main.rs` stays as the fallback when no adapter is available.

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use s2engine::sys::{SCREEN_H, SCREEN_W};
use winit::window::Window;

use crate::frontcfg::Filter;

/// Uniform block of `present.wgsl` (`Params`).
#[repr(C)]
#[derive(Clone, Copy)]
struct Params {
    view: [f32; 4],
    src: [f32; 2],
    scale: [f32; 2],
    out_srgb: u32,
    _pad: [u32; 3],
}

impl Params {
    fn bytes(&self) -> [u8; 48] {
        let mut b = [0u8; 48];
        let words: [u32; 12] = [
            self.view[0].to_bits(),
            self.view[1].to_bits(),
            self.view[2].to_bits(),
            self.view[3].to_bits(),
            self.src[0].to_bits(),
            self.src[1].to_bits(),
            self.scale[0].to_bits(),
            self.scale[1].to_bits(),
            self.out_srgb,
            0,
            0,
            0,
        ];
        for (k, w) in words.iter().enumerate() {
            b[k * 4..k * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        b
    }
}

/// Device, frame texture and the filter pipelines for one target format.
struct Core {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    frame: wgpu::Texture,
    params: wgpu::Buffer,
    bind: wgpu::BindGroup,
    /// One pipeline per [`Filter`], in `Filter::ALL` order.
    pipelines: Vec<wgpu::RenderPipeline>,
}

pub struct Gpu {
    surface: wgpu::Surface<'static>,
    core: Core,
    config: wgpu::SurfaceConfiguration,
    pub adapter_info: String,
}

/// What went wrong with a frame, and whether the renderer is still usable.
pub enum FrameError {
    /// Skip this frame (minimised, timeout); try again on the next.
    Skip,
    /// The device or surface is gone for good: fall back to the CPU path.
    Fatal(String),
}

impl Gpu {
    pub fn new(window: Arc<Window>) -> Result<Gpu> {
        let size = window.inner_size();
        let desc = wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone()));
        let instance = wgpu::Instance::new(desc);
        let surface = instance.create_surface(window).context("creating the GPU surface")?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .context("no GPU adapter for the window")?;
        let adapter_info = describe(&adapter);
        let caps = surface.get_capabilities(&adapter);
        // A plain (non-sRGB) 8-bit format shows the palette values as they
        // are, like the CPU path; an sRGB-only surface gets linear output.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm))
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| anyhow!("the surface reports no formats"))?;
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo
        };
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| anyhow!("the surface is not supported by {adapter_info}"))?;
        config.format = format;
        config.present_mode = present_mode;
        config.desired_maximum_frame_latency = 2;
        let core = Core::new(&adapter, format)?;
        surface.configure(&core.device, &config);
        Ok(Gpu { surface, core, config, adapter_info })
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w > 0 && h > 0 && (w, h) != (self.config.width, self.config.height) {
            self.config.width = w;
            self.config.height = h;
            self.surface.configure(&self.core.device, &self.config);
        }
    }

    /// Draw `rgba` (640x480, 4 bytes per pixel) into `view` (x, y, w, h)
    /// of a `w`x`h` window with `filter`, black around it.
    pub fn render(&mut self, rgba: &[u8], size: (u32, u32), view: (u32, u32, u32, u32), filter: Filter) -> Result<(), FrameError> {
        self.resize(size.0, size.1);
        let tex = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                // Use it, and reconfigure for the next frame.
                self.surface.configure(&self.core.device, &self.config);
                t
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return Err(FrameError::Skip),
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.core.device, &self.config);
                return Err(FrameError::Skip);
            }
            other => return Err(FrameError::Fatal(format!("surface: {other:?}"))),
        };
        let target = tex.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let cmd = self.core.encode(&target, rgba, size, view, filter);
        self.core.queue.submit([cmd]);
        self.core.queue.present(tex);
        Ok(())
    }
}

impl Core {
    fn new(adapter: &wgpu::Adapter, format: wgpu::TextureFormat) -> Result<Core> {
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("opensuper2"),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .context("opening the GPU device")?;
        let frame = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("game frame"),
            size: wgpu::Extent3d { width: SCREEN_W as u32, height: SCREEN_H as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("present params"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear clamp"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("present"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let view = frame.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("present"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        let pipelines = build_pipelines(&device, &layout, format);
        Ok(Core { device, queue, format, frame, params, bind, pipelines })
    }

    /// Upload the frame and record its drawing into `target` (a `size`
    /// texture): black, with the frame in `view` through `filter`.
    fn encode(
        &self,
        target: &wgpu::TextureView,
        rgba: &[u8],
        size: (u32, u32),
        view: (u32, u32, u32, u32),
        filter: Filter,
    ) -> wgpu::CommandBuffer {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.frame,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SCREEN_W as u32 * 4), rows_per_image: None },
            wgpu::Extent3d { width: SCREEN_W as u32, height: SCREEN_H as u32, depth_or_array_layers: 1 },
        );
        let (vx, vy, vw, vh) = view;
        let p = Params {
            view: [vx as f32, vy as f32, vw.max(1) as f32, vh.max(1) as f32],
            src: [SCREEN_W as f32, SCREEN_H as f32],
            scale: [vw as f32 / SCREEN_W as f32, vh as f32 / SCREEN_H as f32],
            out_srgb: self.format.is_srgb() as u32,
            _pad: [0; 3],
        };
        self.queue.write_buffer(&self.params, 0, &p.bytes());
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("present") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if vw > 0 && vh > 0 && vx + vw <= size.0 && vy + vh <= size.1 {
                pass.set_viewport(vx as f32, vy as f32, vw as f32, vh as f32, 0.0, 1.0);
                pass.set_pipeline(&self.pipelines[filter.index()]);
                pass.set_bind_group(0, &self.bind, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        enc.finish()
    }
}

fn describe(adapter: &wgpu::Adapter) -> String {
    let info = adapter.get_info();
    format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type)
}

/// Render `rgba` with `filter` into a `w`x`h` image without a window (for
/// `--filter-shots`); returns RGBA bytes and the adapter used.
pub fn render_offscreen(rgba: &[u8], w: u32, h: u32, view: (u32, u32, u32, u32), filter: Filter) -> Result<(Vec<u8>, String)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .context("no GPU adapter")?;
    let info = describe(&adapter);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let core = Core::new(&adapter, format)?;
    let target = core.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let tv = target.create_view(&wgpu::TextureViewDescriptor::default());
    let draw = core.encode(&tv, rgba, (w, h), view, filter);
    let stride = (w * 4).div_ceil(256) * 256;
    let buf = core.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (stride * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = core.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: None },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    core.queue.submit([draw, enc.finish()]);
    buf.map_async(wgpu::MapMode::Read, .., |_| {});
    core.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).context("waiting for the GPU")?;
    let data = buf.get_mapped_range(..).map_err(|e| anyhow!("mapping the readback buffer: {e:?}"))?;
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let row = (y * stride) as usize;
        out.extend_from_slice(&data[row..row + (w * 4) as usize]);
    }
    Ok((out, info))
}

fn build_pipelines(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, format: wgpu::TextureFormat) -> Vec<wgpu::RenderPipeline> {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("present.wgsl"),
        source: wgpu::ShaderSource::Wgsl(include_str!("present.wgsl").into()),
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("present"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    Filter::ALL
        .iter()
        .map(|f| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(f.name()),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(f.entry_point()),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
        .collect()
}
