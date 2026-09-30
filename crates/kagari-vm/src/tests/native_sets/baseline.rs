// Observed at a6a3413 before migrating Set defaults.
pub(super) struct Baseline {
    pub name: &'static str,
    pub steps: u64,
    pub effects: &'static [(&'static str, u64)],
    pub holes: &'static [(u64, u64)],
}
mod difference_nominal;
mod difference_scalar;
mod intersection_nominal;
mod intersection_scalar;
mod is_disjoint_float;
mod is_disjoint_nominal;
mod is_disjoint_scalar;
mod is_subset_float;
mod is_subset_nominal;
mod is_subset_scalar;
mod is_superset_float;
mod is_superset_nominal;
mod is_superset_scalar;
mod symmetric_difference_nominal;
mod symmetric_difference_scalar;
mod union_nominal;
mod union_scalar;
pub(super) fn all() -> impl Iterator<Item = &'static Baseline> {
    [
        union_scalar::BASELINE,
        union_nominal::BASELINE,
        intersection_scalar::BASELINE,
        intersection_nominal::BASELINE,
        difference_scalar::BASELINE,
        difference_nominal::BASELINE,
        symmetric_difference_scalar::BASELINE,
        symmetric_difference_nominal::BASELINE,
        is_subset_scalar::BASELINE,
        is_subset_nominal::BASELINE,
        is_subset_float::BASELINE,
        is_superset_scalar::BASELINE,
        is_superset_nominal::BASELINE,
        is_superset_float::BASELINE,
        is_disjoint_scalar::BASELINE,
        is_disjoint_nominal::BASELINE,
        is_disjoint_float::BASELINE,
    ]
    .into_iter()
    .flatten()
}
