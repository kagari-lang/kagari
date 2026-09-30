use super::Baseline;
pub(super) const BASELINE: &[Baseline] = &[
    Baseline {
        name: "set_from_identity_true_native",
        steps: 49,
        effects: &[("input", 17), ("constructed", 37), ("done", 47)],
        holes: &[],
    },
    Baseline {
        name: "set_from_identity_true_proxy",
        steps: 67,
        effects: &[
            ("input", 18),
            ("iter", 23),
            ("constructed", 55),
            ("done", 65),
        ],
        holes: &[],
    },
    Baseline {
        name: "set_from_identity_true_generic",
        steps: 52,
        effects: &[("input", 17), ("constructed", 40), ("done", 50)],
        holes: &[],
    },
    Baseline {
        name: "set_from_identity_true_dynamic",
        steps: 67,
        effects: &[
            ("input", 18),
            ("iter", 23),
            ("constructed", 55),
            ("done", 65),
        ],
        holes: &[],
    },
    Baseline {
        name: "set_from_identity_false_native",
        steps: 150,
        effects: &[("input", 29), ("constructed", 73), ("done", 148)],
        holes: &[],
    },
    Baseline {
        name: "set_from_identity_false_proxy",
        steps: 220,
        effects: &[
            ("input", 30),
            ("iter", 35),
            ("next", 60),
            ("next", 79),
            ("next", 98),
            ("next", 117),
            ("constructed", 143),
            ("done", 218),
        ],
        holes: &[],
    },
    Baseline {
        name: "set_from_identity_false_generic",
        steps: 153,
        effects: &[("input", 29), ("constructed", 76), ("done", 151)],
        holes: &[],
    },
    Baseline {
        name: "set_from_identity_false_dynamic",
        steps: 220,
        effects: &[
            ("input", 30),
            ("iter", 35),
            ("next", 60),
            ("next", 79),
            ("next", 98),
            ("next", 117),
            ("constructed", 143),
            ("done", 218),
        ],
        holes: &[],
    },
];
