use yakui::widgets::{List, Pad};
use yakui::{align, colored_box_container, label, pad, Alignment, Color, MainAxisSize};

struct Entry {
    name: &'static str,
    score: u32,
    currency: u32,
}

pub fn run() {
    let data = [
        Entry {
            name: "First Person",
            score: 802,
            currency: 2,
        },
        Entry {
            name: "Second Person",
            score: 40,
            currency: 3000,
        },
        Entry {
            name: "Third Person Longer Name",
            score: 0,
            currency: 3,
        },
    ];

    align(Alignment::TOP_RIGHT, || {
        pad(Pad::all(8.0), || {
            colored_box_container(Color::hex(0x444444), || {
                let mut row = List::row();
                row.main_axis_size = MainAxisSize::Min;
                row.show(|| {
                    stat_column(|| {
                        label("Name");
                        for datum in &data {
                            label(datum.name);
                        }
                    });

                    stat_column(|| {
                        label("Score");
                        for datum in &data {
                            label(format!("{}", datum.score));
                        }
                    });

                    stat_column(|| {
                        label("Money");
                        for datum in &data {
                            label(format!("{}", datum.currency));
                        }
                    });
                });
            });
        });
    });
}

fn stat_column(children: impl FnOnce()) {
    let mut col = List::column();
    col.main_axis_size = MainAxisSize::Min;
    col.show(children);
}

fn main() {
    bootstrap::start(run as fn());
}
