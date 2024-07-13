#![allow(clippy::new_without_default)]

mod bindgroup_cache;
mod buffer;
mod pipeline_cache;
mod samplers;
mod texture;

use std::collections::HashMap;
use std::mem::size_of;
use std::ops::Range;
use std::sync::Arc;

use buffer::Buffer;
use bytemuck::{Pod, Zeroable};
use glam::UVec2;
use thunderdome::{Arena, Index};
use yakui_core::geometry::{Rect, Vec2, Vec4};
use yakui_core::paint::{PaintDom, Pipeline, Texture, TextureChange, TextureFormat};
use yakui_core::{ManagedTextureId, TextureId};

use self::bindgroup_cache::TextureBindgroupCache;
use self::bindgroup_cache::TextureBindgroupCacheEntry;
use self::pipeline_cache::PipelineCache;
use self::samplers::Samplers;
use self::texture::{GpuManagedTexture, GpuTexture};

pub struct YakuiWgpu {
    main_pipeline: PipelineCache,
    text_pipeline: PipelineCache,
    samplers: Samplers,
    textures: Arena<GpuTexture>,
    managed_textures: HashMap<ManagedTextureId, GpuManagedTexture>,
    texture_bindgroup_cache: TextureBindgroupCache,

    vertices: Buffer,
    indices: Buffer,
    commands: Vec<DrawCommand>,
}

#[derive(Debug, Clone)]
pub struct SurfaceInfo<'a> {
    pub format: wgpu::TextureFormat,
    pub sample_count: u32,
    pub color_attachment: &'a wgpu::TextureView,
    pub resolve_target: Option<&'a wgpu::TextureView>,
}

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
struct Vertex {
    pos: Vec2,
    texcoord: Vec2,
    color: Vec4,
}

impl Vertex {
    const DESCRIPTOR: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: size_of::<Self>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![
            0 => Float32x2,
            1 => Float32x2,
            2 => Float32x4,
        ],
    };
}

impl YakuiWgpu {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yakui Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yakui Main Pipeline Layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let main_pipeline = PipelineCache::new(pipeline_layout);

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yakui Text Pipeline Layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let text_pipeline = PipelineCache::new(pipeline_layout);

        let samplers = Samplers::new(device);

        let default_texture_data =
            Texture::new(TextureFormat::Rgba8Srgb, UVec2::new(1, 1), vec![255; 4]);
        let default_texture = GpuManagedTexture::new(device, queue, &default_texture_data);
        let default_bindgroup = bindgroup_cache::bindgroup(
            device,
            &layout,
            &samplers,
            &default_texture.view,
            default_texture.min_filter,
            default_texture.mag_filter,
            wgpu::FilterMode::Nearest,
            wgpu::AddressMode::ClampToEdge,
        );

        Self {
            main_pipeline,
            text_pipeline,
            samplers,
            textures: Arena::new(),
            managed_textures: HashMap::new(),

            texture_bindgroup_cache: TextureBindgroupCache::new(layout, default_bindgroup),
            vertices: Buffer::new(wgpu::BufferUsages::VERTEX),
            indices: Buffer::new(wgpu::BufferUsages::INDEX),
            commands: Vec::new(),
        }
    }

    /// Creates a `TextureId` from an existing wgpu texture that then be used by
    /// any yakui widgets.
    pub fn add_texture(
        &mut self,
        view: impl Into<Arc<wgpu::TextureView>>,
        min_filter: wgpu::FilterMode,
        mag_filter: wgpu::FilterMode,
        mipmap_filter: wgpu::FilterMode,
        address_mode: wgpu::AddressMode,
    ) -> TextureId {
        let index = self.textures.insert(GpuTexture {
            view: view.into(),
            min_filter,
            mag_filter,
            mipmap_filter,
            address_mode,
        });
        TextureId::User(index.to_bits())
    }

    /// Update an existing texture with a new texture view.
    ///
    /// ## Panics
    ///
    /// Will panic if `TextureId` was not created from a previous call to
    /// `add_texture`.
    pub fn update_texture(&mut self, id: TextureId, view: impl Into<Arc<wgpu::TextureView>>) {
        let index = match id {
            TextureId::User(bits) => Index::from_bits(bits).expect("invalid user texture"),
            _ => panic!("invalid user texture"),
        };

        let existing = self
            .textures
            .get_mut(index)
            .expect("user texture does not exist");
        existing.view = view.into();
    }

    #[must_use = "YakuiWgpu::paint returns a command buffer which MUST be submitted to wgpu."]
    pub fn paint(
        &mut self,
        state: &mut yakui_core::Yakui,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface: SurfaceInfo<'_>,
    ) -> wgpu::CommandBuffer {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("yakui Encoder"),
        });

        self.paint_with_encoder(state, device, queue, &mut encoder, surface);

        encoder.finish()
    }

    pub fn paint_with_encoder(
        &mut self,
        state: &mut yakui_core::Yakui,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        surface: SurfaceInfo<'_>,
    ) {
        profiling::scope!("yakui-wgpu paint_with_encoder");

        let paint = state.paint();

        self.update_textures(device, paint, queue);

        let layers = paint.layers();
        if layers.iter().all(|layer| layer.calls.is_empty()) {
            return;
        }

        self.update_buffers(device, paint);

        let vertices = self.vertices.upload(device, queue);
        let indices = self.indices.upload(device, queue);
        let commands = &self.commands;

        if paint.surface_size() == Vec2::ZERO {
            return;
        }

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("yakui Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: surface.color_attachment,
                    resolve_target: surface.resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });

            render_pass.set_vertex_buffer(0, vertices.slice(..));
            render_pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);

            let mut last_clip = None;

            let main_pipeline = self.main_pipeline.get(
                device,
                surface.format,
                surface.sample_count,
                make_main_pipeline,
            );

            let text_pipeline = self.text_pipeline.get(
                device,
                surface.format,
                surface.sample_count,
                make_text_pipeline,
            );

            for command in commands {
                match command.pipeline {
                    Pipeline::Main => render_pass.set_pipeline(main_pipeline),
                    Pipeline::Text => render_pass.set_pipeline(text_pipeline),
                    _ => continue,
                }

                if command.clip != last_clip {
                    last_clip = command.clip;

                    let surface = paint.surface_size().as_uvec2();

                    match command.clip {
                        Some(rect) => {
                            let pos = rect.pos().as_uvec2();
                            let size = rect.size().as_uvec2();

                            let max = (pos + size).min(surface);
                            let size = UVec2::new(
                                max.x.saturating_sub(pos.x),
                                max.y.saturating_sub(pos.y),
                            );

                            // If the scissor rect isn't valid, we can skip this
                            // entire draw call.
                            if pos.x > surface.x || pos.y > surface.y || size.x == 0 || size.y == 0
                            {
                                continue;
                            }

                            render_pass.set_scissor_rect(pos.x, pos.y, size.x, size.y);
                        }
                        None => {
                            render_pass.set_scissor_rect(0, 0, surface.x, surface.y);
                        }
                    }
                }

                let bindgroup = command
                    .bind_group_entry
                    .map(|entry| self.texture_bindgroup_cache.get(&entry))
                    .unwrap_or(&self.texture_bindgroup_cache.default);

                render_pass.set_bind_group(0, bindgroup, &[]);
                render_pass.draw_indexed(command.index_range.clone(), 0, 0..1);
            }
        }
    }

    fn update_buffers(&mut self, device: &wgpu::Device, paint: &PaintDom) {
        profiling::scope!("update_buffers");

        self.vertices.clear();
        self.indices.clear();
        self.commands.clear();
        self.texture_bindgroup_cache.clear();

        let commands = paint
            .layers()
            .iter()
            .flat_map(|layer| &layer.calls)
            .map(|call| {
                let vertices = call.vertices.iter().map(|vertex| Vertex {
                    pos: vertex.position,
                    texcoord: vertex.texcoord,
                    color: vertex.color,
                });

                let base = self.vertices.len() as u32;
                let indices = call.indices.iter().map(|&index| base + index as u32);

                let start = self.indices.len() as u32;
                let end = start + indices.len() as u32;

                self.vertices.extend(vertices);
                self.indices.extend(indices);

                let bind_group_entry = call
                    .texture
                    .and_then(|id| match id {
                        TextureId::Managed(managed) => {
                            let texture = self.managed_textures.get(&managed)?;
                            Some((
                                id,
                                &texture.view,
                                texture.min_filter,
                                texture.mag_filter,
                                wgpu::FilterMode::Nearest,
                                texture.address_mode,
                            ))
                        }
                        TextureId::User(bits) => {
                            let index = Index::from_bits(bits)?;
                            let texture = self.textures.get(index)?;
                            Some((
                                id,
                                &texture.view,
                                texture.min_filter,
                                texture.mag_filter,
                                texture.mipmap_filter,
                                texture.address_mode,
                            ))
                        }
                    })
                    .map(
                        |(id, view, min_filter, mag_filter, mipmap_filter, address_mode)| {
                            let entry = TextureBindgroupCacheEntry {
                                id,
                                min_filter,
                                mag_filter,
                                mipmap_filter,
                                address_mode,
                            };
                            self.texture_bindgroup_cache.update(
                                device,
                                entry,
                                view,
                                &self.samplers,
                            );
                            entry
                        },
                    );

                DrawCommand {
                    index_range: start..end,
                    bind_group_entry,
                    pipeline: call.pipeline,
                    clip: call.clip,
                }
            });

        self.commands.extend(commands);
    }

    fn update_textures(&mut self, device: &wgpu::Device, paint: &PaintDom, queue: &wgpu::Queue) {
        profiling::scope!("update_textures");

        for (id, texture) in paint.textures() {
            self.managed_textures
                .entry(id)
                .or_insert_with(|| GpuManagedTexture::new(device, queue, texture));
        }

        for (id, change) in paint.texture_edits() {
            match change {
                TextureChange::Added => {
                    let texture = paint.texture(id).unwrap();
                    self.managed_textures
                        .insert(id, GpuManagedTexture::new(device, queue, texture));
                }

                TextureChange::Removed => {
                    self.managed_textures.remove(&id);
                }

                TextureChange::Modified => {
                    if let Some(existing) = self.managed_textures.get_mut(&id) {
                        let texture = paint.texture(id).unwrap();
                        existing.update(device, queue, texture);
                    }
                }
            }
        }
    }
}

struct DrawCommand {
    index_range: Range<u32>,
    bind_group_entry: Option<TextureBindgroupCacheEntry>,
    pipeline: Pipeline,
    clip: Option<Rect>,
}

fn make_main_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::RenderPipeline {
    let main_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Main Shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/main.wgsl").into()),
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("yakui Main Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &main_shader,
            entry_point: "vs_main",
            compilation_options: Default::default(),
            buffers: &[Vertex::DESCRIPTOR],
        },
        fragment: Some(wgpu::FragmentState {
            module: &main_shader,
            entry_point: "fs_main",
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        },
        multiview: None,
    })
}

fn make_text_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::RenderPipeline {
    let text_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Text Shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/text.wgsl").into()),
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("yakui Text Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &text_shader,
            entry_point: "vs_main",
            compilation_options: Default::default(),
            buffers: &[Vertex::DESCRIPTOR],
        },
        fragment: Some(wgpu::FragmentState {
            module: &text_shader,
            entry_point: "fs_main",
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        },
        multiview: None,
    })
}
