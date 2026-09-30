// Observed at ee8fddc before replacing ArrayList::from_fn lowering.
pub(super) struct Baseline {
    pub name: &'static str,
    pub steps: u64,
    pub effects: &'static [(&'static str, u64)],
}
pub(super) const BASELINE: &[Baseline] = &[
    Baseline {
        name: "index_0",
        steps: 44,
        effects: &[],
    },
    Baseline {
        name: "object_0",
        steps: 44,
        effects: &[],
    },
    Baseline {
        name: "shared_0",
        steps: 45,
        effects: &[],
    },
    Baseline {
        name: "tuple_0",
        steps: 44,
        effects: &[],
    },
    Baseline {
        name: "named_0",
        steps: 36,
        effects: &[],
    },
    Baseline {
        name: "index_1",
        steps: 69,
        effects: &[],
    },
    Baseline {
        name: "object_1",
        steps: 71,
        effects: &[],
    },
    Baseline {
        name: "shared_1",
        steps: 71,
        effects: &[],
    },
    Baseline {
        name: "tuple_1",
        steps: 86,
        effects: &[],
    },
    Baseline {
        name: "named_1",
        steps: 64,
        effects: &[],
    },
    Baseline {
        name: "index_3",
        steps: 119,
        effects: &[],
    },
    Baseline {
        name: "object_3",
        steps: 125,
        effects: &[],
    },
    Baseline {
        name: "shared_3",
        steps: 123,
        effects: &[],
    },
    Baseline {
        name: "tuple_3",
        steps: 170,
        effects: &[],
    },
    Baseline {
        name: "named_3",
        steps: 120,
        effects: &[],
    },
    Baseline {
        name: "index_5",
        steps: 169,
        effects: &[],
    },
    Baseline {
        name: "object_5",
        steps: 179,
        effects: &[],
    },
    Baseline {
        name: "shared_5",
        steps: 175,
        effects: &[],
    },
    Baseline {
        name: "tuple_5",
        steps: 254,
        effects: &[],
    },
    Baseline {
        name: "named_5",
        steps: 176,
        effects: &[],
    },
];
