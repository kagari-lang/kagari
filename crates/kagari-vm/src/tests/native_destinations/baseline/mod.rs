mod collect;
mod direct;
mod fallible;
mod partition;
pub(super) struct Baseline {
    pub name: &'static str,
    pub steps: u64,
    pub effects: &'static [(&'static str, u64)],
    pub holes: &'static [(u64, u64)],
}
pub(super) fn all() -> impl Iterator<Item = &'static Baseline> {
    collect::BASELINE
        .iter()
        .chain(direct::BASELINE)
        .chain(fallible::BASELINE)
        .chain(partition::BASELINE)
}
