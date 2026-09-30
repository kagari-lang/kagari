// Observed at 199e509 before native array interval migration.
pub(super) struct Baseline {
    pub name: &'static str,
    pub steps: u64,
    pub effects: &'static [(&'static str, u64)],
    pub holes: &'static [(u64, u64)],
}
mod copy_heap;
mod copy_scalar;
mod remove_heap;
mod remove_scalar;
pub(super) fn all() -> impl Iterator<Item = &'static Baseline> {
    [
        copy_scalar::BASELINE,
        copy_heap::BASELINE,
        remove_scalar::BASELINE,
        remove_heap::BASELINE,
    ]
    .into_iter()
    .flatten()
}
