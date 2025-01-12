use yakui::{pad, widgets::Pad};

use bootstrap::ExampleState;
use yakui_core::geometry::Color;
use yakui_widgets::widgets::OutlineSide::{Inside, Outside};
use yakui_widgets::{button, column, outline, text};

pub fn run(_state: &mut ExampleState) {
    column(|| {
        pad(Pad::all(20.0), || {
            outline(Color::RED, 5.0, Inside, || {
                text(20.0, "internal outline");
            });
        });
        pad(Pad::all(20.0), || {
            outline(Color::RED, 5.0, Outside, || {
                text(20.0, "external outline");
            });
        });
        pad(Pad::all(20.0), || {
            outline(Color::RED, 1.0, Outside, || {
                text(20.0, "varying width");
            });
        });
        pad(Pad::all(20.0), || {
            outline(Color::RED, 2.0, Outside, || {
                text(20.0, "varying width");
            });
        });
        pad(Pad::all(20.0), || {
            outline(Color::GREEN, 3.0, Outside, || {
                text(20.0, "other colors");
            });
        });
        pad(Pad::all(20.0), || {
            outline(Color::GREEN, 3.0, Outside, || {
                button("other widgets");
            });
        });
    });
}

fn main() {
    bootstrap::start(run as fn(&mut ExampleState));
}
