use super::Baseline;
pub(super) const BASELINE: &[Baseline] = &[
    Baseline {
        name: "dedup_tuple_0_direct",
        steps: 32,
        effects: &[("receiver", 4), ("committed", 22), ("done", 30)],
        holes: &[],
    },
    Baseline {
        name: "dedup_tuple_0_generic",
        steps: 36,
        effects: &[("receiver", 4), ("committed", 26), ("done", 34)],
        holes: &[],
    },
    Baseline {
        name: "dedup_tuple_1_direct",
        steps: 56,
        effects: &[("receiver", 10), ("committed", 46), ("done", 54)],
        holes: &[(43, 42)],
    },
    Baseline {
        name: "dedup_tuple_1_generic",
        steps: 60,
        effects: &[("receiver", 10), ("committed", 50), ("done", 58)],
        holes: &[(45, 44)],
    },
    Baseline {
        name: "dedup_tuple_3_direct",
        steps: 167,
        effects: &[
            ("receiver", 22),
            ("eq", 68),
            ("eq", 117),
            ("committed", 157),
            ("done", 165),
        ],
        holes: &[(152, 151), (153, 151), (154, 151)],
    },
    Baseline {
        name: "dedup_tuple_3_generic",
        steps: 171,
        effects: &[
            ("receiver", 22),
            ("eq", 70),
            ("eq", 119),
            ("committed", 161),
            ("done", 169),
        ],
        holes: &[(154, 153), (155, 153), (156, 153)],
    },
];
