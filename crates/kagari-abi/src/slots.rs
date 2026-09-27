use crate::types::AbiType;
use std::collections::BTreeMap;
/// Semantic contracts supplement the physical frame layout.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SemanticSlots {
    #[serde(deserialize_with = "crate::decode_limits::map")]
    pub params: BTreeMap<usize, AbiType>,
    pub result: Option<AbiType>,
    #[serde(deserialize_with = "crate::decode_limits::map")]
    pub locals: BTreeMap<usize, AbiType>,
    #[serde(deserialize_with = "crate::decode_limits::map")]
    pub registers: BTreeMap<usize, AbiType>,
}
