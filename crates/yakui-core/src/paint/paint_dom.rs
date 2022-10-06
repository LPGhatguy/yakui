use std::collections::HashMap;

use glam::Vec2;
use thunderdome::Arena;

use crate::dom::Dom;
use crate::geometry::Rect;
use crate::id::{ManagedTextureId, WidgetId};
use crate::layout::LayoutDom;
use crate::widget::PaintContext;

use super::primitives::{PaintCall, PaintMesh, PaintRect, Vertex};
use super::texture::{Texture, TextureChange};

#[rustfmt::skip]
const RECT_POS: [[f32; 2]; 4] = [
    [0.0, 0.0],
    [0.0, 1.0],
    [1.0, 1.0],
    [1.0, 0.0]
];

#[rustfmt::skip]
const RECT_INDEX: [u16; 6] = [
    0, 1, 2,
    3, 0, 2,
];

/// Contains all information about how to paint the current set of widgets.
#[derive(Debug)]
pub struct PaintDom {
    textures: Arena<Texture>,
    texture_edits: HashMap<ManagedTextureId, TextureChange>,
    surface_size: Vec2,
    unscaled_viewport: Rect,
    scale_factor: f32,

    calls: Vec<PaintCall>,
    clip_stack: Vec<Rect>,
}

impl PaintDom {
    /// Create a new, empty Paint DOM.
    pub fn new() -> Self {
        Self {
            textures: Arena::new(),
            texture_edits: HashMap::new(),
            surface_size: Vec2::ONE,
            unscaled_viewport: Rect::ONE,
            scale_factor: 1.0,
            calls: Vec::new(),
            clip_stack: Vec::new(),
        }
    }

    /// Prepares the PaintDom to be updated for the frame.
    pub fn start(&mut self) {
        self.texture_edits.clear();
        self.clip_stack.clear();
    }

    /// Returns the size of the surface that is being painted onto.
    pub fn surface_size(&self) -> Vec2 {
        self.surface_size
    }

    /// Set the size of the surface that yakui is being rendered on.
    pub(crate) fn set_surface_size(&mut self, size: Vec2) {
        self.surface_size = size;
    }

    pub(crate) fn set_unscaled_viewport(&mut self, viewport: Rect) {
        self.unscaled_viewport = viewport;
    }

    pub(crate) fn set_scale_factor(&mut self, scale_factor: f32) {
        self.scale_factor = scale_factor;
    }

    /// Paint a specific widget. This function is usually called as part of an
    /// implementation of [`Widget::paint`][crate::widget::Widget::paint].
    ///
    /// Must only be called once per widget per paint pass.
    pub fn paint(&mut self, dom: &Dom, layout: &LayoutDom, id: WidgetId) {
        profiling::scope!("PaintDom::paint");

        let layout_node = layout.get(id).unwrap();
        if layout_node.clipping_enabled {
            self.push_clip(layout_node.rect);
        }

        dom.enter(id);

        let context = PaintContext {
            dom,
            layout,
            paint: self,
        };
        let node = dom.get(id).unwrap();
        node.widget.paint(context);

        dom.exit(id);

        if layout_node.clipping_enabled {
            self.pop_clip();
        }
    }

    /// Paint all of the widgets in the given DOM.
    pub fn paint_all(&mut self, dom: &Dom, layout: &LayoutDom) {
        profiling::scope!("PaintDom::paint_all");
        log::debug!("PaintDom:paint_all()");

        self.calls.clear();

        let context = PaintContext {
            dom,
            layout,
            paint: self,
        };

        let node = dom.get(dom.root()).unwrap();
        node.widget.paint(context);
    }

    /// Add a texture to the Paint DOM, returning an ID that can be used to
    /// reference it later.
    pub fn add_texture(&mut self, texture: Texture) -> ManagedTextureId {
        let id = ManagedTextureId::new(self.textures.insert(texture));
        self.texture_edits.insert(id, TextureChange::Added);
        id
    }

    /// Remove a texture from the Paint DOM.
    pub fn remove_texture(&mut self, id: ManagedTextureId) {
        self.textures.remove(id.index());
        self.texture_edits.insert(id, TextureChange::Removed);
    }

    /// Retrieve a texture by its ID, if it exists.
    pub fn texture(&self, id: ManagedTextureId) -> Option<&Texture> {
        self.textures.get(id.index())
    }

    /// Retrieves a mutable reference to a texture by its ID.
    pub fn texture_mut(&mut self, id: ManagedTextureId) -> Option<&mut Texture> {
        self.textures.get_mut(id.index())
    }

    /// Mark a texture as modified so that changes can be detected.
    pub fn mark_texture_modified(&mut self, id: ManagedTextureId) {
        self.texture_edits.insert(id, TextureChange::Modified);
    }

    /// Returns an iterator over all textures known to the Paint DOM.
    pub fn textures(&self) -> impl Iterator<Item = (ManagedTextureId, &Texture)> {
        self.textures
            .iter()
            .map(|(index, texture)| (ManagedTextureId::new(index), texture))
    }

    /// Iterates over the list of changes that happened to yakui-managed
    /// textures this frame.
    ///
    /// This is useful for renderers that need to upload or remove GPU resources
    /// related to textures.
    pub fn texture_edits(&self) -> impl Iterator<Item = (ManagedTextureId, TextureChange)> + '_ {
        self.texture_edits.iter().map(|(&id, &edit)| (id, edit))
    }

    /// Returns a list of paint calls that could be used to draw the UI.
    pub fn calls(&self) -> &[PaintCall] {
        self.calls.as_slice()
    }

    /// Add a mesh to be painted.
    pub fn add_mesh<V, I>(&mut self, mesh: PaintMesh<V, I>)
    where
        V: IntoIterator<Item = Vertex>,
        I: IntoIterator<Item = u16>,
    {
        profiling::scope!("PaintDom::add_mesh");

        let texture_id = mesh.texture.map(|(index, _rect)| index);

        let current_clip = self.clip_stack.last().copied();
        let call = match self.calls.last_mut() {
            Some(call)
                if call.texture == texture_id
                    && call.pipeline == mesh.pipeline
                    && call.clip == current_clip =>
            {
                call
            }
            _ => {
                let mut call = PaintCall::new();
                call.texture = texture_id;
                call.pipeline = mesh.pipeline;
                call.clip = current_clip;

                self.calls.push(call);
                self.calls.last_mut().unwrap()
            }
        };

        let indices = mesh
            .indices
            .into_iter()
            .map(|index| index + call.vertices.len() as u16);
        call.indices.extend(indices);

        let vertices = mesh.vertices.into_iter().map(|mut vertex| {
            let mut pos = vertex.position * self.scale_factor;
            pos += self.unscaled_viewport.pos();
            pos /= self.surface_size;
            vertex.position = pos;
            vertex
        });
        call.vertices.extend(vertices);
    }

    /// Add a rectangle to be painted. This is a convenience function over
    /// [`PaintDom::add_mesh`].
    pub fn add_rect(&mut self, rect: PaintRect) {
        let size = rect.rect.size();
        let pos = rect.rect.pos();
        let color = rect.color.to_linear();
        let texture_rect = match rect.texture {
            Some((_index, rect)) => rect,
            None => Rect::from_pos_size(Vec2::ZERO, Vec2::ONE),
        };

        let vertices = RECT_POS.map(Vec2::from).map(|vert| {
            Vertex::new(
                vert * size + pos,
                vert * texture_rect.size() + texture_rect.pos(),
                color,
            )
        });

        let mut mesh = PaintMesh::new(vertices, RECT_INDEX);
        mesh.texture = rect.texture;
        mesh.pipeline = rect.pipeline;

        self.add_mesh(mesh);
    }

    /// Use the given region as the clipping rect for all following paint calls.
    fn push_clip(&mut self, region: Rect) {
        let mut unscaled = Rect::from_pos_size(
            region.pos() * self.scale_factor,
            region.size() * self.scale_factor,
        );

        if let Some(previous) = self.clip_stack.last() {
            unscaled = unscaled.constrain(*previous);
        }

        self.clip_stack.push(unscaled);
    }

    /// Pop the most recent clip region, restoring the previous clipping rect.
    fn pop_clip(&mut self) {
        let top = self.clip_stack.pop();
        debug_assert!(
            top.is_some(),
            "cannot call pop_clip without a corresponding push_clip call"
        );
    }
}
