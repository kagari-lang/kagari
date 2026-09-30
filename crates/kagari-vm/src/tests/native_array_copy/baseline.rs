// Observed at 60ffc8e before migrating array construction/copying.
pub(super) struct Baseline {
    pub name: &'static str,
    pub steps: u64,
    pub effects: &'static [(&'static str, u64)],
    pub holes: &'static [(u64, u64)],
}
mod appending;
mod collection;
mod copying;
mod factories;
pub(super) fn all() -> impl Iterator<Item = &'static Baseline> {
    [
        factories::BASELINE,
        collection::BASELINE,
        copying::BASELINE,
        appending::BASELINE,
    ]
    .into_iter()
    .flatten()
}
