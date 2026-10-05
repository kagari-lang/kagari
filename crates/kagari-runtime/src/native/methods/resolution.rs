use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        methods::{InherentMember, receiver::AppliedReceiver},
    },
};
use kagari_common::identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment};
use kagari_contract::types::PublicItem;
use kagari_types::ty::matching::match_receiver;

impl AppliedReceiver {
    pub(crate) fn method(&self, name: &str) -> NativeResult<InherentMember> {
        self.find_member(|candidate, _| candidate == name)
    }

    pub(crate) fn method_declaration(
        &self,
        declaration: &DefinitionPath,
    ) -> NativeResult<InherentMember> {
        self.find_member(|_, candidate| candidate == declaration)
    }

    fn find_member(
        &self,
        matches: impl Fn(&str, &DefinitionPath) -> bool,
    ) -> NativeResult<InherentMember> {
        let mut found = None;
        for owner in self.owner().members() {
            for item in &owner.bytecode.public_items {
                let PublicItem::InherentTable(table) = item else {
                    continue;
                };
                if match_receiver(
                    &table.generic_params,
                    &table.for_type,
                    self.type_argument().ty(),
                    &Default::default(),
                )
                .map_err(|_| RuntimeError::module_validation("inherent receiver template"))?
                .is_none()
                {
                    continue;
                }
                for method in &table.methods {
                    let mut declaration = owner.definition(table.declaration)?.to_path();
                    declaration.path.push(DefinitionPathSegment {
                        kind: DefinitionKind::Method,
                        name: method.name.clone(),
                        occurrence: 0,
                    });
                    if !matches(&method.name, &declaration) {
                        continue;
                    }
                    if found.is_some() {
                        return Err(RuntimeError::module_validation("ambiguous inherent member"));
                    }
                    found = Some(InherentMember {
                        applied: self.clone(),
                        declaration,
                        receiver: table.for_type.clone(),
                        parameters: table.generic_params.clone(),
                        method_arity: method.generic_params.len(),
                        has_receiver: method
                            .params
                            .first()
                            .is_some_and(|param| param.name == "self"),
                    });
                }
            }
        }
        found.ok_or_else(|| RuntimeError::module_validation("unknown public inherent member"))
    }
}
