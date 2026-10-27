//! Intrinsic defining modules and explicit native constructor bindings.
use crate::{
    declaration::{TypeDef, TypeDefKind, module::DeclarationError, native::NativeTypeConstructor},
    scalar::BuiltinType,
    ty::Ty,
};
use kagari_common::identity::{ModuleIdentity, PackageId, table::DefinitionTable};
use std::collections::BTreeMap;

/// Constructor ownership is supplied by declarations, independent of API names.
#[derive(Debug, Clone, Default)]
pub struct ReceiverOwners {
    constructors: BTreeMap<NativeTypeConstructor, ModuleIdentity>,
}

impl ReceiverOwners {
    /// Select a dependency-visible view of already validated constructor owners.
    ///
    /// This filters the finite constructor index, not declaration records. Nominal
    /// ownership still requires the caller's retained identity table.
    pub fn restricted_to(&self, visible: impl Fn(&ModuleIdentity) -> bool) -> Self {
        Self {
            constructors: self
                .constructors
                .iter()
                .filter(|(_, module)| visible(module))
                .map(|(constructor, module)| (*constructor, module.clone()))
                .collect(),
        }
    }

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

    /// Resolve receiver ownership in the supplied identity scope. Unknown or
    /// foreign nominal IDs never establish ownership.
    pub fn owner(&self, ty: &Ty, definitions: &DefinitionTable) -> Option<ModuleIdentity> {
        let constructor = match ty {
            Ty::Host(declaration) => {
                return definitions
                    .resolve(*declaration)
                    .ok()
                    .map(|view| view.module().clone());
            }
            Ty::NativeObject(nominal)
            | Ty::Struct(nominal)
            | Ty::Enum(nominal)
            | Ty::Trait(nominal) => {
                return definitions
                    .resolve(nominal.declaration)
                    .ok()
                    .map(|view| view.module().clone());
            }
            Ty::Builtin(BuiltinType::Char) => {
                return Some(ModuleIdentity {
                    package: PackageId("kagari-core".into()),
                    path: vec!["char".into()],
                });
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
    use crate::{
        declaration::module::ModuleDecl,
        ty::{GenericParam, NominalTy},
    };
    use kagari_common::identity::{DefinitionKind, map::DefinitionContext};

    fn sequence(module: &ModuleDecl, context: &DefinitionContext) -> TypeDef {
        let root = context.intern_root(&module.identity).unwrap();
        TypeDef {
            name: "Sequence".into(),
            kind: TypeDefKind::Native(NativeTypeConstructor::Iter),
            generic_params: vec![GenericParam {
                owner: context
                    .intern_child(root, DefinitionKind::AssociatedType, "Sequence", 0)
                    .unwrap(),
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
        let context = DefinitionContext::new().unwrap();
        let definitions = context.snapshot();
        let receiver = Ty::Iter(Box::new(Ty::Builtin(BuiltinType::I32)));
        assert_eq!(
            ReceiverOwners::default().owner(&receiver, &definitions),
            None
        );
        let left_type = sequence(&left, &context);
        let right_type = sequence(&right, &context);
        let owners = ReceiverOwners::from_types([(&left.identity, &left_type)]).unwrap();
        assert_eq!(
            owners.owner(&receiver, &definitions),
            Some(left.identity.clone())
        );
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

    #[test]
    fn nominal_receiver_ownership_requires_its_retained_scope() {
        let context = DefinitionContext::new().unwrap();
        let module = ModuleIdentity::single_file("owner.kgr");
        let root = context.intern_root(&module).unwrap();
        let before = context.snapshot();
        let id = context
            .intern_child(root, DefinitionKind::Struct, "Value", 0)
            .unwrap();
        let receiver = Ty::Struct(NominalTy {
            declaration: id,
            arguments: vec![],
            associated_types: BTreeMap::new(),
        });
        let owners = ReceiverOwners::default();
        let retained = context.snapshot();
        assert_eq!(owners.owner(&receiver, &retained), Some(module));
        assert_eq!(owners.owner(&receiver, &before), None);
        let foreign = DefinitionContext::new().unwrap();
        let path = retained.resolve(id).unwrap().to_path();
        let foreign_id = foreign.intern(&path).unwrap();
        assert_ne!(id, foreign_id);
        assert_eq!(owners.owner(&receiver, &foreign.snapshot()), None);
        context
            .intern_child(root, DefinitionKind::Struct, "Later", 0)
            .unwrap();
        assert_eq!(
            owners.owner(&receiver, &retained),
            owners.owner(&receiver, &context.snapshot())
        );
    }
}
