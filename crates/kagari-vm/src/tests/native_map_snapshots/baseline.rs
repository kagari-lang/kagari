// Observed at 6eaf7e3 before replacing both Map snapshot lowering paths.
pub(super) struct Baseline {
    pub name: &'static str,
    pub steps: u64,
    pub effects: &'static [(&'static str, u64)],
}
mod entries;
mod keys;
mod values;
pub(super) fn all() -> impl Iterator<Item = &'static Baseline> {
    [keys::BASELINE, values::BASELINE, entries::BASELINE]
        .into_iter()
        .flatten()
}
