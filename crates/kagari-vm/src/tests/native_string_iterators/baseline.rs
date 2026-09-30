// Observed at 2643293 before native string iterator construction.
pub(super) struct Baseline {
    pub name: &'static str,
    pub steps: u64,
    pub effects: &'static [(&'static str, u64)],
    pub holes: &'static [(u64, u64)],
}
mod bytes;
mod char_indices;
mod lines;
mod split;
mod split_whitespace;
mod splitn;
pub(super) fn all() -> impl Iterator<Item = &'static Baseline> {
    [
        bytes::BASELINE,
        char_indices::BASELINE,
        split::BASELINE,
        splitn::BASELINE,
        split_whitespace::BASELINE,
        lines::BASELINE,
    ]
    .into_iter()
    .flatten()
}
