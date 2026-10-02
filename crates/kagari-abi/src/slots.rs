use crate::{
    native_import::{
        callables::NativeCallableRequirement,
        protocol::{adapter_arguments, adapter_contract},
    },
    types::{AbiType, ConcreteFunctionIdentity, verify::concrete_type_valid},
};
use kagari_common::identity::DefinitionKind;
use std::collections::BTreeMap;
/// Semantic contracts supplement the physical frame layout.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SemanticSlots {
    /// Applied checked language protocol implemented by this generated function.
    pub protocol_adapter: Option<NativeCallableRequirement>,
    #[serde(deserialize_with = "crate::decode_limits::map")]
    pub params: BTreeMap<usize, AbiType>,
    pub result: Option<AbiType>,
    #[serde(deserialize_with = "crate::decode_limits::map")]
    pub locals: BTreeMap<usize, AbiType>,
    #[serde(deserialize_with = "crate::decode_limits::map")]
    pub registers: BTreeMap<usize, AbiType>,
}

impl SemanticSlots {
    /// Local shape checks precede dependency-dependent protocol eligibility.
    pub fn protocol_adapter_valid(&self, identity: Option<&ConcreteFunctionIdentity>) -> bool {
        let Some(required) = &self.protocol_adapter else {
            return true;
        };
        let Some(identity) = identity else {
            return false;
        };
        let Some((kind, signature)) = adapter_contract(required) else {
            return false;
        };
        required.member.within_path_limit()
            && required.receiver.within_wire_limits()
            && concrete_type_valid(&required.receiver, &Default::default())
            && identity.arguments == adapter_arguments(required)
            && identity.declaration.path.len() == 1
            && identity.declaration.path[0].kind == DefinitionKind::Function
            && identity.declaration.path[0].name == format!("$derived_{}", kind.name())
            && identity.declaration.path[0].occurrence == 0
            && self.params.len() == signature.params.len()
            && signature
                .params
                .iter()
                .enumerate()
                .all(|(index, ty)| self.params.get(&index) == Some(ty))
            && self.result.as_ref() == Some(&signature.result)
    }
}
