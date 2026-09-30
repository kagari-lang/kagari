// Observed at 56bf0bd before replacing the List equality query algorithms.
pub(super) struct Baseline {
    pub name: &'static str,
    pub steps: u64,
    pub effects: &'static [(&'static str, u64)],
}
mod enumeration;
mod leaf;
mod nested;
mod option;
mod scalar;
mod tuple;
pub(super) fn all() -> impl Iterator<Item = &'static Baseline> {
    [
        scalar::BASELINE,
        leaf::BASELINE,
        tuple::BASELINE,
        option::BASELINE,
        enumeration::BASELINE,
        nested::BASELINE,
    ]
    .into_iter()
    .flatten()
}
