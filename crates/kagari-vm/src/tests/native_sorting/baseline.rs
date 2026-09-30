// Observed at 490fe39 before migrating stable sort and adjacent dedup.
pub(super) struct Baseline {
    pub name: &'static str,
    pub steps: u64,
    pub effects: &'static [(&'static str, u64)],
    pub holes: &'static [(u64, u64)],
}
mod choice;
mod dedup;
mod option;
mod sort;
mod sort_by;
mod sort_by_key;
mod tuple;
pub(super) fn all() -> impl Iterator<Item = &'static Baseline> {
    [
        sort::BASELINE,
        sort_by::BASELINE,
        sort_by_key::BASELINE,
        dedup::BASELINE,
        tuple::BASELINE,
        option::BASELINE,
        choice::BASELINE,
    ]
    .into_iter()
    .flatten()
}
