// Observed at 2643293 before native string iterator construction.
use super::Baseline;
pub(super) const BASELINE: &[Baseline] = &[
    Baseline {
        name: "lines_0_false",
        steps: 50,
        effects: &[("source", 6), ("created", 13), ("first", 25), ("done", 48)],
        holes: &[],
    },
    Baseline {
        name: "lines_0_true",
        steps: 50,
        effects: &[("source", 6), ("created", 13), ("first", 25), ("done", 48)],
        holes: &[],
    },
    Baseline {
        name: "lines_1_false",
        steps: 51,
        effects: &[("source", 7), ("created", 14), ("first", 26), ("done", 49)],
        holes: &[],
    },
    Baseline {
        name: "lines_1_true",
        steps: 51,
        effects: &[("source", 7), ("created", 14), ("first", 26), ("done", 49)],
        holes: &[],
    },
    Baseline {
        name: "lines_2_false",
        steps: 51,
        effects: &[("source", 7), ("created", 14), ("first", 26), ("done", 49)],
        holes: &[],
    },
    Baseline {
        name: "lines_2_true",
        steps: 51,
        effects: &[("source", 7), ("created", 14), ("first", 26), ("done", 49)],
        holes: &[],
    },
    Baseline {
        name: "lines_3_false",
        steps: 93,
        effects: &[
            ("source", 9),
            ("created", 16),
            ("first", 28),
            ("item", 46),
            ("item", 66),
            ("done", 91),
        ],
        holes: &[],
    },
    Baseline {
        name: "lines_3_true",
        steps: 93,
        effects: &[
            ("source", 9),
            ("created", 16),
            ("first", 28),
            ("item", 46),
            ("item", 66),
            ("done", 91),
        ],
        holes: &[],
    },
    Baseline {
        name: "lines_4_false",
        steps: 72,
        effects: &[
            ("source", 8),
            ("created", 15),
            ("first", 27),
            ("item", 45),
            ("done", 70),
        ],
        holes: &[],
    },
    Baseline {
        name: "lines_4_true",
        steps: 72,
        effects: &[
            ("source", 8),
            ("created", 15),
            ("first", 27),
            ("item", 45),
            ("done", 70),
        ],
        holes: &[],
    },
    Baseline {
        name: "lines_5_false",
        steps: 51,
        effects: &[("source", 7), ("created", 14), ("first", 26), ("done", 49)],
        holes: &[],
    },
    Baseline {
        name: "lines_5_true",
        steps: 51,
        effects: &[("source", 7), ("created", 14), ("first", 26), ("done", 49)],
        holes: &[],
    },
];
