use super::Baseline;
pub(super) const BASELINE: &[Baseline] = &[
    Baseline {
        name: "scalar_direct_values_0",
        steps: 48,
        effects: &[("snapshot", 28), ("done", 46)],
    },
    Baseline {
        name: "scalar_direct_values_1",
        steps: 84,
        effects: &[("snapshot", 41), ("done", 82)],
    },
    Baseline {
        name: "scalar_direct_values_3",
        steps: 156,
        effects: &[("snapshot", 67), ("done", 154)],
    },
    Baseline {
        name: "scalar_custom_values_0",
        steps: 64,
        effects: &[("iter", 12), ("snapshot", 44), ("done", 62)],
    },
    Baseline {
        name: "scalar_custom_values_1",
        steps: 111,
        effects: &[("iter", 15), ("next", 39), ("snapshot", 68), ("done", 109)],
    },
    Baseline {
        name: "scalar_custom_values_3",
        steps: 205,
        effects: &[
            ("iter", 21),
            ("next", 45),
            ("next", 66),
            ("next", 87),
            ("snapshot", 116),
            ("done", 203),
        ],
    },
    Baseline {
        name: "scalar_dynamic_values_0",
        steps: 62,
        effects: &[("iter", 11), ("snapshot", 42), ("done", 60)],
    },
    Baseline {
        name: "scalar_dynamic_values_1",
        steps: 109,
        effects: &[("iter", 14), ("next", 38), ("snapshot", 66), ("done", 107)],
    },
    Baseline {
        name: "scalar_dynamic_values_3",
        steps: 203,
        effects: &[
            ("iter", 20),
            ("next", 44),
            ("next", 65),
            ("next", 86),
            ("snapshot", 114),
            ("done", 201),
        ],
    },
    Baseline {
        name: "scalar_native_values_0",
        steps: 62,
        effects: &[("snapshot", 42), ("done", 60)],
    },
    Baseline {
        name: "scalar_native_values_1",
        steps: 106,
        effects: &[("snapshot", 63), ("done", 104)],
    },
    Baseline {
        name: "scalar_native_values_3",
        steps: 194,
        effects: &[("snapshot", 105), ("done", 192)],
    },
    Baseline {
        name: "scalar_static_values_0",
        steps: 61,
        effects: &[("snapshot", 41), ("done", 59)],
    },
    Baseline {
        name: "scalar_static_values_1",
        steps: 105,
        effects: &[("snapshot", 62), ("done", 103)],
    },
    Baseline {
        name: "scalar_static_values_3",
        steps: 193,
        effects: &[("snapshot", 104), ("done", 191)],
    },
    Baseline {
        name: "heap_direct_values_0",
        steps: 48,
        effects: &[("snapshot", 28), ("done", 46)],
    },
    Baseline {
        name: "heap_direct_values_1",
        steps: 103,
        effects: &[("hash", 30), ("snapshot", 58), ("done", 101)],
    },
    Baseline {
        name: "heap_direct_values_3",
        steps: 213,
        effects: &[
            ("hash", 40),
            ("hash", 65),
            ("hash", 90),
            ("snapshot", 118),
            ("done", 211),
        ],
    },
    Baseline {
        name: "heap_custom_values_0",
        steps: 64,
        effects: &[("iter", 12), ("snapshot", 44), ("done", 62)],
    },
    Baseline {
        name: "heap_custom_values_1",
        steps: 115,
        effects: &[("iter", 17), ("next", 41), ("snapshot", 70), ("done", 113)],
    },
    Baseline {
        name: "heap_custom_values_3",
        steps: 217,
        effects: &[
            ("iter", 27),
            ("next", 51),
            ("next", 72),
            ("next", 93),
            ("snapshot", 122),
            ("done", 215),
        ],
    },
    Baseline {
        name: "heap_dynamic_values_0",
        steps: 62,
        effects: &[("iter", 11), ("snapshot", 42), ("done", 60)],
    },
    Baseline {
        name: "heap_dynamic_values_1",
        steps: 113,
        effects: &[("iter", 16), ("next", 40), ("snapshot", 68), ("done", 111)],
    },
    Baseline {
        name: "heap_dynamic_values_3",
        steps: 215,
        effects: &[
            ("iter", 26),
            ("next", 50),
            ("next", 71),
            ("next", 92),
            ("snapshot", 120),
            ("done", 213),
        ],
    },
    Baseline {
        name: "heap_native_values_0",
        steps: 62,
        effects: &[("snapshot", 42), ("done", 60)],
    },
    Baseline {
        name: "heap_native_values_1",
        steps: 125,
        effects: &[("hash", 30), ("snapshot", 80), ("done", 123)],
    },
    Baseline {
        name: "heap_native_values_3",
        steps: 251,
        effects: &[
            ("hash", 40),
            ("hash", 65),
            ("hash", 90),
            ("snapshot", 156),
            ("done", 249),
        ],
    },
    Baseline {
        name: "heap_static_values_0",
        steps: 61,
        effects: &[("snapshot", 41), ("done", 59)],
    },
    Baseline {
        name: "heap_static_values_1",
        steps: 124,
        effects: &[("hash", 30), ("snapshot", 79), ("done", 122)],
    },
    Baseline {
        name: "heap_static_values_3",
        steps: 250,
        effects: &[
            ("hash", 40),
            ("hash", 65),
            ("hash", 90),
            ("snapshot", 155),
            ("done", 248),
        ],
    },
    Baseline {
        name: "float_custom_values_0",
        steps: 64,
        effects: &[("iter", 12), ("snapshot", 44), ("done", 62)],
    },
    Baseline {
        name: "float_custom_values_1",
        steps: 111,
        effects: &[("iter", 15), ("next", 39), ("snapshot", 68), ("done", 109)],
    },
    Baseline {
        name: "float_custom_values_3",
        steps: 205,
        effects: &[
            ("iter", 21),
            ("next", 45),
            ("next", 66),
            ("next", 87),
            ("snapshot", 116),
            ("done", 203),
        ],
    },
    Baseline {
        name: "float_dynamic_values_0",
        steps: 62,
        effects: &[("iter", 11), ("snapshot", 42), ("done", 60)],
    },
    Baseline {
        name: "float_dynamic_values_1",
        steps: 109,
        effects: &[("iter", 14), ("next", 38), ("snapshot", 66), ("done", 107)],
    },
    Baseline {
        name: "float_dynamic_values_3",
        steps: 203,
        effects: &[
            ("iter", 20),
            ("next", 44),
            ("next", 65),
            ("next", 86),
            ("snapshot", 114),
            ("done", 201),
        ],
    },
];
