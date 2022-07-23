use std::borrow::Cow;

use yakui_core::event::{EventInterest, EventResponse, WidgetEvent};
use yakui_core::geometry::Color3;
use yakui_core::input::MouseButton;
use yakui_core::widget::Widget;
use yakui_core::{Alignment, Response};

use crate::colors;
use crate::style::{TextAlignment, TextStyle};
use crate::util::widget;
use crate::widgets::Pad;

use super::RenderText;

/**
A button containing some text.

Responds with [ButtonResponse].

Shorthand:
```rust
# let _handle = yakui_widgets::DocTest::start();
if yakui::button("Hello").clicked {
    println!("The button was clicked");
}
```
*/
#[derive(Debug)]
#[non_exhaustive]
pub struct Button {
    pub text: Cow<'static, str>,
    pub text_style: TextStyle,
    pub padding: Pad,
    pub fill: Color3,
    pub hover_fill: Option<Color3>,
    pub down_fill: Option<Color3>,
}

impl Button {
    pub fn unstyled(text: Cow<'static, str>) -> Self {
        let mut text_style = TextStyle::label();
        text_style.align = TextAlignment::Center;

        Self {
            text,
            text_style,
            padding: Pad::ZERO,
            fill: Color3::GRAY,
            hover_fill: None,
            down_fill: None,
        }
    }

    pub fn styled(text: Cow<'static, str>) -> Self {
        let mut text_style = TextStyle::label();
        text_style.align = TextAlignment::Center;

        Self {
            text,
            text_style,
            padding: Pad::balanced(20.0, 10.0),
            fill: colors::BACKGROUND_3,
            hover_fill: Some(colors::BACKGROUND_3.adjust(1.2)),
            down_fill: Some(colors::BACKGROUND_3.adjust(0.8)),
        }
    }

    pub fn show(self) -> Response<ButtonWidget> {
        widget::<ButtonWidget>(self)
    }
}

#[derive(Debug)]
pub struct ButtonWidget {
    props: Button,
    hovering: bool,
    mouse_down: bool,
    clicked: bool,
}

#[derive(Debug)]
pub struct ButtonResponse {
    pub hovering: bool,
    pub clicked: bool,
}

impl Widget for ButtonWidget {
    type Props = Button;
    type Response = ButtonResponse;

    fn new() -> Self {
        Self {
            props: Button::unstyled(Cow::Borrowed("")),
            hovering: false,
            mouse_down: false,
            clicked: false,
        }
    }

    fn update(&mut self, props: Self::Props) -> Self::Response {
        self.props = props;

        let mut color = self.props.fill;

        if let (Some(fill), true) = (self.props.down_fill, self.mouse_down) {
            color = fill
        } else if let (Some(hover), true) = (self.props.hover_fill, self.hovering) {
            color = hover
        }

        let alignment = match self.props.text_style.align {
            TextAlignment::Start => Alignment::CENTER_LEFT,
            TextAlignment::Center => Alignment::CENTER,
            TextAlignment::End => Alignment::CENTER_RIGHT,
        };

        crate::colored_box_container(color, || {
            crate::pad(self.props.padding, || {
                crate::align(alignment, || {
                    let mut text = RenderText::label(self.props.text.clone());
                    text.style = self.props.text_style.clone();
                    text.show();
                });
            });
        });

        let clicked = self.clicked;
        self.clicked = false;

        Self::Response {
            hovering: self.hovering,
            clicked,
        }
    }

    fn event_interest(&self) -> EventInterest {
        EventInterest::MOUSE
    }

    fn event(&mut self, event: &WidgetEvent) -> EventResponse {
        match event {
            WidgetEvent::MouseEnter => {
                self.hovering = true;
                EventResponse::Sink
            }
            WidgetEvent::MouseLeave => {
                self.hovering = false;
                EventResponse::Sink
            }
            WidgetEvent::MouseButtonChanged(MouseButton::One, down) => {
                if *down {
                    self.mouse_down = true;
                    EventResponse::Sink
                } else if self.mouse_down {
                    self.mouse_down = false;
                    self.clicked = true;
                    EventResponse::Sink
                } else {
                    EventResponse::Bubble
                }
            }
            WidgetEvent::MouseButtonChangedOutside(MouseButton::One, down) => {
                if !*down {
                    self.mouse_down = false;
                }

                EventResponse::Bubble
            }
            _ => EventResponse::Bubble,
        }
    }
}
