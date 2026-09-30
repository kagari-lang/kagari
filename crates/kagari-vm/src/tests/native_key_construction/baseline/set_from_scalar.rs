use super::Baseline;
pub(super) const BASELINE: &[Baseline] = &[
    Baseline {
        name: "set_from_scalar_true_native",
        steps: 46,
        effects: &[("input", 14), ("constructed", 34), ("done", 44)],
        holes: &[],
    },
    Baseline {
        name: "set_from_scalar_true_proxy",
        steps: 64,
        effects: &[
            ("input", 15),
            ("iter", 20),
            ("constructed", 52),
            ("done", 62),
        ],
        holes: &[],
    },
    Baseline {
        name: "set_from_scalar_true_generic",
        steps: 49,
        effects: &[("input", 14), ("constructed", 37), ("done", 47)],
        holes: &[],
    },
    Baseline {
        name: "set_from_scalar_true_dynamic",
        steps: 64,
        effects: &[
            ("input", 15),
            ("iter", 20),
            ("constructed", 52),
            ("done", 62),
        ],
        holes: &[],
    },
    Baseline {
        name: "set_from_scalar_false_native",
        steps: 147,
        effects: &[("input", 26), ("constructed", 70), ("done", 145)],
        holes: &[],
    },
    Baseline {
        name: "set_from_scalar_false_proxy",
        steps: 217,
        effects: &[
            ("input", 27),
            ("iter", 32),
            ("next", 57),
            ("next", 76),
            ("next", 95),
            ("next", 114),
            ("constructed", 140),
            ("done", 215),
        ],
        holes: &[],
    },
    Baseline {
        name: "set_from_scalar_false_generic",
        steps: 150,
        effects: &[("input", 26), ("constructed", 73), ("done", 148)],
        holes: &[],
    },
    Baseline {
        name: "set_from_scalar_false_dynamic",
        steps: 217,
        effects: &[
            ("input", 27),
            ("iter", 32),
            ("next", 57),
            ("next", 76),
            ("next", 95),
            ("next", 114),
            ("constructed", 140),
            ("done", 215),
        ],
        holes: &[],
    },
];
