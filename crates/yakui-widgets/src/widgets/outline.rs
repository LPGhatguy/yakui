use crate::widgets::PadResponse;
use crate::{shapes, shorthand::pad, util::widget_children, widgets::pad::Pad};
use yakui_core::geometry::Color;
use yakui_core::{
    widget::{PaintContext, Widget},
    Response,
};

/**
Applies a colored outline around its children.
 */
#[derive(Debug)]
#[must_use = "yakui widgets do nothing if you don't `show` them"]
pub struct Outline {
    color: Color,
    width: f32,
    side: OutlineSide,
}

#[derive(Copy, Clone, Debug)]
pub enum OutlineSide {
    Inside,
    Outside,
}
impl Outline {
    pub fn new(color: Color, width: f32, side: OutlineSide) -> Self {
        Self { color, width, side }
    }

    pub fn show(self, children: impl FnOnce()) -> Response<()> {
        let width = self.width;
        let side = self.side;
        widget_children::<OutlineWidget, _>(
            || match side {
                OutlineSide::Inside => {
                    children();
                }
                OutlineSide::Outside => {
                    pad(Pad::all(width), children);
                }
            },
            self,
        )
    }
}

#[derive(Debug)]
pub struct OutlineWidget {
    props: Option<Outline>,
}

impl Widget for OutlineWidget {
    type Props<'a> = Outline;
    type Response = ();

    fn new() -> Self {
        Self { props: None }
    }

    fn update(&mut self, props: Self::Props<'_>) -> Self::Response {
        self.props = Some(props);
    }

    fn paint(&self, mut ctx: PaintContext<'_>) {
        let props = self.props.as_ref().unwrap();
        let Outline { color, width, .. } = *props;

        let node = ctx.dom.get_current();
        for &child in &node.children {
            ctx.paint(child);
        }

        let rect = ctx.layout.get(ctx.dom.current()).unwrap().rect;
        shapes::outline(ctx.paint, rect, width, color);
    }
}
