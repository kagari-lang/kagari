//! Checked product of handwritten library/core trait modules.
//! Executable consumers decode declarations without loading source or HIR.
use crate::{
    language::{self, role::LangRole},
    types::{PublicItem, TraitDef, verify},
};
use bincode::{DefaultOptions, Options};
use kagari_common::cancellation::CancellationToken;
use std::sync::OnceLock;

pub(crate) fn declarations() -> Vec<TraitDef> {
    static TRAITS: OnceLock<Vec<TraitDef>> = OnceLock::new();
    TRAITS
        .get_or_init(|| {
            let traits: Vec<TraitDef> = DefaultOptions::new()
                .with_fixint_encoding()
                .with_little_endian()
                .with_limit(64 * 1024)
                .deserialize(include_bytes!("traits.bin"))
                .expect("checked language declaration product");
            assert_eq!(traits.len(), LangRole::ALL.len());
            for (role, item) in LangRole::ALL.into_iter().zip(&traits) {
                assert_eq!(
                    item.name,
                    role.protocol().name(),
                    "language product role identity"
                );
                verify::validate(
                    &[PublicItem::Trait(item.clone())],
                    &language::identity(role.protocol()).module,
                    &CancellationToken::default(),
                )
                .expect("valid checked language declaration product");
            }
            traits
        })
        .clone()
}
