use super::Baseline;
pub(super) const BASELINE: &[Baseline] = &[
    Baseline {
        name: "dedup_option_0_direct",
        steps: 32,
        effects: &[("receiver", 4), ("committed", 22), ("done", 30)],
        holes: &[],
    },
    Baseline {
        name: "dedup_option_0_generic",
        steps: 36,
        effects: &[("receiver", 4), ("committed", 26), ("done", 34)],
        holes: &[],
    },
    Baseline {
        name: "dedup_option_1_direct",
        steps: 55,
        effects: &[("receiver", 9), ("committed", 45), ("done", 53)],
        holes: &[(42, 41)],
    },
    Baseline {
        name: "dedup_option_1_generic",
        steps: 59,
        effects: &[("receiver", 9), ("committed", 49), ("done", 57)],
        holes: &[(44, 43)],
    },
    Baseline {
        name: "dedup_option_3_direct",
        steps: 166,
        effects: &[
            ("receiver", 19),
            ("eq", 65),
            ("eq", 114),
            ("committed", 156),
            ("done", 164),
        ],
        holes: &[(151, 150), (152, 150), (153, 150)],
    },
    Baseline {
        name: "dedup_option_3_generic",
        steps: 170,
        effects: &[
            ("receiver", 19),
            ("eq", 67),
            ("eq", 116),
            ("committed", 160),
            ("done", 168),
        ],
        holes: &[(153, 152), (154, 152), (155, 152)],
    },
];
