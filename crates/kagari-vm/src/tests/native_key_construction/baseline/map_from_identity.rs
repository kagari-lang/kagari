use super::Baseline;
pub(super) const BASELINE: &[Baseline] = &[
    Baseline {
        name: "map_from_identity_true_native",
        steps: 49,
        effects: &[("input", 17), ("constructed", 37), ("done", 47)],
        holes: &[],
    },
    Baseline {
        name: "map_from_identity_true_proxy",
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
        name: "map_from_identity_true_generic",
        steps: 52,
        effects: &[("input", 17), ("constructed", 40), ("done", 50)],
        holes: &[],
    },
    Baseline {
        name: "map_from_identity_true_dynamic",
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
        name: "map_from_identity_false_native",
        steps: 203,
        effects: &[("input", 37), ("constructed", 97), ("done", 201)],
        holes: &[],
    },
    Baseline {
        name: "map_from_identity_false_proxy",
        steps: 273,
        effects: &[
            ("input", 38),
            ("iter", 43),
            ("next", 68),
            ("next", 91),
            ("next", 114),
            ("next", 137),
            ("constructed", 167),
            ("done", 271),
        ],
        holes: &[],
    },
    Baseline {
        name: "map_from_identity_false_generic",
        steps: 206,
        effects: &[("input", 37), ("constructed", 100), ("done", 204)],
        holes: &[],
    },
    Baseline {
        name: "map_from_identity_false_dynamic",
        steps: 273,
        effects: &[
            ("input", 38),
            ("iter", 43),
            ("next", 68),
            ("next", 91),
            ("next", 114),
            ("next", 137),
            ("constructed", 167),
            ("done", 271),
        ],
        holes: &[],
    },
];
