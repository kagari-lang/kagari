// Observed at df30702 before migrating prepared retention.
pub(super) struct Baseline {
    pub name: &'static str,
    pub steps: u64,
    pub effects: &'static [(&'static str, u64)],
    pub holes: &'static [(u64, u64)],
}
mod array_heap;
mod array_scalar;
mod map_custom;
mod map_heap;
mod map_scalar;
mod set_custom;
mod set_scalar;
pub(super) fn all() -> impl Iterator<Item = &'static Baseline> {
    [
        array_scalar::BASELINE,
        array_heap::BASELINE,
        map_scalar::BASELINE,
        map_heap::BASELINE,
        map_custom::BASELINE,
        set_scalar::BASELINE,
        set_custom::BASELINE,
    ]
    .into_iter()
    .flatten()
}
