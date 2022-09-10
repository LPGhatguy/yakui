//! Contains terse defaults for the most common widgets.
//!
//! Each function in this module is easy to read in order to enable extending a
//! widget if its defaults don't work for you.

use std::borrow::Cow;

use yakui_core::geometry::{Color, Constraints, Vec2};
use yakui_core::{Alignment, ManagedTextureId, Response};

use crate::widgets::{
    Align, AlignWidget, Button, ButtonWidget, Checkbox, CheckboxWidget, ColoredBox,
    ColoredBoxWidget, ConstrainedBox, ConstrainedBoxWidget, Draggable, DraggableWidget, Flexible,
    FlexibleWidget, Image, ImageWidget, List, ListWidget, NineSlice, NineSliceWidget, Offset,
    OffsetWidget, Pad, PadWidget, Scrollable, ScrollableWidget, State, StateWidget, Text, TextBox,
    TextBoxWidget, TextWidget,
};

/// See [List].
pub fn column<F: FnOnce()>(children: F) -> Response<ListWidget> {
    List::column().show(children)
}

/// See [List].
pub fn row<F: FnOnce()>(children: F) -> Response<ListWidget> {
    List::row().show(children)
}

/// See [Align].
pub fn center<F: FnOnce()>(children: F) -> Response<AlignWidget> {
    Align::center().show(children)
}

/// See [Align].
pub fn align<F: FnOnce()>(alignment: Alignment, children: F) -> Response<AlignWidget> {
    Align::new(alignment).show(children)
}

/// See [Button].
pub fn button<S: Into<Cow<'static, str>>>(text: S) -> Response<ButtonWidget> {
    Button::styled(text.into()).show()
}

/// See [ColoredBox].
pub fn colored_box<S: Into<Vec2>>(color: Color, size: S) -> Response<ColoredBoxWidget> {
    ColoredBox::sized(color, size.into()).show()
}

/// See [ColoredBox].
pub fn colored_box_container<F: FnOnce()>(color: Color, children: F) -> Response<ColoredBoxWidget> {
    ColoredBox::container(color).show_children(children)
}

/// See [Image].
pub fn image(image: ManagedTextureId, size: Vec2) -> Response<ImageWidget> {
    Image::new(image, size).show()
}

/// See [Pad].
pub fn pad<F: FnOnce()>(padding: Pad, children: F) -> Response<PadWidget> {
    padding.show(children)
}

/// See [Text].
pub fn text<S: Into<Cow<'static, str>>>(size: f32, text: S) -> Response<TextWidget> {
    Text::new(size, text.into()).show()
}

/// See [Text].
pub fn label<S: Into<Cow<'static, str>>>(text: S) -> Response<TextWidget> {
    Text::label(text.into()).show()
}

/// See [TextBox].
pub fn textbox<S: Into<String>>(text: S) -> Response<TextBoxWidget> {
    TextBox::new(text.into()).show()
}

/// See [Flexible].
pub fn flexible<F: FnOnce()>(flex: u32, children: F) -> Response<FlexibleWidget> {
    Flexible::new(flex).show(children)
}

/// See [Flexible].
pub fn expanded<F: FnOnce()>(children: F) -> Response<FlexibleWidget> {
    Flexible::expanded().show(children)
}

/// See [ConstrainedBox].
pub fn constrained<F: FnOnce()>(
    constraints: Constraints,
    children: F,
) -> Response<ConstrainedBoxWidget> {
    ConstrainedBox::new(constraints).show(children)
}

/// See [Checkbox].
pub fn checkbox(checked: bool) -> Response<CheckboxWidget> {
    Checkbox::new(checked).show()
}

/// See [Offset].
pub fn offset<F: FnOnce()>(offset: Vec2, children: F) -> Response<OffsetWidget> {
    Offset::new(offset).show(children)
}

/// See [Draggable].
pub fn draggable<F: FnOnce()>(children: F) -> Response<DraggableWidget> {
    Draggable::new().show(children)
}

/// See [NineSlice].
pub fn nineslice(
    texture: ManagedTextureId,
    margins: Pad,
    scale: f32,
    children: impl FnOnce(),
) -> Response<NineSliceWidget> {
    NineSlice::new(texture, margins, scale).show(children)
}

/// See [Scrollable].
pub fn scroll_vertical(children: impl FnOnce()) -> Response<ScrollableWidget> {
    Scrollable::vertical().show(children)
}

pub fn use_state<F, T: 'static>(default: F) -> Response<StateWidget<T>>
where
    F: FnOnce() -> T + 'static,
{
    State::new(default).show()
}
