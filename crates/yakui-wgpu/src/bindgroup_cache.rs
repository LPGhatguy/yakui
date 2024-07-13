use crate::samplers::Samplers;
use std::collections::HashMap;
use yakui_core::TextureId;

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub(crate) struct TextureBindgroupCacheEntry {
    pub id: TextureId,
    pub min_filter: wgpu::FilterMode,
    pub mag_filter: wgpu::FilterMode,
    pub mipmap_filter: wgpu::FilterMode,
    pub address_mode: wgpu::AddressMode,
}

pub(crate) struct TextureBindgroupCache {
    cache: HashMap<TextureBindgroupCacheEntry, wgpu::BindGroup>,
    layout: wgpu::BindGroupLayout,
    pub default: wgpu::BindGroup,
}

impl TextureBindgroupCache {
    pub fn new(layout: wgpu::BindGroupLayout, default: wgpu::BindGroup) -> Self {
        Self {
            cache: HashMap::new(),
            layout,
            default,
        }
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn update(
        &mut self,
        device: &wgpu::Device,
        entry: TextureBindgroupCacheEntry,
        view: &wgpu::TextureView,
        samplers: &Samplers,
    ) {
        self.cache.entry(entry).or_insert_with(|| {
            bindgroup(
                device,
                &self.layout,
                samplers,
                view,
                entry.min_filter,
                entry.mag_filter,
                entry.mipmap_filter,
                entry.address_mode,
            )
        });
    }

    pub fn get(&self, entry: &TextureBindgroupCacheEntry) -> &wgpu::BindGroup {
        self.cache.get(entry).unwrap_or(&self.default)
    }
}

pub fn bindgroup(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    samplers: &Samplers,
    view: &wgpu::TextureView,
    min_filter: wgpu::FilterMode,
    mag_filter: wgpu::FilterMode,
    mipmap_filter: wgpu::FilterMode,
    address_mode: wgpu::AddressMode,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("yakui Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(samplers.get(
                    min_filter,
                    mag_filter,
                    mipmap_filter,
                    address_mode,
                )),
            },
        ],
    })
}
