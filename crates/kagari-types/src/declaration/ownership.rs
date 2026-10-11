//! Intrinsic defining modules and explicit native constructor bindings.
use crate::{
    declaration::{TypeDef, TypeDefKind, module::DeclarationError, native::NativeTypeConstructor},
    scalar::BuiltinType,
    ty::Ty,
};
use kagari_common::identity::{ModuleIdentity, PackageId};
use std::collections::BTreeMap;

/// Constructor ownership is supplied by declarations, independent of API names.
#[derive(Debug, Clone, Default)]
pub struct ReceiverOwners {
    constructors: BTreeMap<NativeTypeConstructor, ModuleIdentity>,
}

impl ReceiverOwners {
    pub fn from_types<'a>(
        types: impl IntoIterator<Item = (&'a ModuleIdentity, &'a TypeDef)>,
    ) -> Result<Self, DeclarationError> {
        let mut owners = Self::default();
        for (module, declaration) in types {
            let TypeDefKind::Native(constructor) = declaration.kind else {
                continue;
            };
            if !constructor.shape_valid(declaration) {
                return Err(DeclarationError(
                    "invalid native constructor declaration".into(),
                ));
            }
            if owners
                .constructors
                .insert(constructor, module.clone())
                .is_some_and(|previous| previous != *module)
            {
                return Err(DeclarationError(
                    "native constructor has multiple defining modules".into(),
                ));
            }
        }
        Ok(owners)
    }

    pub fn owner(&self, ty: &Ty) -> Option<ModuleIdentity> {
        let constructor = match ty {
            Ty::NativeObject(nominal) | Ty::Struct(nominal) | Ty::Enum(nominal) => {
                return Some(nominal.declaration.module.clone());
            }
            // Primitive scalar identity is part of language semantics, not a library recipe.
            Ty::Builtin(kind) if *kind != BuiltinType::String => {
                return Some(ModuleIdentity {
                    package: PackageId("kagari-core".into()),
                    path: vec!["num".into()],
                });
            }
            Ty::Builtin(BuiltinType::String) => NativeTypeConstructor::String,
            Ty::Array(..) => {
                return Some(ModuleIdentity {
                    package: PackageId("kagari-core".into()),
                    path: vec!["array".into()],
                });
            }
            Ty::Map { .. } => NativeTypeConstructor::Map,
            Ty::Set(..) => NativeTypeConstructor::Set,
            Ty::Iter(_) => NativeTypeConstructor::Iter,
            Ty::Range(_, kind) => NativeTypeConstructor::Range(*kind),

            _ => return None,
        };
        self.constructors.get(&constructor).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{collection::CollectionAccess, declaration::module::ModuleDecl, ty::GenericParam};
    use kagari_common::identity::DefinitionKind;

    fn sequence(module: &ModuleDecl) -> TypeDef {
        TypeDef {
            name: "Sequence".into(),
            kind: TypeDefKind::Native(NativeTypeConstructor::Set),
            generic_params: vec![GenericParam {
                owner: module.definition(DefinitionKind::AssociatedType, "Sequence"),
                position: 0,
            }],
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        }
    }

    #[test]
    fn native_family_ownership_comes_from_validated_bindings() {
        let left = ModuleDecl::new(ModuleIdentity::single_file("left.kgr"));
        let right = ModuleDecl::new(ModuleIdentity::single_file("right.kgr"));
        let receiver = Ty::Set(
            Box::new(Ty::Builtin(BuiltinType::I32)),
            CollectionAccess::Mutable,
        );
        assert_eq!(ReceiverOwners::default().owner(&receiver), None);
        let left_type = sequence(&left);
        let right_type = sequence(&right);
        let owners = ReceiverOwners::from_types([(&left.identity, &left_type)]).unwrap();
        assert_eq!(owners.owner(&receiver), Some(left.identity.clone()));
        assert!(
            ReceiverOwners::from_types([
                (&left.identity, &left_type),
                (&right.identity, &right_type)
            ])
            .is_err()
        );
        let mut invalid = left_type;
        invalid.generic_params.clear();
        assert!(ReceiverOwners::from_types([(&left.identity, &invalid)]).is_err());
    }
}
