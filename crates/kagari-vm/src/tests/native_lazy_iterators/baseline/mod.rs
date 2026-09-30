pub(super) struct Baseline {
    pub name: &'static str,
    pub steps: u64,
    pub allocations: usize,
    pub depth: u32,
    pub effects: &'static [(&'static str, u64)],
    pub holes: &'static [(u64, u64)],
}
mod chain;
mod chunks;
mod enumerate;
mod filter;
mod filter_map;
mod flat_map;
mod flatten;
mod fuse;
mod inspect;
mod map;
mod skip;
mod skip_while;
mod take;
mod take_while;
mod windows;
mod zip;
pub(super) fn all() -> impl Iterator<Item = &'static Baseline> {
    [
        map::BASELINE,
        filter::BASELINE,
        filter_map::BASELINE,
        take::BASELINE,
        skip::BASELINE,
        enumerate::BASELINE,
        zip::BASELINE,
        chain::BASELINE,
        take_while::BASELINE,
        skip_while::BASELINE,
        inspect::BASELINE,
        fuse::BASELINE,
        flat_map::BASELINE,
        flatten::BASELINE,
        windows::BASELINE,
        chunks::BASELINE,
    ]
    .into_iter()
    .flatten()
}
