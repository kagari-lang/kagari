use super::Baseline;
pub(super) const BASELINE: &[Baseline] = &[
    Baseline {
        name: "map_from_scalar_true_native",
        steps: 46,
        effects: &[("input", 14), ("constructed", 34), ("done", 44)],
        holes: &[],
    },
    Baseline {
        name: "map_from_scalar_true_proxy",
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
        name: "map_from_scalar_true_generic",
        steps: 49,
        effects: &[("input", 14), ("constructed", 37), ("done", 47)],
        holes: &[],
    },
    Baseline {
        name: "map_from_scalar_true_dynamic",
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
        name: "map_from_scalar_false_native",
        steps: 200,
        effects: &[("input", 34), ("constructed", 94), ("done", 198)],
        holes: &[],
    },
    Baseline {
        name: "map_from_scalar_false_proxy",
        steps: 270,
        effects: &[
            ("input", 35),
            ("iter", 40),
            ("next", 65),
            ("next", 88),
            ("next", 111),
            ("next", 134),
            ("constructed", 164),
            ("done", 268),
        ],
        holes: &[],
    },
    Baseline {
        name: "map_from_scalar_false_generic",
        steps: 203,
        effects: &[("input", 34), ("constructed", 97), ("done", 201)],
        holes: &[],
    },
    Baseline {
        name: "map_from_scalar_false_dynamic",
        steps: 270,
        effects: &[
            ("input", 35),
            ("iter", 40),
            ("next", 65),
            ("next", 88),
            ("next", 111),
            ("next", 134),
            ("constructed", 164),
            ("done", 268),
        ],
        holes: &[],
    },
];
