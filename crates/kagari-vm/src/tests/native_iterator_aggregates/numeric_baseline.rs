// Observed direct numeric trait execution at 136ec59 before native replacement.
use super::baseline::Baseline;
pub(super) const BASELINE: &[Baseline] = &[
    Baseline {
        name: "direct_sum_i8_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_product_i8_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_sum_i8_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "direct_product_i8_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "direct_sum_i8_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "direct_product_i8_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "direct_sum_i16_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_product_i16_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_sum_i16_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "direct_product_i16_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "direct_sum_i16_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "direct_product_i16_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "direct_sum_i32_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_product_i32_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_sum_i32_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "direct_product_i32_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "direct_sum_i32_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "direct_product_i32_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "direct_sum_i64_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_product_i64_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_sum_i64_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "direct_product_i64_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "direct_sum_i64_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "direct_product_i64_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "direct_sum_isize_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_product_isize_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_sum_isize_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "direct_product_isize_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "direct_sum_isize_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "direct_product_isize_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "direct_sum_u8_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_product_u8_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_sum_u8_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "direct_product_u8_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "direct_sum_u8_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "direct_product_u8_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "direct_sum_u16_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_product_u16_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_sum_u16_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "direct_product_u16_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "direct_sum_u16_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "direct_product_u16_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "direct_sum_u32_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_product_u32_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_sum_u32_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "direct_product_u32_1",
        steps: 42,
        effects: &[("done", 40)],
    },
    Baseline {
        name: "direct_sum_u32_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "direct_product_u32_3",
        steps: 72,
        effects: &[("done", 70)],
    },
    Baseline {
        name: "direct_sum_u64_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_product_u64_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_sum_u64_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "direct_product_u64_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "direct_sum_u64_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "direct_product_u64_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "direct_sum_usize_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_product_usize_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_sum_usize_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "direct_product_usize_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "direct_sum_usize_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "direct_product_usize_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "direct_sum_f32_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_product_f32_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_sum_f32_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "direct_product_f32_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "direct_sum_f32_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "direct_product_f32_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "direct_sum_f64_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_product_f64_0",
        steps: 27,
        effects: &[("done", 25)],
    },
    Baseline {
        name: "direct_sum_f64_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "direct_product_f64_1",
        steps: 35,
        effects: &[("done", 33)],
    },
    Baseline {
        name: "direct_sum_f64_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "direct_product_f64_3",
        steps: 51,
        effects: &[("done", 49)],
    },
    Baseline {
        name: "direct_sum_iterator_0",
        steps: 29,
        effects: &[("done", 27)],
    },
    Baseline {
        name: "direct_product_iterator_0",
        steps: 29,
        effects: &[("done", 27)],
    },
    Baseline {
        name: "direct_sum_iterator_1",
        steps: 37,
        effects: &[("done", 35)],
    },
    Baseline {
        name: "direct_product_iterator_1",
        steps: 37,
        effects: &[("done", 35)],
    },
    Baseline {
        name: "direct_sum_iterator_3",
        steps: 53,
        effects: &[("done", 51)],
    },
    Baseline {
        name: "direct_product_iterator_3",
        steps: 53,
        effects: &[("done", 51)],
    },
    Baseline {
        name: "direct_sum_custom_iterator_0",
        steps: 38,
        effects: &[("next", 14), ("done", 36)],
    },
    Baseline {
        name: "direct_product_custom_iterator_0",
        steps: 38,
        effects: &[("next", 14), ("done", 36)],
    },
    Baseline {
        name: "direct_sum_custom_iterator_1",
        steps: 71,
        effects: &[("next", 15), ("next", 47), ("done", 69)],
    },
    Baseline {
        name: "direct_product_custom_iterator_1",
        steps: 71,
        effects: &[("next", 15), ("next", 47), ("done", 69)],
    },
    Baseline {
        name: "direct_sum_custom_iterator_3",
        steps: 137,
        effects: &[
            ("next", 17),
            ("next", 49),
            ("next", 81),
            ("next", 113),
            ("done", 135),
        ],
    },
    Baseline {
        name: "direct_product_custom_iterator_3",
        steps: 137,
        effects: &[
            ("next", 17),
            ("next", 49),
            ("next", 81),
            ("next", 113),
            ("done", 135),
        ],
    },
    Baseline {
        name: "direct_sum_lazy_0",
        steps: 43,
        effects: &[("done", 41)],
    },
    Baseline {
        name: "direct_product_lazy_0",
        steps: 43,
        effects: &[("done", 41)],
    },
    Baseline {
        name: "direct_sum_lazy_1",
        steps: 65,
        effects: &[("item", 30), ("done", 63)],
    },
    Baseline {
        name: "direct_product_lazy_1",
        steps: 65,
        effects: &[("item", 30), ("done", 63)],
    },
    Baseline {
        name: "direct_sum_lazy_3",
        steps: 109,
        effects: &[("item", 32), ("item", 53), ("item", 74), ("done", 107)],
    },
    Baseline {
        name: "direct_product_lazy_3",
        steps: 109,
        effects: &[("item", 32), ("item", 53), ("item", 74), ("done", 107)],
    },
    Baseline {
        name: "direct_sum_custom_iterable_0",
        steps: 45,
        effects: &[("iter", 12), ("next", 21), ("done", 43)],
    },
    Baseline {
        name: "direct_product_custom_iterable_0",
        steps: 45,
        effects: &[("iter", 12), ("next", 21), ("done", 43)],
    },
    Baseline {
        name: "direct_sum_custom_iterable_1",
        steps: 78,
        effects: &[("iter", 13), ("next", 22), ("next", 54), ("done", 76)],
    },
    Baseline {
        name: "direct_product_custom_iterable_1",
        steps: 78,
        effects: &[("iter", 13), ("next", 22), ("next", 54), ("done", 76)],
    },
    Baseline {
        name: "direct_sum_custom_iterable_3",
        steps: 144,
        effects: &[
            ("iter", 15),
            ("next", 24),
            ("next", 56),
            ("next", 88),
            ("next", 120),
            ("done", 142),
        ],
    },
    Baseline {
        name: "direct_product_custom_iterable_3",
        steps: 144,
        effects: &[
            ("iter", 15),
            ("next", 24),
            ("next", 56),
            ("next", 88),
            ("next", 120),
            ("done", 142),
        ],
    },
    Baseline {
        name: "direct_sum_native_iterable_0",
        steps: 36,
        effects: &[("iter", 12), ("done", 34)],
    },
    Baseline {
        name: "direct_product_native_iterable_0",
        steps: 36,
        effects: &[("iter", 12), ("done", 34)],
    },
    Baseline {
        name: "direct_sum_native_iterable_1",
        steps: 44,
        effects: &[("iter", 13), ("done", 42)],
    },
    Baseline {
        name: "direct_product_native_iterable_1",
        steps: 44,
        effects: &[("iter", 13), ("done", 42)],
    },
    Baseline {
        name: "direct_sum_native_iterable_3",
        steps: 60,
        effects: &[("iter", 15), ("done", 58)],
    },
    Baseline {
        name: "direct_product_native_iterable_3",
        steps: 60,
        effects: &[("iter", 15), ("done", 58)],
    },
    Baseline {
        name: "direct_sum_dynamic_iterable_0",
        steps: 46,
        effects: &[("iter", 13), ("next", 22), ("done", 44)],
    },
    Baseline {
        name: "direct_product_dynamic_iterable_0",
        steps: 46,
        effects: &[("iter", 13), ("next", 22), ("done", 44)],
    },
    Baseline {
        name: "direct_sum_dynamic_iterable_1",
        steps: 79,
        effects: &[("iter", 14), ("next", 23), ("next", 55), ("done", 77)],
    },
    Baseline {
        name: "direct_product_dynamic_iterable_1",
        steps: 79,
        effects: &[("iter", 14), ("next", 23), ("next", 55), ("done", 77)],
    },
    Baseline {
        name: "direct_sum_dynamic_iterable_3",
        steps: 145,
        effects: &[
            ("iter", 16),
            ("next", 25),
            ("next", 57),
            ("next", 89),
            ("next", 121),
            ("done", 143),
        ],
    },
    Baseline {
        name: "direct_product_dynamic_iterable_3",
        steps: 145,
        effects: &[
            ("iter", 16),
            ("next", 25),
            ("next", 57),
            ("next", 89),
            ("next", 121),
            ("done", 143),
        ],
    },
    Baseline {
        name: "direct_sum_dynamic_list_0",
        steps: 33,
        effects: &[("done", 31)],
    },
    Baseline {
        name: "direct_product_dynamic_list_0",
        steps: 33,
        effects: &[("done", 31)],
    },
    Baseline {
        name: "direct_sum_dynamic_list_1",
        steps: 41,
        effects: &[("done", 39)],
    },
    Baseline {
        name: "direct_product_dynamic_list_1",
        steps: 41,
        effects: &[("done", 39)],
    },
    Baseline {
        name: "direct_sum_dynamic_list_3",
        steps: 57,
        effects: &[("done", 55)],
    },
    Baseline {
        name: "direct_product_dynamic_list_3",
        steps: 57,
        effects: &[("done", 55)],
    },
    Baseline {
        name: "direct_sum_readonly_0",
        steps: 33,
        effects: &[("done", 31)],
    },
    Baseline {
        name: "direct_product_readonly_0",
        steps: 33,
        effects: &[("done", 31)],
    },
    Baseline {
        name: "direct_sum_readonly_1",
        steps: 41,
        effects: &[("done", 39)],
    },
    Baseline {
        name: "direct_product_readonly_1",
        steps: 41,
        effects: &[("done", 39)],
    },
    Baseline {
        name: "direct_sum_readonly_3",
        steps: 57,
        effects: &[("done", 55)],
    },
    Baseline {
        name: "direct_product_readonly_3",
        steps: 57,
        effects: &[("done", 55)],
    },
];
