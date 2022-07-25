use yakui_core::{
    dom::Dom,
    geometry::Rect,
    glam::{vec2, Vec2, Vec4},
    layout::LayoutDom,
    paint::{PaintDom, PaintMesh, Vertex},
    widget::Widget,
    Response, TextureId,
};

use crate::{shorthand::pad, util::widget_children, widgets::pad::Pad};

#[derive(Debug)]
pub struct Ninepatch {
    texture: TextureId,
    /// Texture margins in pixels around the central ninepatch region, before
    /// scaling.
    margins: Pad,
    scale: f32,
}

impl Ninepatch {
    pub fn new(texture: TextureId, margins: Pad, scale: f32) -> Self {
        Self {
            texture,
            margins,
            scale,
        }
    }

    pub fn show(self, children: impl FnOnce()) -> Response<NinepatchWidget> {
        let scaled_margins = {
            let mut m = self.margins;
            m.left *= self.scale;
            m.top *= self.scale;
            m.right *= self.scale;
            m.bottom *= self.scale;
            m
        };

        widget_children::<NinepatchWidget, _>(
            || {
                pad(scaled_margins, children);
            },
            self,
        )
    }
}

#[derive(Debug)]
pub struct NinepatchWidget {
    props: Option<Ninepatch>,
}

impl Widget for NinepatchWidget {
    type Props = Ninepatch;
    type Response = ();

    fn new() -> Self {
        Self { props: None }
    }

    fn update(&mut self, props: Self::Props) -> Self::Response {
        self.props = Some(props);
    }

    fn paint(&self, dom: &Dom, layout: &LayoutDom, paint: &mut PaintDom) {
        let props = self.props.as_ref().unwrap();
        let Ninepatch {
            texture,
            margins:
                Pad {
                    left,
                    right,
                    top,
                    bottom,
                    ..
                },
            scale,
        } = *props;

        let rect = layout.get(dom.current()).unwrap().rect;

        let texture = paint.get_texture(texture).unwrap();
        let texture_size = texture.size().as_vec2();

        let top_left = rect.pos();
        let size = rect.size();

        // Vertex coordinates relative to the widget
        let rel_xs = [0.0, left * scale, size.x - right * scale, size.x];
        let rel_ys = [0.0, top * scale, size.y - bottom * scale, size.y];

        // Texture coordinates in pixel units
        let pixel_us = [0.0, left, texture_size.x - right, texture_size.x];
        let pixel_vs = [0.0, top, texture_size.y - bottom, texture_size.y];

        // Convert to 0.0-1.0 range
        let us = pixel_us.map(|pixel_u| pixel_u / texture_size.x);
        let vs = pixel_vs.map(|pixel_v| pixel_v / texture_size.y);

        // Vertices are laid out from left to right, then top to bottom.
        let vertices = rel_ys.into_iter().zip(vs).flat_map(|(y, v)| {
            rel_xs.into_iter().zip(us).map(move |(x, u)| {
                let rel_pos = vec2(x, y);
                let tex_coords = vec2(u, v);

                let pos = top_left + rel_pos;
                Vertex::new(pos, tex_coords, Vec4::splat(1.0))
            })
        });

        // Build rectangles between the vertices.
        let indices = (0..3).flat_map(|i| {
            (0..3).flat_map(move |j| {
                let first = i * 4 + j;
                [first, first + 5, first + 1, first, first + 4, first + 5]
            })
        });

        let mut mesh = PaintMesh::new(vertices, indices);
        mesh.texture = Some((props.texture, Rect::from_pos_size(Vec2::ZERO, texture_size)));
        paint.add_mesh(mesh);

        let node = dom.get_current();
        for &child in &node.children {
            paint.paint(dom, layout, child);
        }
    }
}
