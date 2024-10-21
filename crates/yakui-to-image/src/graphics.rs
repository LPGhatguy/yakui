use std::mem::size_of;

use image::RgbaImage;
use yakui_wgpu::SurfaceInfo;

pub struct Graphics {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub format: wgpu::TextureFormat,
}

impl Graphics {
    pub async fn new() -> Self {
        let instance = wgpu::Instance::default();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: Default::default(),
                    label: None,
                },
                None,
            )
            .await
            .unwrap();

        Self {
            device,
            queue,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
        }
    }

    pub fn paint(
        &self,
        yak: &mut yakui_core::Yakui,
        yak_renderer: &mut yakui_wgpu::YakuiWgpu,
    ) -> RgbaImage {
        let viewport = yak.layout_dom().unscaled_viewport();
        let size = wgpu::Extent3d {
            width: viewport.size().x as u32,
            height: viewport.size().y as u32,
            depth_or_array_layers: 1,
        };

        let buffer_size = BufferDimensions::new(size.width, size.height);

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("yakui-to-image output"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            view_formats: &[],
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yakui-to-image return buffer"),
            size: (buffer_size.padded_bytes_per_row * buffer_size.height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Clear"),
            });

        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }

        let clear = encoder.finish();

        let surface = SurfaceInfo {
            format: self.format,
            sample_count: 1,
            color_attachment: &view,
            resolve_target: None,
        };
        let paint_yak = yak_renderer.paint(yak, &self.device, &self.queue, surface);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Copy"),
            });

        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(buffer_size.padded_bytes_per_row),
                    rows_per_image: Some(size.height),
                },
            },
            size,
        );

        let copy = encoder.finish();

        self.queue.submit([clear, paint_yak, copy]);
        let buffer_slice = buffer.slice(..);
        buffer_slice.map_async(wgpu::MapMode::Read, |res| {
            res.unwrap();
        });

        self.device.poll(wgpu::Maintain::Wait);

        let padded_data = buffer_slice.get_mapped_range().to_vec();
        let mut data = Vec::new();
        for chunk in padded_data.chunks(buffer_size.padded_bytes_per_row as usize) {
            data.extend(&chunk[..buffer_size.unpadded_bytes_per_row as usize]);
        }
        RgbaImage::from_raw(size.width, size.height, data).unwrap()
    }
}

// https://github.com/gfx-rs/wgpu/blob/e49ef973111265eb4f6de65a75ee701a90cfa4fb/wgpu/examples/capture/main.rs#L186-L207
#[allow(unused)]
struct BufferDimensions {
    width: u32,
    height: u32,
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
}

impl BufferDimensions {
    fn new(width: u32, height: u32) -> Self {
        let bytes_per_pixel = size_of::<u32>() as u32;
        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row_padding = (align - unpadded_bytes_per_row % align) % align;
        let padded_bytes_per_row = unpadded_bytes_per_row + padded_bytes_per_row_padding;
        Self {
            width,
            height,
            unpadded_bytes_per_row,
            padded_bytes_per_row,
        }
    }
}
