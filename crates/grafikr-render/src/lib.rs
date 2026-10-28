//! GPU renderer for the display list. Quad SDF pipeline + glyph atlas pipeline.

use std::borrow::Cow;
use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use grafikr_core::{Color, DisplayItem, DisplayList, FontData, ImageData, Rect};
use swash::FontRef;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;
use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, BindGroupLayoutEntry, BindingType, BlendState, Buffer, BufferBindingType,
    BufferUsages, ColorTargetState, ColorWrites, Device, Extent3d, FragmentState, LoadOp,
    MultisampleState, Operations, PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPass,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor,
    SamplerBindingType, ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp,
    TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
    TextureView, TextureViewDimension, VertexState,
};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    viewport: [f32; 2],
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    pos: [f32; 2],
    center: [f32; 2],
    half: [f32; 2],
    radius: f32,
    border: f32,
    fill: [f32; 4],
    stroke: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TextVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    font: u64,
    index: u32,
    glyph: u32,
    px_quarter: u16,
}

#[derive(Clone, Copy)]
struct GlyphEntry {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    left: i32,
    top: i32,
}

struct Atlas {
    size: u32,
    texture: wgpu::Texture,
    view: TextureView,
    sampler: wgpu::Sampler,
    x: u32,
    y: u32,
    row: u32,
    entries: HashMap<GlyphKey, GlyphEntry>,
}

impl Atlas {
    fn new(device: &Device, size: u32) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("grafikr glyph atlas"),
            size: Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("grafikr glyph sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            size,
            texture,
            view,
            sampler,
            x: 0,
            y: 0,
            row: 0,
            entries: HashMap::new(),
        }
    }

    fn reset(&mut self, queue: &Queue) {
        self.entries.clear();
        self.x = 0;
        self.y = 0;
        self.row = 0;
        let zeros = vec![0u8; (self.size * self.size) as usize];
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &zeros,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.size),
                rows_per_image: Some(self.size),
            },
            Extent3d {
                width: self.size,
                height: self.size,
                depth_or_array_layers: 1,
            },
        );
    }

    fn alloc(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if width == 0 || height == 0 {
            return Some((0, 0));
        }
        if self.x + width + 1 > self.size {
            self.x = 0;
            self.y += self.row + 1;
            self.row = 0;
        }
        if self.y + height + 1 > self.size {
            return None;
        }
        let position = (self.x, self.y);
        self.x += width + 1;
        self.row = self.row.max(height);
        Some(position)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Quad,
    Text,
    Image,
}

#[derive(Clone, Copy)]
struct Batch {
    kind: Kind,
    clip: Option<Rect>,
    image: Option<u64>,
    start: u32,
    end: u32,
}

struct ImageTexture {
    _texture: wgpu::Texture,
    _view: TextureView,
    bind_group: BindGroup,
}

/// Counters from the most recent frame, useful for memory diagnostics.
#[derive(Clone, Copy, Debug, Default)]
pub struct RendererStats {
    pub glyphs: usize,
    pub images: usize,
    pub quad_vertices: usize,
    pub text_vertices: usize,
    pub batches: usize,
    pub atlas_pixels: (u32, u32),
}

pub struct Renderer {
    quad_pipeline: RenderPipeline,
    text_pipeline: RenderPipeline,
    image_pipeline: RenderPipeline,
    bind_group: BindGroup,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: Buffer,
    quad_buffer: Buffer,
    quad_capacity: u64,
    text_buffer: Buffer,
    text_capacity: u64,
    quad_vertices: Vec<QuadVertex>,
    text_vertices: Vec<TextVertex>,
    batches: Vec<Batch>,
    images: HashMap<u64, ImageTexture>,
    mask_glyphs: HashMap<(u64, u32), u32>,
    atlas: Atlas,
    scaler: ScaleContext,
    format: TextureFormat,
}

impl Renderer {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grafikr shader"),
            source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
        });

        let atlas = Atlas::new(device, 1024);
        let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grafikr uniforms"),
            contents: bytemuck::bytes_of(&Uniforms {
                viewport: [1.0, 1.0],
                _pad: [0.0; 2],
            }),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grafikr bind group layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grafikr bind group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&atlas.sampler),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grafikr pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        let targets = [Some(ColorTargetState {
            format,
            blend: Some(BlendState::ALPHA_BLENDING),
            write_mask: ColorWrites::ALL,
        })];

        let quad_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grafikr quad pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_quad"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<QuadVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2,
                        1 => Float32x2,
                        2 => Float32x2,
                        3 => Float32,
                        4 => Float32,
                        5 => Float32x4,
                        6 => Float32x4
                    ],
                })],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_quad"),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        });

        let text_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grafikr text pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_text"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TextVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2,
                        1 => Float32x2,
                        2 => Float32x4
                    ],
                })],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_text"),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        });

        let image_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grafikr image pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_text"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TextVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2,
                        1 => Float32x2,
                        2 => Float32x4
                    ],
                })],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_image"),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        });

        let quad_buffer = create_vertex_buffer(device, 4096, "grafikr quads");
        let text_buffer = create_vertex_buffer(device, 4096, "grafikr text");

        Self {
            quad_pipeline,
            text_pipeline,
            image_pipeline,
            bind_group,
            bind_group_layout: layout,
            sampler: atlas.sampler.clone(),
            uniforms,
            quad_buffer,
            quad_capacity: 4096,
            text_buffer,
            text_capacity: 4096,
            quad_vertices: Vec::new(),
            text_vertices: Vec::new(),
            batches: Vec::new(),
            images: HashMap::new(),
            mask_glyphs: HashMap::new(),
            atlas,
            scaler: ScaleContext::new(),
            format,
        }
    }

    pub fn format(&self) -> TextureFormat {
        self.format
    }

    /// Draw one frame. `viewport` is physical pixels, `scale` maps DIP to pixels.
    #[allow(
        clippy::too_many_arguments,
        reason = "explicit frame state is clearer than a params struct here"
    )]
    pub fn stats(&self) -> RendererStats {
        RendererStats {
            glyphs: self.atlas.entries.len(),
            images: self.images.len(),
            quad_vertices: self.quad_vertices.len(),
            text_vertices: self.text_vertices.len(),
            batches: self.batches.len(),
            atlas_pixels: (self.atlas.x, self.atlas.y),
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "render needs device, queue, target, viewport, scale, clear color, and list"
    )]
    pub fn render(
        &mut self,
        device: &Device,
        queue: &Queue,
        view: &TextureView,
        viewport: (u32, u32),
        scale: f32,
        clear: Color,
        list: &DisplayList,
    ) {
        if viewport.0 == 0 || viewport.1 == 0 {
            return;
        }
        if self.atlas.y > self.atlas.size * 7 / 8 {
            self.atlas.reset(queue);
        }
        if self.images.len() > 64 {
            self.images.clear();
        }

        queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::bytes_of(&Uniforms {
                viewport: [viewport.0 as f32, viewport.1 as f32],
                _pad: [0.0; 2],
            }),
        );

        self.quad_vertices.clear();
        self.text_vertices.clear();
        self.batches.clear();
        let mut current: Option<Batch> = None;

        for item in &list.items {
            match item {
                DisplayItem::Rect {
                    rect,
                    color,
                    radius,
                    border,
                    clip,
                } => {
                    let start = self.quad_vertices.len() as u32;
                    push_quad(
                        &mut self.quad_vertices,
                        *rect,
                        *color,
                        *radius,
                        *border,
                        scale,
                    );
                    let end = self.quad_vertices.len() as u32;
                    push_batch(
                        &mut self.batches,
                        &mut current,
                        Kind::Quad,
                        *clip,
                        None,
                        start,
                        end,
                    );
                }
                DisplayItem::Shadow {
                    rect,
                    radius,
                    blur,
                    color,
                    clip,
                } => {
                    let start = self.quad_vertices.len() as u32;
                    let layers = 8u32;
                    for index in 0..layers {
                        let t = index as f32 / (layers - 1) as f32;
                        let spread = blur * (1.0 - t);
                        let expanded = Rect::new(
                            rect.x - spread,
                            rect.y - spread,
                            rect.width + spread * 2.0,
                            rect.height + spread * 2.0,
                        );
                        let alpha = (color.a as f32 * 0.10).clamp(1.0, 255.0) as u8;
                        push_quad(
                            &mut self.quad_vertices,
                            expanded,
                            color.with_alpha(alpha),
                            radius + spread,
                            None,
                            scale,
                        );
                    }
                    let end = self.quad_vertices.len() as u32;
                    push_batch(
                        &mut self.batches,
                        &mut current,
                        Kind::Quad,
                        *clip,
                        None,
                        start,
                        end,
                    );
                }
                DisplayItem::Image { data, rect, clip } => {
                    if self.image_bind_group(device, queue, data).is_none() {
                        continue;
                    }
                    let start = self.text_vertices.len() as u32;
                    let x0 = rect.x * scale;
                    let y0 = rect.y * scale;
                    let x1 = rect.right() * scale;
                    let y1 = rect.bottom() * scale;
                    let color = [1.0, 1.0, 1.0, 1.0];
                    let verts = [
                        ([x0, y0], [0.0, 0.0]),
                        ([x1, y0], [1.0, 0.0]),
                        ([x1, y1], [1.0, 1.0]),
                        ([x0, y0], [0.0, 0.0]),
                        ([x1, y1], [1.0, 1.0]),
                        ([x0, y1], [0.0, 1.0]),
                    ];
                    for (pos, uv) in verts {
                        self.text_vertices.push(TextVertex { pos, uv, color });
                    }
                    let end = self.text_vertices.len() as u32;
                    push_batch(
                        &mut self.batches,
                        &mut current,
                        Kind::Image,
                        *clip,
                        Some(data.id()),
                        start,
                        end,
                    );
                }
                DisplayItem::Text(run) => {
                    let start = self.text_vertices.len() as u32;
                    let px = run.font_size * scale;
                    for glyph in &run.glyphs {
                        let glyph_id = if run.mask {
                            self.masked_glyph(&run.font)
                        } else {
                            glyph.id
                        };
                        if glyph_id == 0 {
                            continue;
                        }
                        let key = GlyphKey {
                            font: run.font.data.id(),
                            index: run.font.index,
                            glyph: glyph_id,
                            px_quarter: (px * 4.0).round().clamp(1.0, u16::MAX as f32) as u16,
                        };
                        let Some(entry) = self.atlas_entry(queue, key, &run.font, px) else {
                            continue;
                        };
                        if entry.w == 0 || entry.h == 0 {
                            continue;
                        }
                        let x = glyph.x * scale;
                        let y = glyph.y * scale;
                        let x0 = (x + entry.left as f32).round();
                        let y0 = (y - entry.top as f32).round();
                        let x1 = x0 + entry.w as f32;
                        let y1 = y0 + entry.h as f32;
                        let u0 = entry.x as f32 / self.atlas.size as f32;
                        let v0 = entry.y as f32 / self.atlas.size as f32;
                        let u1 = (entry.x + entry.w) as f32 / self.atlas.size as f32;
                        let v1 = (entry.y + entry.h) as f32 / self.atlas.size as f32;
                        let color = run.color.to_array();
                        let verts = [
                            ([x0, y0], [u0, v0]),
                            ([x1, y0], [u1, v0]),
                            ([x1, y1], [u1, v1]),
                            ([x0, y0], [u0, v0]),
                            ([x1, y1], [u1, v1]),
                            ([x0, y1], [u0, v1]),
                        ];
                        for (pos, uv) in verts {
                            self.text_vertices.push(TextVertex { pos, uv, color });
                        }
                    }
                    let end = self.text_vertices.len() as u32;
                    if end > start {
                        push_batch(
                            &mut self.batches,
                            &mut current,
                            Kind::Text,
                            run.clip,
                            None,
                            start,
                            end,
                        );
                    }
                }
            }
        }
        if let Some(batch) = current {
            self.batches.push(batch);
        }
        if self.batches.is_empty() {
            return;
        }

        if !self.quad_vertices.is_empty() {
            let bytes = bytemuck::cast_slice(&self.quad_vertices);
            ensure_buffer(
                device,
                &mut self.quad_buffer,
                &mut self.quad_capacity,
                bytes.len() as u64,
                "grafikr quads",
            );
            queue.write_buffer(&self.quad_buffer, 0, bytes);
        }
        if !self.text_vertices.is_empty() {
            let bytes = bytemuck::cast_slice(&self.text_vertices);
            ensure_buffer(
                device,
                &mut self.text_buffer,
                &mut self.text_capacity,
                bytes.len() as u64,
                "grafikr text",
            );
            queue.write_buffer(&self.text_buffer, 0, bytes);
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grafikr frame"),
        });
        {
            let attachment = RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(wgpu::Color {
                        r: clear.to_linear()[0],
                        g: clear.to_linear()[1],
                        b: clear.to_linear()[2],
                        a: clear.to_linear()[3],
                    }),
                    store: StoreOp::Store,
                },
            };
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("grafikr pass"),
                color_attachments: &[Some(attachment)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            self.draw_batches(&mut pass, viewport, scale);
        }
        queue.submit(Some(encoder.finish()));
    }

    fn draw_batches(&self, pass: &mut RenderPass<'_>, viewport: (u32, u32), scale: f32) {
        let mut active = Kind::Quad;
        pass.set_pipeline(&self.quad_pipeline);
        pass.set_vertex_buffer(0, self.quad_buffer.slice(..));

        for batch in &self.batches {
            if batch.start == batch.end {
                continue;
            }
            let Some((x, y, width, height)) = scissor(batch.clip, viewport, scale) else {
                continue;
            };
            if batch.kind != active {
                match batch.kind {
                    Kind::Quad => {
                        pass.set_pipeline(&self.quad_pipeline);
                        pass.set_vertex_buffer(0, self.quad_buffer.slice(..));
                    }
                    Kind::Text => {
                        pass.set_pipeline(&self.text_pipeline);
                        pass.set_vertex_buffer(0, self.text_buffer.slice(..));
                    }
                    Kind::Image => {
                        pass.set_pipeline(&self.image_pipeline);
                        pass.set_vertex_buffer(0, self.text_buffer.slice(..));
                    }
                }
                active = batch.kind;
            }
            let bind_group = match batch.kind {
                Kind::Image => {
                    let Some(id) = batch.image else {
                        continue;
                    };
                    let Some(image) = self.images.get(&id) else {
                        continue;
                    };
                    &image.bind_group
                }
                Kind::Quad | Kind::Text => &self.bind_group,
            };
            pass.set_bind_group(0, bind_group, &[]);
            pass.set_scissor_rect(x, y, width, height);
            pass.draw(batch.start..batch.end, 0..1);
        }
    }

    fn masked_glyph(&mut self, font: &FontData) -> u32 {
        let key = (font.data.id(), font.index);
        if let Some(glyph) = self.mask_glyphs.get(&key) {
            return *glyph;
        }
        let glyph = FontRef::from_index(font.data.data(), font.index as usize)
            .map(|font_ref| {
                let charmap = font_ref.charmap();
                let bullet = charmap.map('\u{2022}') as u32;
                if bullet != 0 {
                    bullet
                } else {
                    charmap.map('*') as u32
                }
            })
            .unwrap_or(0);
        self.mask_glyphs.insert(key, glyph);
        glyph
    }

    fn image_bind_group(&mut self, device: &Device, queue: &Queue, data: &ImageData) -> Option<()> {
        if self.images.contains_key(&data.id()) {
            return Some(());
        }
        if self.images.len() >= 128 {
            return None;
        }
        let size = Extent3d {
            width: data.width,
            height: data.height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("grafikr image"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(data.width * 4),
                rows_per_image: Some(data.height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grafikr image bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.images.insert(
            data.id(),
            ImageTexture {
                _texture: texture,
                _view: view,
                bind_group,
            },
        );
        Some(())
    }

    fn atlas_entry(
        &mut self,
        queue: &Queue,
        key: GlyphKey,
        font: &FontData,
        px: f32,
    ) -> Option<GlyphEntry> {
        if let Some(entry) = self.atlas.entries.get(&key) {
            return Some(*entry);
        }
        let font_ref = FontRef::from_index(font.data.data(), font.index as usize)?;
        let mut scaler = self.scaler.builder(font_ref).size(px).hint(true).build();
        let sources = [Source::Outline];
        let mut render = Render::new(&sources);
        render.format(Format::Alpha);
        let image = render.render(&mut scaler, key.glyph as u16)?;
        let (x, y) = self
            .atlas
            .alloc(image.placement.width, image.placement.height)?;
        if image.placement.width > 0 && image.placement.height > 0 {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &image.data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(image.placement.width),
                    rows_per_image: Some(image.placement.height),
                },
                Extent3d {
                    width: image.placement.width,
                    height: image.placement.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.atlas.entries.insert(
            key,
            GlyphEntry {
                x,
                y,
                w: image.placement.width,
                h: image.placement.height,
                left: image.placement.left,
                top: image.placement.top,
            },
        );
        self.atlas.entries.get(&key).copied()
    }
}

fn push_batch(
    batches: &mut Vec<Batch>,
    current: &mut Option<Batch>,
    kind: Kind,
    clip: Option<Rect>,
    image: Option<u64>,
    start: u32,
    end: u32,
) {
    if let Some(batch) = current {
        if batch.kind == kind && batch.clip == clip && batch.image == image && batch.end == start {
            batch.end = end;
            return;
        }
        batches.push(*batch);
    }
    *current = Some(Batch {
        kind,
        clip,
        image,
        start,
        end,
    });
}

fn push_quad(
    vertices: &mut Vec<QuadVertex>,
    rect: Rect,
    color: Color,
    radius: f32,
    border: Option<(f32, Color)>,
    scale: f32,
) {
    let x0 = rect.x * scale;
    let y0 = rect.y * scale;
    let x1 = rect.right() * scale;
    let y1 = rect.bottom() * scale;
    let center = [(x0 + x1) * 0.5, (y0 + y1) * 0.5];
    let half = [(x1 - x0) * 0.5, (y1 - y0) * 0.5];
    let (border_width, stroke) = border
        .map(|(width, color)| (width * scale, color.to_array()))
        .unwrap_or((0.0, [0.0; 4]));
    let fill = color.to_array();
    let radius = radius * scale;
    let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y0], [x1, y1], [x0, y1]];
    for pos in corners {
        vertices.push(QuadVertex {
            pos,
            center,
            half,
            radius,
            border: border_width,
            fill,
            stroke,
        });
    }
}

fn scissor(clip: Option<Rect>, viewport: (u32, u32), scale: f32) -> Option<(u32, u32, u32, u32)> {
    let (mut x, mut y, mut width, mut height) = match clip {
        Some(clip) => (
            (clip.x * scale).floor().max(0.0) as u32,
            (clip.y * scale).floor().max(0.0) as u32,
            (clip.width * scale).ceil().max(0.0) as u32,
            (clip.height * scale).ceil().max(0.0) as u32,
        ),
        None => (0, 0, viewport.0, viewport.1),
    };
    if x > viewport.0 || y > viewport.1 {
        return None;
    }
    width = width.min(viewport.0 - x);
    height = height.min(viewport.1 - y);
    if width == 0 || height == 0 {
        return None;
    }
    x = x.min(viewport.0);
    y = y.min(viewport.1);
    Some((x, y, width, height))
}

fn create_vertex_buffer(device: &Device, size: u64, label: &str) -> Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn ensure_buffer(
    device: &Device,
    buffer: &mut Buffer,
    capacity: &mut u64,
    needed: u64,
    label: &str,
) {
    if needed <= *capacity {
        return;
    }
    *capacity = needed.next_power_of_two().max(4096);
    *buffer = create_vertex_buffer(device, *capacity, label);
}
