#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod buffer;
mod descriptors;
mod util;
mod vulkan_context;
mod vulkan_texture;

use ash::util::read_spv;
pub use ash::vk;
use buffer::Buffer;
use bytemuck::{bytes_of, Pod, Zeroable};
pub use descriptors::Descriptors;
use std::{collections::HashMap, ffi::CStr, io::Cursor};
pub use vulkan_context::VulkanContext;
use vulkan_texture::{UploadQueue, NO_TEXTURE_ID};
pub use vulkan_texture::{VulkanTexture, VulkanTextureCreateInfo};

use yakui_core as yakui;
use yakui_core::geometry::UVec2;
use yakui_core::{paint::Vertex as YakuiVertex, ManagedTextureId};

/// A struct wrapping everything needed to render yakui on Vulkan. This will be your main entry point.
///
/// Uses a simple descriptor system that requires Vulkan 1.2 and some extensions to be enabled. See the [crate level documentation](`crate`)
/// for details.
///
/// Construct the struct by populating a [`VulkanContext`] with the relevant handles to your Vulkan renderer and
/// call [`YakuiVulkan::new()`].
///
/// Make sure to call [`YakuiVulkan::cleanup()`].
pub struct YakuiVulkan {
    /// The pipeline layout used to draw
    pipeline_layout: vk::PipelineLayout,
    /// The graphics pipeline used to draw images
    image_pipeline: vk::Pipeline,
    /// The graphics pipeline used to draw text
    text_pipeline: vk::Pipeline,
    /// A single index buffer, shared between all draw calls
    index_buffer: Buffer<u32>,
    /// A single vertex buffer, shared between all draw calls
    vertex_buffer: Buffer<Vertex>,
    /// Have we synced the first textures from yakui?
    initial_textures_synced: bool,
    /// Textures owned by yakui
    yakui_managed_textures: HashMap<ManagedTextureId, VulkanTexture>,
    /// Textures owned by the user
    user_textures: thunderdome::Arena<VulkanTexture>,
    /// A wrapper around descriptor set functionality
    descriptors: Descriptors,
    uploads: UploadQueue,
}

/// Vulkan configuration
#[non_exhaustive]
#[derive(Default, Clone, Copy)]
pub struct Options {
    /// Indicates that VK_KHR_dynamic_rendering is enabled and should be used with the given format
    pub dynamic_rendering_format: Option<vk::Format>,
    /// Render pass that the GUI will be drawn in. Ignored if `dynamic_rendering_format` is set.
    pub render_pass: vk::RenderPass,
    /// Subpass that the GUI will be drawn in. Ignored if `dynamic_rendering_format` is set.
    pub subpass: u32,
}

#[derive(Clone, Copy, Debug)]
/// A single draw call to render a yakui mesh
struct DrawCall {
    index_offset: u32,
    index_count: u32,
    clip: Option<yakui::geometry::Rect>,
    texture_id: u32,
    pipeline: yakui::paint::Pipeline,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
/// Push constant used to determine texture and workflow
struct PushConstant {
    texture_id: u32,
}

unsafe impl Zeroable for PushConstant {}
unsafe impl Pod for PushConstant {}

impl PushConstant {
    pub fn new(texture_id: u32) -> Self {
        Self { texture_id }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct Vertex {
    position: yakui::geometry::Vec2,
    texcoord: yakui::geometry::Vec2,
    color: yakui::geometry::Vec4,
}

impl From<&YakuiVertex> for Vertex {
    fn from(y: &YakuiVertex) -> Self {
        Self {
            position: y.position,
            texcoord: y.texcoord,
            color: y.color,
        }
    }
}

impl YakuiVulkan {
    /// Create a new [`YakuiVulkan`] instance
    ///
    /// ## Safety
    /// - `vulkan_context` must have valid members
    /// - the members of `render_surface` must have been created with the same [`ash::Device`] as `vulkan_context`.
    pub fn new(vulkan_context: &VulkanContext, options: Options) -> Self {
        let device = vulkan_context.device;
        let descriptors = Descriptors::new(vulkan_context);

        let index_buffer = Buffer::new(vulkan_context, vk::BufferUsageFlags::INDEX_BUFFER, &[]);
        let vertex_buffer = Buffer::new(vulkan_context, vk::BufferUsageFlags::VERTEX_BUFFER, &[]);

        let pipeline_layout = unsafe {
            device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .push_constant_ranges(std::slice::from_ref(
                            &vk::PushConstantRange::default()
                                .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                                .size(std::mem::size_of::<PushConstant>() as _),
                        ))
                        .set_layouts(std::slice::from_ref(&descriptors.layout)),
                    None,
                )
                .unwrap()
        };

        let image_pipeline = create_graphics_pipeline(
            pipeline_layout,
            options,
            device,
            yakui::paint::Pipeline::Main,
        );
        let text_pipeline = create_graphics_pipeline(
            pipeline_layout,
            options,
            device,
            yakui::paint::Pipeline::Text,
        );

        Self {
            descriptors,
            pipeline_layout,
            image_pipeline,
            text_pipeline,
            index_buffer,
            vertex_buffer,
            user_textures: Default::default(),
            yakui_managed_textures: Default::default(),
            initial_textures_synced: false,
            uploads: UploadQueue::new(),
        }
    }

    /// Record transfer commands that must complete before painting this `paint`
    ///
    /// ## Safety
    /// - `vulkan_context` must be the same as the one used to create this [`YakuiVulkan`] instance
    pub unsafe fn transfer(
        &mut self,
        paint: &yakui_core::paint::PaintDom,
        vulkan_context: &VulkanContext,
        cmd: vk::CommandBuffer,
    ) {
        self.update_textures(vulkan_context, paint);
        self.uploads.record(vulkan_context, cmd);
    }

    /// Call when commands recorded by zero or more successive `transfer` calls have been submitted to
    /// a queue
    ///
    /// To support N concurrent frames in flight, call this N times before creating any textures.
    pub fn transfers_submitted(&mut self) {
        self.uploads.phase_submitted();
    }

    /// Call when the commands associated with the oldest call to `transfers_submitted` have finished.
    ///
    /// ## Safety
    ///
    /// Those commands recorded prior to the oldest call to `transfers_submitted` not yet associated
    /// with a call to `transfers_finished` must not be executing.
    pub unsafe fn transfers_finished(&mut self, vulkan_context: &VulkanContext) {
        self.uploads.phase_executed(vulkan_context);
    }

    /// Paint the yakui GUI using the provided [`VulkanContext`]
    ///
    /// ## Safety
    /// - `vulkan_context` must be the same as the one used to create this [`YakuiVulkan`] instance
    /// - `cmd` must be in rendering state, with viewport and scissor dynamic states set.
    pub unsafe fn paint(
        &mut self,
        paint: &yakui_core::paint::PaintDom,
        vulkan_context: &VulkanContext,
        cmd: vk::CommandBuffer,
        resolution: vk::Extent2D,
    ) {
        // If there's nothing to paint, well.. don't paint!
        let layers = paint.layers();
        if layers.iter().all(|layer| layer.calls.is_empty()) {
            return;
        }

        let draw_calls = self.build_draw_calls(vulkan_context, paint);

        self.render(vulkan_context, resolution, cmd, &draw_calls);
    }

    /// Render the draw calls we've built up
    fn render(
        &self,
        vulkan_context: &VulkanContext,
        resolution: vk::Extent2D,
        command_buffer: vk::CommandBuffer,
        draw_calls: &[DrawCall],
    ) {
        let device = vulkan_context.device;

        let surface_size = UVec2::new(resolution.width, resolution.height);

        unsafe {
            device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.image_pipeline,
            );
            let default_scissor = [resolution.into()];

            device.cmd_bind_vertex_buffers(command_buffer, 0, &[self.vertex_buffer.handle], &[0]);
            device.cmd_bind_index_buffer(
                command_buffer,
                self.index_buffer.handle,
                0,
                vk::IndexType::UINT32,
            );
            device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                std::slice::from_ref(&self.descriptors.set),
                &[],
            );
            let mut last_clip = None;
            for draw_call in draw_calls {
                if draw_call.clip != last_clip {
                    last_clip = draw_call.clip;

                    // TODO - do this when processing draw calls
                    match draw_call.clip {
                        Some(rect) => {
                            let pos = rect.pos().as_uvec2();
                            let size = rect.size().as_uvec2();

                            let max = (pos + size).min(surface_size);
                            let size = UVec2::new(
                                max.x.saturating_sub(pos.x),
                                max.y.saturating_sub(pos.y),
                            );

                            // If the scissor rect isn't valid, we can skip this
                            // entire draw call.
                            if pos.x > surface_size.x
                                || pos.y > surface_size.y
                                || size.x == 0
                                || size.y == 0
                            {
                                continue;
                            }

                            let scissors = [vk::Rect2D {
                                offset: vk::Offset2D {
                                    x: pos.x as _,
                                    y: pos.y as _,
                                },
                                extent: vk::Extent2D {
                                    width: size.x,
                                    height: size.y,
                                },
                            }];
                            // If there's a clip, update the scissor
                            device.cmd_set_scissor(command_buffer, 0, &scissors);
                        }
                        None => {
                            // Otherwise, return the scissor back to its default state
                            device.cmd_set_scissor(command_buffer, 0, &default_scissor);
                        }
                    }
                }

                // Instead of using different pipelines for text and non-text rendering, we just
                // pass the "workflow" down through a push constant and branch in the shader.
                device.cmd_push_constants(
                    command_buffer,
                    self.pipeline_layout,
                    vk::ShaderStageFlags::FRAGMENT,
                    0,
                    bytes_of(&PushConstant::new(draw_call.texture_id)),
                );

                // Draw the mesh with the indexes we were provided
                device.cmd_draw_indexed(
                    command_buffer,
                    draw_call.index_count,
                    1,
                    draw_call.index_offset,
                    0,
                    1,
                );
            }
        }
    }

    /// Create and add a "user managed" texture to this [`YakuiVulkan`] instance. Returns a [`yakui::TextureId`] that can be used
    /// to refer to the texture in your GUI code.
    ///
    /// ## Safety
    /// - `vulkan_context` must be the same as the one used to create this instance
    pub fn create_user_texture(
        &mut self,
        vulkan_context: &VulkanContext,
        texture_create_info: VulkanTextureCreateInfo<Vec<u8>>,
    ) -> yakui::TextureId {
        let texture = VulkanTexture::new(
            vulkan_context,
            &mut self.descriptors,
            texture_create_info,
            &mut self.uploads,
        );
        yakui::TextureId::User(self.user_textures.insert(texture).to_bits())
    }

    /// Add a "user managed" texture to this [`YakuiVulkan`] instance from an existing [`ash::vk::Image`].
    /// Returns a [`yakui::TextureId`] that can be used to refer to the texture in your GUI code.
    ///
    /// ## Safety
    /// - `vulkan_context` must be the same as the one used to create this instance
    /// - `image` must have been created from the same `vulkan_context`
    pub fn add_user_texture(&mut self, texture: VulkanTexture) -> yakui::TextureId {
        yakui::TextureId::User(self.user_textures.insert(texture).to_bits())
    }

    /// Clean up all Vulkan related handles on this instance. You'll probably want to call this when the program ends, but
    /// before you've cleaned up your [`ash::Device`], or you'll receive warnings from the Vulkan Validation Layers.
    ///
    /// ## Safety
    /// - After calling this function, this instance will be **unusable**. You **must not** make any further calls on this instance
    ///   or you will have a terrible time.
    /// - `device` must be the same [`ash::Device`] used to create this instance.
    pub unsafe fn cleanup(&mut self, device: &ash::Device) {
        device.device_wait_idle().unwrap();
        self.descriptors.cleanup(device);
        for (_, texture) in self.yakui_managed_textures.drain() {
            texture.cleanup(device);
        }
        for (_, texture) in self.user_textures.drain() {
            texture.cleanup(device);
        }
        device.destroy_pipeline_layout(self.pipeline_layout, None);
        device.destroy_pipeline(self.image_pipeline, None);
        self.index_buffer.cleanup(device);
        self.vertex_buffer.cleanup(device);
        self.uploads.cleanup(device);
    }

    /// Provides access to the descriptors used by `YakuiVulkan` to manage textures.
    /// Only useful for creating a [`crate::VulkanTexture`] from a pre-existing [`ash::vk::Image`]
    pub fn descriptors(&mut self) -> &mut Descriptors {
        &mut self.descriptors
    }

    fn update_textures(&mut self, vulkan_context: &VulkanContext, paint: &yakui::paint::PaintDom) {
        use yakui::paint::TextureChange;
        if !self.initial_textures_synced {
            self.initial_textures_synced = true;
            for (id, texture) in paint.textures() {
                let texture = VulkanTexture::from_yakui_texture(
                    vulkan_context,
                    &mut self.descriptors,
                    texture,
                    &mut self.uploads,
                );
                self.yakui_managed_textures.insert(id, texture);
            }

            return;
        }

        for (id, change) in paint.texture_edits() {
            match change {
                TextureChange::Added => {
                    let texture = paint.texture(id).unwrap();
                    let texture = VulkanTexture::from_yakui_texture(
                        vulkan_context,
                        &mut self.descriptors,
                        texture,
                        &mut self.uploads,
                    );
                    self.yakui_managed_textures.insert(id, texture);
                }

                TextureChange::Removed => {
                    if let Some(removed) = self.yakui_managed_textures.remove(&id) {
                        unsafe {
                            self.uploads.dispose(removed);
                        }
                    }
                }

                TextureChange::Modified => {
                    if let Some(old) = self.yakui_managed_textures.remove(&id) {
                        unsafe {
                            self.uploads.dispose(old);
                        }
                    }
                    let new = paint.texture(id).unwrap();
                    let texture = VulkanTexture::from_yakui_texture(
                        vulkan_context,
                        &mut self.descriptors,
                        new,
                        &mut self.uploads,
                    );
                    self.yakui_managed_textures.insert(id, texture);
                }
            }
        }
    }

    fn build_draw_calls(
        &mut self,
        vulkan_context: &VulkanContext,
        paint: &yakui::paint::PaintDom,
    ) -> Vec<DrawCall> {
        let mut vertices: Vec<Vertex> = Default::default();
        let mut indices: Vec<u32> = Default::default();
        let mut draw_calls: Vec<DrawCall> = Default::default();

        let calls = paint.layers().iter().flat_map(|layer| &layer.calls);

        for call in calls {
            let base = vertices.len() as u32;
            let index_offset = indices.len() as u32;
            let index_count = call.indices.len() as u32;

            for index in &call.indices {
                indices.push(*index as u32 + base);
            }
            for vertex in &call.vertices {
                vertices.push(vertex.into())
            }

            let texture_id = call
                .texture
                .and_then(|id| match id {
                    yakui::TextureId::Managed(managed) => {
                        let texture = self.yakui_managed_textures.get(&managed)?;
                        Some(texture.id)
                    }
                    yakui::TextureId::User(bits) => {
                        let texture = self
                            .user_textures
                            .get(thunderdome::Index::from_bits(bits)?)?;
                        Some(texture.id)
                    }
                })
                .unwrap_or(NO_TEXTURE_ID);

            draw_calls.push(DrawCall {
                index_offset,
                index_count,
                clip: call.clip,
                texture_id,
                pipeline: call.pipeline.into(),
            });
        }

        unsafe {
            self.index_buffer.write(vulkan_context, 0, &indices);
            self.vertex_buffer.write(vulkan_context, 0, &vertices);
        }

        draw_calls
    }
}

fn create_graphics_pipeline(
    pipeline_layout: vk::PipelineLayout,
    options: Options,
    device: &ash::Device,
    yakui_pipeline: yakui::paint::Pipeline,
) -> vk::Pipeline {
    let mut vertex_spv_file = Cursor::new(&include_bytes!("../shaders/main.vert.spv")[..]);
    let mut frag_spv_file = Cursor::new(&include_bytes!("../shaders/main.frag.spv")[..]);

    let vertex_code =
        read_spv(&mut vertex_spv_file).expect("Failed to read vertex shader spv file");
    let vertex_shader_info = vk::ShaderModuleCreateInfo::default().code(&vertex_code);

    let frag_code = read_spv(&mut frag_spv_file).expect("Failed to read fragment shader spv file");
    let frag_shader_info = vk::ShaderModuleCreateInfo::default().code(&frag_code);

    let vertex_shader_module = unsafe {
        device
            .create_shader_module(&vertex_shader_info, None)
            .expect("Vertex shader module error")
    };

    let fragment_shader_module = unsafe {
        device
            .create_shader_module(&frag_shader_info, None)
            .expect("Fragment shader module error")
    };
    let shader_entry_name = unsafe { CStr::from_bytes_with_nul_unchecked(b"main\0") };
    let shader_stage_create_infos = [
        vk::PipelineShaderStageCreateInfo::default()
            .module(vertex_shader_module)
            .name(shader_entry_name)
            .stage(vk::ShaderStageFlags::VERTEX),
        vk::PipelineShaderStageCreateInfo::default()
            .module(fragment_shader_module)
            .name(shader_entry_name)
            .stage(vk::ShaderStageFlags::FRAGMENT),
    ];
    let vertex_input_binding_descriptions = [vk::VertexInputBindingDescription {
        binding: 0,
        stride: std::mem::size_of::<Vertex>() as u32,
        input_rate: vk::VertexInputRate::VERTEX,
    }];

    let vertex_input_attribute_descriptions = [
        // position
        vk::VertexInputAttributeDescription {
            location: 0,
            binding: 0,
            format: vk::Format::R32G32_SFLOAT,
            offset: bytemuck::offset_of!(Vertex, position) as _,
        },
        // UV / texcoords
        vk::VertexInputAttributeDescription {
            location: 1,
            binding: 0,
            format: vk::Format::R32G32_SFLOAT,
            offset: bytemuck::offset_of!(Vertex, texcoord) as _,
        },
        // color
        vk::VertexInputAttributeDescription {
            location: 2,
            binding: 0,
            format: vk::Format::R32G32B32A32_SFLOAT,
            offset: bytemuck::offset_of!(Vertex, color) as _,
        },
    ];

    let vertex_input_state_info = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_attribute_descriptions(&vertex_input_attribute_descriptions)
        .vertex_binding_descriptions(&vertex_input_binding_descriptions);
    let vertex_input_assembly_state_info = vk::PipelineInputAssemblyStateCreateInfo {
        topology: vk::PrimitiveTopology::TRIANGLE_LIST,
        ..Default::default()
    };
    let viewport_state_info = vk::PipelineViewportStateCreateInfo::default()
        .scissor_count(1)
        .viewport_count(1);

    let rasterization_info = vk::PipelineRasterizationStateCreateInfo {
        front_face: vk::FrontFace::COUNTER_CLOCKWISE,
        line_width: 1.0,
        polygon_mode: vk::PolygonMode::FILL,
        ..Default::default()
    };
    let multisample_state_info = vk::PipelineMultisampleStateCreateInfo {
        rasterization_samples: vk::SampleCountFlags::TYPE_1,
        ..Default::default()
    };
    let noop_stencil_state = vk::StencilOpState {
        fail_op: vk::StencilOp::KEEP,
        pass_op: vk::StencilOp::KEEP,
        depth_fail_op: vk::StencilOp::KEEP,
        compare_op: vk::CompareOp::ALWAYS,
        ..Default::default()
    };
    let depth_state_info = vk::PipelineDepthStencilStateCreateInfo {
        depth_test_enable: 1,
        depth_write_enable: 1,
        depth_compare_op: vk::CompareOp::LESS_OR_EQUAL,
        front: noop_stencil_state,
        back: noop_stencil_state,
        max_depth_bounds: 1.0,
        ..Default::default()
    };

    let color_blend_attachment_state = vk::PipelineColorBlendAttachmentState {
        blend_enable: 1,
        src_color_blend_factor: vk::BlendFactor::SRC_ALPHA,
        dst_color_blend_factor: vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
        color_blend_op: vk::BlendOp::ADD,
        src_alpha_blend_factor: vk::BlendFactor::ONE,
        dst_alpha_blend_factor: vk::BlendFactor::ZERO,
        alpha_blend_op: vk::BlendOp::ADD,
        color_write_mask: vk::ColorComponentFlags::RGBA,
    };

    let color_blend_state = vk::PipelineColorBlendStateCreateInfo::default()
        .logic_op(vk::LogicOp::CLEAR)
        .attachments(std::slice::from_ref(&color_blend_attachment_state));

    let dynamic_state = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic_state_info =
        vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_state);

    let mut graphic_pipeline_info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&shader_stage_create_infos)
        .vertex_input_state(&vertex_input_state_info)
        .input_assembly_state(&vertex_input_assembly_state_info)
        .viewport_state(&viewport_state_info)
        .rasterization_state(&rasterization_info)
        .multisample_state(&multisample_state_info)
        .depth_stencil_state(&depth_state_info)
        .color_blend_state(&color_blend_state)
        .dynamic_state(&dynamic_state_info)
        .layout(pipeline_layout);
    let rendering_info_formats;
    let mut rendering_info;

    assert!(
        options.dynamic_rendering_format.is_some() || options.render_pass != vk::RenderPass::null(),
        "either dynamic_rendering_format or render_pass must be set"
    );
    if let Some(format) = options.dynamic_rendering_format {
        rendering_info_formats = [format];
        rendering_info = vk::PipelineRenderingCreateInfo::default()
            .color_attachment_formats(&rendering_info_formats);
        graphic_pipeline_info = graphic_pipeline_info.push_next(&mut rendering_info);
    } else {
        graphic_pipeline_info = graphic_pipeline_info
            .render_pass(options.render_pass)
            .subpass(options.subpass);
    }

    let graphics_pipelines = unsafe {
        device
            .create_graphics_pipelines(vk::PipelineCache::null(), &[graphic_pipeline_info], None)
            .expect("Unable to create graphics pipeline")
    };

    let graphics_pipeline = graphics_pipelines[0];

    unsafe {
        device.destroy_shader_module(vertex_shader_module, None);
        device.destroy_shader_module(fragment_shader_module, None);
    }

    graphics_pipeline
}
