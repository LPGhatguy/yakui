use yakui::widgets::List;
use yakui::{colored_box, label, Color, CrossAxisAlignment, MainAxisAlignment};

use bootstrap::ExampleState;

pub fn run(state: &mut ExampleState) {
    let alignments = [
        MainAxisAlignment::Start,
        MainAxisAlignment::Center,
        MainAxisAlignment::End,
    ];

    let index = (state.time.floor() as usize) % alignments.len();
    let alignment = alignments[index];

    let mut row = List::row();
    row.main_axis_alignment = alignment;
    row.cross_axis_alignment = CrossAxisAlignment::Center;
    row.show(|| {
        colored_box(Color::RED, [100.0, 100.0]);
        label(format!("MainAxisAlignment::{alignment:?}"));
        colored_box(Color::BLUE, [100.0, 100.0]);
    });
}

fn main() {
    bootstrap::start(run as fn(&mut ExampleState));
}
