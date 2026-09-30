// Observed at 3ee6486 before native grouping.
pub(super) struct Baseline {
    pub name: &'static str,
    pub steps: u64,
    pub effects: &'static [(&'static str, u64)],
    pub holes: &'static [(u64, u64)],
}
mod identity_repeated;
mod identity_sparse;
mod interface_repeated;
mod interface_sparse;
mod nominal_repeated;
mod nominal_sparse;
mod option_repeated;
mod option_sparse;
mod scalar_repeated;
mod scalar_sparse;
mod tuple_repeated;
mod tuple_sparse;
pub(super) fn all() -> impl Iterator<Item = &'static Baseline> {
    [
        scalar_sparse::BASELINE,
        scalar_repeated::BASELINE,
        nominal_sparse::BASELINE,
        nominal_repeated::BASELINE,
        tuple_sparse::BASELINE,
        tuple_repeated::BASELINE,
        option_sparse::BASELINE,
        option_repeated::BASELINE,
        identity_sparse::BASELINE,
        identity_repeated::BASELINE,
        interface_sparse::BASELINE,
        interface_repeated::BASELINE,
    ]
    .into_iter()
    .flatten()
}
