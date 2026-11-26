//! Intrinsic defining modules and explicit native constructor bindings.
use crate::{
    declaration::{TypeDef, TypeDefKind, module::DeclarationError, native::NativeTypeConstructor},
    scalar::BuiltinType,
    ty::Ty,
};
use kagari_common::identity::{
    ModuleIdentity, PackageId,
    table::{DefinitionId, DefinitionTable},
};
use std::{collections::BTreeMap, iter};

/// Checks the implementation module for inherent methods, without changing nominal
/// ownership for conversions or trait coherence. Local schema validation may see
/// only the implementation module, not its imported packaged constructor.
pub fn owns_inherent_methods(
    module: &ModuleIdentity,
    receiver: &Ty,
    defining: Option<ModuleIdentity>,
) -> bool {
    if matches!(receiver, Ty::Builtin(BuiltinType::String))
        && defining
            .as_ref()
            .is_none_or(|m| m.package.0 == "kagari-alloc" && m.path == ["string"])
    {
        return module.package.0 == "std" && module.path == ["string"];
    }
    defining.as_ref() == Some(module)
}

/// Check an authored impl's ordered inputs, not its bounds or associated outputs.
/// Kagari has no transparent fundamental wrapper: a constructor covers its arguments
/// but is local only by its own identity. An unresolved projection is not a constructor.
/// Visibility and the validity of reserved constructor declarations are checked separately.
pub fn trait_impl_orphan_error(
    module: &ModuleIdentity,
    definitions: &DefinitionTable,
    interface: DefinitionId,
    receiver: &Ty,
    arguments: &[Ty],
) -> Option<&'static str> {
    if definitions
        .resolve(interface)
        .is_ok_and(|view| view.module().package == module.package)
    {
        return None;
    }
    for input in iter::once(receiver).chain(arguments) {
        if matches!(input, Ty::Parameter { .. })
            || matches!(input, Ty::Projection { .. }) && input.contains_parameter()
        {
            return Some("uncovered impl parameter before the first local type");
        }
        if receiver_owner(input, definitions, |kind| Some(kind.declaration().module))
            .is_some_and(|owner| owner.package == module.package)
        {
            return None;
        }
    }
    Some("the impl package must own the trait or an input type")
}

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
            if NativeTypeConstructor::reserved_name(module, &declaration.name) != Some(constructor)
                || !constructor.shape_valid(declaration)
            {
                return Err(DeclarationError(
                    "invalid native constructor declaration".into(),
                ));
            }
            owners.insert(constructor, module)?;
        }
        Ok(owners)
    }

    fn insert(
        &mut self,
        constructor: NativeTypeConstructor,
        module: &ModuleIdentity,
    ) -> Result<(), DeclarationError> {
        if self
            .constructors
            .insert(constructor, module.clone())
            .is_some_and(|previous| previous != *module)
        {
            return Err(DeclarationError(
                "native constructor has multiple defining modules".into(),
            ));
        }
        Ok(())
    }

    /// Resolve receiver ownership in the supplied identity scope. Unknown or
    /// foreign nominal IDs never establish ownership.
    pub fn owner(&self, ty: &Ty, definitions: &DefinitionTable) -> Option<ModuleIdentity> {
        receiver_owner(ty, definitions, |kind| {
            self.constructors.get(&kind).cloned()
        })
    }
}

fn receiver_owner(
    ty: &Ty,
    definitions: &DefinitionTable,
    constructor_owner: impl FnOnce(NativeTypeConstructor) -> Option<ModuleIdentity>,
) -> Option<ModuleIdentity> {
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

        Ty::Range(_, kind) => NativeTypeConstructor::Range(*kind),

        _ => return None,
    };
    constructor_owner(constructor)
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
            name: "Range".into(),
            kind: TypeDefKind::Native(NativeTypeConstructor::Range(
                crate::range::RangeKind::Exclusive,
            )),
            generic_params: vec![GenericParam {
                owner: context
                    .intern_child(root, DefinitionKind::AssociatedType, "Range", 0)
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
        let left = ModuleDecl::new(
            NativeTypeConstructor::Range(crate::range::RangeKind::Exclusive)
                .declaration()
                .module,
        );
        let right = ModuleDecl::new(ModuleIdentity::single_file("right.kgr"));
        let context = DefinitionContext::new().unwrap();
        let definitions = context.snapshot();
        let receiver = Ty::Range(
            Box::new(Ty::Builtin(BuiltinType::I32)),
            crate::range::RangeKind::Exclusive,
        );
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
        assert!(ReceiverOwners::from_types([(&right.identity, &right_type)]).is_err());
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
