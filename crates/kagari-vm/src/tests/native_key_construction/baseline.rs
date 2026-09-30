// Observed at 9c09890 before migrating Map/Set construction.
pub(super) struct Baseline {
    pub name: &'static str,
    pub steps: u64,
    pub effects: &'static [(&'static str, u64)],
    pub holes: &'static [(u64, u64)],
}
mod map_from_identity;
mod map_from_iter_identity;
mod map_from_iter_nominal;
mod map_from_iter_option;
mod map_from_iter_scalar;
mod map_from_iter_tuple;
mod map_from_nominal;
mod map_from_option;
mod map_from_scalar;
mod map_from_tuple;
mod set_from_identity;
mod set_from_iter_identity;
mod set_from_iter_nominal;
mod set_from_iter_option;
mod set_from_iter_scalar;
mod set_from_iter_tuple;
mod set_from_nominal;
mod set_from_option;
mod set_from_scalar;
mod set_from_tuple;
pub(super) fn all() -> impl Iterator<Item = &'static Baseline> {
    [
        map_from_scalar::BASELINE,
        map_from_nominal::BASELINE,
        map_from_tuple::BASELINE,
        map_from_option::BASELINE,
        map_from_identity::BASELINE,
        map_from_iter_scalar::BASELINE,
        map_from_iter_nominal::BASELINE,
        map_from_iter_tuple::BASELINE,
        map_from_iter_option::BASELINE,
        map_from_iter_identity::BASELINE,
        set_from_scalar::BASELINE,
        set_from_nominal::BASELINE,
        set_from_tuple::BASELINE,
        set_from_option::BASELINE,
        set_from_identity::BASELINE,
        set_from_iter_scalar::BASELINE,
        set_from_iter_nominal::BASELINE,
        set_from_iter_tuple::BASELINE,
        set_from_iter_option::BASELINE,
        set_from_iter_identity::BASELINE,
    ]
    .into_iter()
    .flatten()
}
