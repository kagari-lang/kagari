// Observed at 5f5365f before migrating key lookup and map callbacks.
pub(super) struct Baseline {
    pub name: &'static str,
    pub steps: u64,
    pub effects: &'static [(&'static str, u64)],
    pub holes: &'static [(u64, u64)],
}
mod contains;
mod contains_key;
mod get;
mod get_or_insert_with;
mod map_insert;
mod map_remove;
mod set_insert;
mod set_remove;
mod update;
pub(super) fn all() -> impl Iterator<Item = &'static Baseline> {
    [
        get::BASELINE,
        contains_key::BASELINE,
        map_insert::BASELINE,
        map_remove::BASELINE,
        contains::BASELINE,
        set_insert::BASELINE,
        set_remove::BASELINE,
        get_or_insert_with::BASELINE,
        update::BASELINE,
    ]
    .into_iter()
    .flatten()
}
