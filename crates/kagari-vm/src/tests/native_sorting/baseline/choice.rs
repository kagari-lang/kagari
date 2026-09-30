use super::Baseline;
pub(super) const BASELINE: &[Baseline] = &[
    Baseline {
        name: "dedup_choice_0_direct",
        steps: 32,
        effects: &[("receiver", 4), ("committed", 22), ("done", 30)],
        holes: &[],
    },
    Baseline {
        name: "dedup_choice_0_generic",
        steps: 36,
        effects: &[("receiver", 4), ("committed", 26), ("done", 34)],
        holes: &[],
    },
    Baseline {
        name: "dedup_choice_1_direct",
        steps: 55,
        effects: &[("receiver", 9), ("committed", 45), ("done", 53)],
        holes: &[(42, 41)],
    },
    Baseline {
        name: "dedup_choice_1_generic",
        steps: 59,
        effects: &[("receiver", 9), ("committed", 49), ("done", 57)],
        holes: &[(44, 43)],
    },
    Baseline {
        name: "dedup_choice_3_direct",
        steps: 170,
        effects: &[
            ("receiver", 19),
            ("eq", 67),
            ("eq", 118),
            ("committed", 160),
            ("done", 168),
        ],
        holes: &[(155, 154), (156, 154), (157, 154)],
    },
    Baseline {
        name: "dedup_choice_3_generic",
        steps: 174,
        effects: &[
            ("receiver", 19),
            ("eq", 69),
            ("eq", 120),
            ("committed", 164),
            ("done", 172),
        ],
        holes: &[(157, 156), (158, 156), (159, 156)],
    },
];
