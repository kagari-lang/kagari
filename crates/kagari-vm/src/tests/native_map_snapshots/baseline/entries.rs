use super::Baseline;
pub(super) const BASELINE: &[Baseline] = &[
    Baseline {
        name: "scalar_direct_entries_0",
        steps: 48,
        effects: &[("snapshot", 28), ("done", 46)],
    },
    Baseline {
        name: "scalar_direct_entries_1",
        steps: 98,
        effects: &[("snapshot", 41), ("done", 96)],
    },
    Baseline {
        name: "scalar_direct_entries_3",
        steps: 198,
        effects: &[("snapshot", 67), ("done", 196)],
    },
    Baseline {
        name: "scalar_custom_entries_0",
        steps: 64,
        effects: &[("iter", 12), ("snapshot", 44), ("done", 62)],
    },
    Baseline {
        name: "scalar_custom_entries_1",
        steps: 123,
        effects: &[("iter", 15), ("next", 39), ("snapshot", 66), ("done", 121)],
    },
    Baseline {
        name: "scalar_custom_entries_3",
        steps: 241,
        effects: &[
            ("iter", 21),
            ("next", 45),
            ("next", 64),
            ("next", 83),
            ("snapshot", 110),
            ("done", 239),
        ],
    },
    Baseline {
        name: "scalar_dynamic_entries_0",
        steps: 62,
        effects: &[("iter", 11), ("snapshot", 42), ("done", 60)],
    },
    Baseline {
        name: "scalar_dynamic_entries_1",
        steps: 121,
        effects: &[("iter", 14), ("next", 38), ("snapshot", 64), ("done", 119)],
    },
    Baseline {
        name: "scalar_dynamic_entries_3",
        steps: 239,
        effects: &[
            ("iter", 20),
            ("next", 44),
            ("next", 63),
            ("next", 82),
            ("snapshot", 108),
            ("done", 237),
        ],
    },
    Baseline {
        name: "scalar_native_entries_0",
        steps: 62,
        effects: &[("snapshot", 42), ("done", 60)],
    },
    Baseline {
        name: "scalar_native_entries_1",
        steps: 118,
        effects: &[("snapshot", 61), ("done", 116)],
    },
    Baseline {
        name: "scalar_native_entries_3",
        steps: 230,
        effects: &[("snapshot", 99), ("done", 228)],
    },
    Baseline {
        name: "scalar_static_entries_0",
        steps: 61,
        effects: &[("snapshot", 41), ("done", 59)],
    },
    Baseline {
        name: "scalar_static_entries_1",
        steps: 117,
        effects: &[("snapshot", 60), ("done", 115)],
    },
    Baseline {
        name: "scalar_static_entries_3",
        steps: 229,
        effects: &[("snapshot", 98), ("done", 227)],
    },
    Baseline {
        name: "heap_direct_entries_0",
        steps: 48,
        effects: &[("snapshot", 28), ("done", 46)],
    },
    Baseline {
        name: "heap_direct_entries_1",
        steps: 118,
        effects: &[("hash", 30), ("snapshot", 58), ("done", 116)],
    },
    Baseline {
        name: "heap_direct_entries_3",
        steps: 258,
        effects: &[
            ("hash", 40),
            ("hash", 65),
            ("hash", 90),
            ("snapshot", 118),
            ("done", 256),
        ],
    },
    Baseline {
        name: "heap_custom_entries_0",
        steps: 64,
        effects: &[("iter", 12), ("snapshot", 44), ("done", 62)],
    },
    Baseline {
        name: "heap_custom_entries_1",
        steps: 128,
        effects: &[("iter", 17), ("next", 41), ("snapshot", 68), ("done", 126)],
    },
    Baseline {
        name: "heap_custom_entries_3",
        steps: 256,
        effects: &[
            ("iter", 27),
            ("next", 51),
            ("next", 70),
            ("next", 89),
            ("snapshot", 116),
            ("done", 254),
        ],
    },
    Baseline {
        name: "heap_dynamic_entries_0",
        steps: 62,
        effects: &[("iter", 11), ("snapshot", 42), ("done", 60)],
    },
    Baseline {
        name: "heap_dynamic_entries_1",
        steps: 126,
        effects: &[("iter", 16), ("next", 40), ("snapshot", 66), ("done", 124)],
    },
    Baseline {
        name: "heap_dynamic_entries_3",
        steps: 254,
        effects: &[
            ("iter", 26),
            ("next", 50),
            ("next", 69),
            ("next", 88),
            ("snapshot", 114),
            ("done", 252),
        ],
    },
    Baseline {
        name: "heap_native_entries_0",
        steps: 62,
        effects: &[("snapshot", 42), ("done", 60)],
    },
    Baseline {
        name: "heap_native_entries_1",
        steps: 138,
        effects: &[("hash", 30), ("snapshot", 78), ("done", 136)],
    },
    Baseline {
        name: "heap_native_entries_3",
        steps: 290,
        effects: &[
            ("hash", 40),
            ("hash", 65),
            ("hash", 90),
            ("snapshot", 150),
            ("done", 288),
        ],
    },
    Baseline {
        name: "heap_static_entries_0",
        steps: 61,
        effects: &[("snapshot", 41), ("done", 59)],
    },
    Baseline {
        name: "heap_static_entries_1",
        steps: 137,
        effects: &[("hash", 30), ("snapshot", 77), ("done", 135)],
    },
    Baseline {
        name: "heap_static_entries_3",
        steps: 289,
        effects: &[
            ("hash", 40),
            ("hash", 65),
            ("hash", 90),
            ("snapshot", 149),
            ("done", 287),
        ],
    },
    Baseline {
        name: "float_custom_entries_0",
        steps: 64,
        effects: &[("iter", 12), ("snapshot", 44), ("done", 62)],
    },
    Baseline {
        name: "float_custom_entries_1",
        steps: 124,
        effects: &[("iter", 15), ("next", 39), ("snapshot", 66), ("done", 122)],
    },
    Baseline {
        name: "float_custom_entries_3",
        steps: 244,
        effects: &[
            ("iter", 21),
            ("next", 45),
            ("next", 64),
            ("next", 83),
            ("snapshot", 110),
            ("done", 242),
        ],
    },
    Baseline {
        name: "float_dynamic_entries_0",
        steps: 62,
        effects: &[("iter", 11), ("snapshot", 42), ("done", 60)],
    },
    Baseline {
        name: "float_dynamic_entries_1",
        steps: 122,
        effects: &[("iter", 14), ("next", 38), ("snapshot", 64), ("done", 120)],
    },
    Baseline {
        name: "float_dynamic_entries_3",
        steps: 242,
        effects: &[
            ("iter", 20),
            ("next", 44),
            ("next", 63),
            ("next", 82),
            ("snapshot", 108),
            ("done", 240),
        ],
    },
];
