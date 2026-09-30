pub(super) struct Baseline {
    pub name: &'static str,
    pub steps: u64,
    pub effects: &'static [(&'static str, u64)],
}
// User destinations and iterator flavors were observed at 9825550. Scalar
// cases use the unchanged direct numeric trait lowering as the same schedule.
pub(super) const BASELINE: &[Baseline] = &[
    Baseline {
        name: "sum_native_false_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "product_native_false_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "sum_native_false_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "product_native_false_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "sum_native_false_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "product_native_false_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "sum_native_true_0",
        steps: 38,
        effects: &[("sum", 9), ("done", 36)],
    },
    Baseline {
        name: "product_native_true_0",
        steps: 38,
        effects: &[("product", 9), ("done", 36)],
    },
    Baseline {
        name: "sum_native_true_1",
        steps: 52,
        effects: &[("sum", 10), ("combine", 25), ("done", 50)],
    },
    Baseline {
        name: "product_native_true_1",
        steps: 52,
        effects: &[("product", 10), ("combine", 25), ("done", 50)],
    },
    Baseline {
        name: "sum_native_true_3",
        steps: 80,
        effects: &[
            ("sum", 12),
            ("combine", 27),
            ("combine", 40),
            ("combine", 53),
            ("done", 78),
        ],
    },
    Baseline {
        name: "product_native_true_3",
        steps: 80,
        effects: &[
            ("product", 12),
            ("combine", 27),
            ("combine", 40),
            ("combine", 53),
            ("done", 78),
        ],
    },
    Baseline {
        name: "sum_custom_false_0",
        steps: 36,
        effects: &[("next", 12), ("done", 34)],
    },
    Baseline {
        name: "product_custom_false_0",
        steps: 36,
        effects: &[("next", 12), ("done", 34)],
    },
    Baseline {
        name: "sum_custom_false_1",
        steps: 69,
        effects: &[("next", 13), ("next", 45), ("done", 67)],
    },
    Baseline {
        name: "product_custom_false_1",
        steps: 69,
        effects: &[("next", 13), ("next", 45), ("done", 67)],
    },
    Baseline {
        name: "sum_custom_false_3",
        steps: 135,
        effects: &[
            ("next", 15),
            ("next", 47),
            ("next", 79),
            ("next", 111),
            ("done", 133),
        ],
    },
    Baseline {
        name: "product_custom_false_3",
        steps: 135,
        effects: &[
            ("next", 15),
            ("next", 47),
            ("next", 79),
            ("next", 111),
            ("done", 133),
        ],
    },
    Baseline {
        name: "sum_custom_true_0",
        steps: 47,
        effects: &[("sum", 10), ("next", 18), ("done", 45)],
    },
    Baseline {
        name: "product_custom_true_0",
        steps: 47,
        effects: &[("product", 10), ("next", 18), ("done", 45)],
    },
    Baseline {
        name: "sum_custom_true_1",
        steps: 86,
        effects: &[
            ("sum", 11),
            ("next", 19),
            ("combine", 48),
            ("next", 57),
            ("done", 84),
        ],
    },
    Baseline {
        name: "product_custom_true_1",
        steps: 86,
        effects: &[
            ("product", 11),
            ("next", 19),
            ("combine", 48),
            ("next", 57),
            ("done", 84),
        ],
    },
    Baseline {
        name: "sum_custom_true_3",
        steps: 164,
        effects: &[
            ("sum", 13),
            ("next", 21),
            ("combine", 50),
            ("next", 59),
            ("combine", 88),
            ("next", 97),
            ("combine", 126),
            ("next", 135),
            ("done", 162),
        ],
    },
    Baseline {
        name: "product_custom_true_3",
        steps: 164,
        effects: &[
            ("product", 13),
            ("next", 21),
            ("combine", 50),
            ("next", 59),
            ("combine", 88),
            ("next", 97),
            ("combine", 126),
            ("next", 135),
            ("done", 162),
        ],
    },
    Baseline {
        name: "sum_lazy_false_0",
        steps: 41,
        effects: &[("done", 39)],
    },
    Baseline {
        name: "product_lazy_false_0",
        steps: 41,
        effects: &[("done", 39)],
    },
    Baseline {
        name: "sum_lazy_false_1",
        steps: 63,
        effects: &[("item", 28), ("done", 61)],
    },
    Baseline {
        name: "product_lazy_false_1",
        steps: 63,
        effects: &[("item", 28), ("done", 61)],
    },
    Baseline {
        name: "sum_lazy_false_3",
        steps: 107,
        effects: &[("item", 30), ("item", 51), ("item", 72), ("done", 105)],
    },
    Baseline {
        name: "product_lazy_false_3",
        steps: 107,
        effects: &[("item", 30), ("item", 51), ("item", 72), ("done", 105)],
    },
    Baseline {
        name: "sum_lazy_true_0",
        steps: 52,
        effects: &[("sum", 13), ("done", 50)],
    },
    Baseline {
        name: "product_lazy_true_0",
        steps: 52,
        effects: &[("product", 13), ("done", 50)],
    },
    Baseline {
        name: "sum_lazy_true_1",
        steps: 80,
        effects: &[("sum", 14), ("item", 34), ("combine", 44), ("done", 78)],
    },
    Baseline {
        name: "product_lazy_true_1",
        steps: 80,
        effects: &[("product", 14), ("item", 34), ("combine", 44), ("done", 78)],
    },
    Baseline {
        name: "sum_lazy_true_3",
        steps: 136,
        effects: &[
            ("sum", 16),
            ("item", 36),
            ("combine", 46),
            ("item", 63),
            ("combine", 73),
            ("item", 90),
            ("combine", 100),
            ("done", 134),
        ],
    },
    Baseline {
        name: "product_lazy_true_3",
        steps: 136,
        effects: &[
            ("product", 16),
            ("item", 36),
            ("combine", 46),
            ("item", 63),
            ("combine", 73),
            ("item", 90),
            ("combine", 100),
            ("done", 134),
        ],
    },
    Baseline {
        name: "sum_i8_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "product_i8_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "sum_i8_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "product_i8_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "sum_i8_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "product_i8_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "sum_i16_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "product_i16_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "sum_i16_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "product_i16_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "sum_i16_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "product_i16_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "sum_i32_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "product_i32_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "sum_i32_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "product_i32_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "sum_i32_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "product_i32_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "sum_i64_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "product_i64_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "sum_i64_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "product_i64_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "sum_i64_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "product_i64_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "sum_isize_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "product_isize_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "sum_isize_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "product_isize_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "sum_isize_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "product_isize_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "sum_u8_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "product_u8_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "sum_u8_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "product_u8_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "sum_u8_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "product_u8_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "sum_u16_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "product_u16_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "sum_u16_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "product_u16_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "sum_u16_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "product_u16_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "sum_u32_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "product_u32_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "sum_u32_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "product_u32_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "sum_u32_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "product_u32_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "sum_u64_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "product_u64_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "sum_u64_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "product_u64_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "sum_u64_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "product_u64_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "sum_usize_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "product_usize_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "sum_usize_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "product_usize_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "sum_usize_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "product_usize_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "sum_f32_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "product_f32_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "sum_f32_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "product_f32_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "sum_f32_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "product_f32_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "sum_f64_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "product_f64_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "sum_f64_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "product_f64_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "sum_f64_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "product_f64_3",
        steps: 51,
        effects: &[("done", 49)],
    },
];
