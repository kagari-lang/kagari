use super::Baseline;
pub(super) const BASELINE: &[Baseline] = &[
    Baseline {
        name: "scalar_direct_keys_0",
        steps: 48,
        effects: &[("snapshot", 28), ("done", 46)],
    },
    Baseline {
        name: "scalar_direct_keys_1",
        steps: 82,
        effects: &[("snapshot", 41), ("done", 80)],
    },
    Baseline {
        name: "scalar_direct_keys_3",
        steps: 150,
        effects: &[("snapshot", 67), ("done", 148)],
    },
    Baseline {
        name: "scalar_custom_keys_0",
        steps: 64,
        effects: &[("iter", 12), ("snapshot", 44), ("done", 62)],
    },
    Baseline {
        name: "scalar_custom_keys_1",
        steps: 109,
        effects: &[("iter", 15), ("next", 39), ("snapshot", 68), ("done", 107)],
    },
    Baseline {
        name: "scalar_custom_keys_3",
        steps: 199,
        effects: &[
            ("iter", 21),
            ("next", 45),
            ("next", 66),
            ("next", 87),
            ("snapshot", 116),
            ("done", 197),
        ],
    },
    Baseline {
        name: "scalar_dynamic_keys_0",
        steps: 62,
        effects: &[("iter", 11), ("snapshot", 42), ("done", 60)],
    },
    Baseline {
        name: "scalar_dynamic_keys_1",
        steps: 107,
        effects: &[("iter", 14), ("next", 38), ("snapshot", 66), ("done", 105)],
    },
    Baseline {
        name: "scalar_dynamic_keys_3",
        steps: 197,
        effects: &[
            ("iter", 20),
            ("next", 44),
            ("next", 65),
            ("next", 86),
            ("snapshot", 114),
            ("done", 195),
        ],
    },
    Baseline {
        name: "scalar_native_keys_0",
        steps: 62,
        effects: &[("snapshot", 42), ("done", 60)],
    },
    Baseline {
        name: "scalar_native_keys_1",
        steps: 104,
        effects: &[("snapshot", 63), ("done", 102)],
    },
    Baseline {
        name: "scalar_native_keys_3",
        steps: 188,
        effects: &[("snapshot", 105), ("done", 186)],
    },
    Baseline {
        name: "scalar_static_keys_0",
        steps: 61,
        effects: &[("snapshot", 41), ("done", 59)],
    },
    Baseline {
        name: "scalar_static_keys_1",
        steps: 103,
        effects: &[("snapshot", 62), ("done", 101)],
    },
    Baseline {
        name: "scalar_static_keys_3",
        steps: 187,
        effects: &[("snapshot", 104), ("done", 185)],
    },
    Baseline {
        name: "heap_direct_keys_0",
        steps: 48,
        effects: &[("snapshot", 28), ("done", 46)],
    },
    Baseline {
        name: "heap_direct_keys_1",
        steps: 100,
        effects: &[("hash", 30), ("snapshot", 58), ("done", 98)],
    },
    Baseline {
        name: "heap_direct_keys_3",
        steps: 204,
        effects: &[
            ("hash", 40),
            ("hash", 65),
            ("hash", 90),
            ("snapshot", 118),
            ("done", 202),
        ],
    },
    Baseline {
        name: "heap_custom_keys_0",
        steps: 64,
        effects: &[("iter", 12), ("snapshot", 44), ("done", 62)],
    },
    Baseline {
        name: "heap_custom_keys_1",
        steps: 112,
        effects: &[("iter", 17), ("next", 41), ("snapshot", 70), ("done", 110)],
    },
    Baseline {
        name: "heap_custom_keys_3",
        steps: 208,
        effects: &[
            ("iter", 27),
            ("next", 51),
            ("next", 72),
            ("next", 93),
            ("snapshot", 122),
            ("done", 206),
        ],
    },
    Baseline {
        name: "heap_dynamic_keys_0",
        steps: 62,
        effects: &[("iter", 11), ("snapshot", 42), ("done", 60)],
    },
    Baseline {
        name: "heap_dynamic_keys_1",
        steps: 110,
        effects: &[("iter", 16), ("next", 40), ("snapshot", 68), ("done", 108)],
    },
    Baseline {
        name: "heap_dynamic_keys_3",
        steps: 206,
        effects: &[
            ("iter", 26),
            ("next", 50),
            ("next", 71),
            ("next", 92),
            ("snapshot", 120),
            ("done", 204),
        ],
    },
    Baseline {
        name: "heap_native_keys_0",
        steps: 62,
        effects: &[("snapshot", 42), ("done", 60)],
    },
    Baseline {
        name: "heap_native_keys_1",
        steps: 122,
        effects: &[("hash", 30), ("snapshot", 80), ("done", 120)],
    },
    Baseline {
        name: "heap_native_keys_3",
        steps: 242,
        effects: &[
            ("hash", 40),
            ("hash", 65),
            ("hash", 90),
            ("snapshot", 156),
            ("done", 240),
        ],
    },
    Baseline {
        name: "heap_static_keys_0",
        steps: 61,
        effects: &[("snapshot", 41), ("done", 59)],
    },
    Baseline {
        name: "heap_static_keys_1",
        steps: 121,
        effects: &[("hash", 30), ("snapshot", 79), ("done", 119)],
    },
    Baseline {
        name: "heap_static_keys_3",
        steps: 241,
        effects: &[
            ("hash", 40),
            ("hash", 65),
            ("hash", 90),
            ("snapshot", 155),
            ("done", 239),
        ],
    },
    Baseline {
        name: "float_custom_keys_0",
        steps: 64,
        effects: &[("iter", 12), ("snapshot", 44), ("done", 62)],
    },
    Baseline {
        name: "float_custom_keys_1",
        steps: 110,
        effects: &[("iter", 15), ("next", 39), ("snapshot", 68), ("done", 108)],
    },
    Baseline {
        name: "float_custom_keys_3",
        steps: 202,
        effects: &[
            ("iter", 21),
            ("next", 45),
            ("next", 66),
            ("next", 87),
            ("snapshot", 116),
            ("done", 200),
        ],
    },
    Baseline {
        name: "float_dynamic_keys_0",
        steps: 62,
        effects: &[("iter", 11), ("snapshot", 42), ("done", 60)],
    },
    Baseline {
        name: "float_dynamic_keys_1",
        steps: 108,
        effects: &[("iter", 14), ("next", 38), ("snapshot", 66), ("done", 106)],
    },
    Baseline {
        name: "float_dynamic_keys_3",
        steps: 200,
        effects: &[
            ("iter", 20),
            ("next", 44),
            ("next", 65),
            ("next", 86),
            ("snapshot", 114),
            ("done", 198),
        ],
    },
];
