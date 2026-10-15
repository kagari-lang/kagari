//! Canonical compilation-input encoding of authoritative native declarations.

use crate::{
    declaration::{
        module::{ImplDecl, ModuleDecl},
        names::NameNamespace,
        wire::{
            declarations::{TraitDefRecord, TypeDefRecord},
            leaf::NominalTyRecord,
            native::NativeCallableRequirementRecord,
            path::PathDeclarationRecord,
            signature::{FnDeclRecord, GenericBoundRecord, GenericParamRecord},
        },
    },
    ty::wire::WireType,
};
use bincode::{DefaultOptions, Options};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity,
        mapping::{DefinitionMappingError, check_cancel},
        scoped::DefinitionReferences,
        table::{
            DefinitionTable,
            wire::{DefinitionEncoding, PortableDefinitionRef},
        },
    },
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize)]
struct ModuleInput<'a> {
    prelude: bool,
    identity: &'a ModuleIdentity,
    package_alias: &'a Option<String>,
    dependencies: &'a BTreeSet<ModuleIdentity>,
    types: Vec<TypeDefRecord>,
    paths: Vec<PathDeclarationRecord>,
    variant_exports: &'a BTreeSet<String>,
    exports: BTreeMap<(u8, &'a str), PortableDefinitionRef>,
    traits: Vec<TraitDefRecord>,
    trait_references: BTreeMap<PortableDefinitionRef, TraitDefRecord>,
    implementations: Vec<ImplementationInput>,
    functions: Vec<FnDeclRecord>,
    private_functions: BTreeSet<PortableDefinitionRef>,
    module_documentation: &'a str,
    documentation: BTreeMap<PortableDefinitionRef, &'a str>,
    callable_requirements: BTreeMap<PortableDefinitionRef, Vec<NativeCallableRequirementRecord>>,
    concrete_results: BTreeMap<PortableDefinitionRef, WireType>,
}

#[derive(Serialize)]
struct ImplementationInput {
    generic_params: Vec<GenericParamRecord>,
    bounds: Vec<GenericBoundRecord>,
    trait_type: Option<NominalTyRecord>,
    for_type: WireType,
    methods: Vec<FnDeclRecord>,
}

impl ModuleDecl {
    /// Encodes all native compilation inputs with canonical file-local identities.
    ///
    /// Includes aliases, visibility, defaults, implementation selectors, callable
    /// requirements, concrete results and documentation, not just the rendered
    /// source view. Unrelated definitions and process-local allocation order do not
    /// affect these bytes. Map/set keys are sorted again after identity conversion.
    /// This is fingerprint input, not an executable or a new importable wire format;
    /// registration and ordinary declaration verification remain required.
    ///
    /// Returns cancellation, invalid identity/type shape or serialization errors.
    pub fn compiler_input_bytes(
        &self,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, DefinitionMappingError> {
        let mut references = BTreeSet::new();
        self.visit_definition_ids(
            &mut |id| {
                references.insert(*id);
                Ok(())
            },
            cancel,
        )?;
        let encoding = definitions.encode(references)?;
        let input = ModuleInput::encode(self, &encoding, cancel)?;
        check_cancel(cancel)?;
        let bytes = DefaultOptions::new()
            .with_fixint_encoding()
            .serialize(&(encoding.table, input))
            .map_err(|_| DefinitionMappingError::InvalidContract)?;
        check_cancel(cancel)?;
        Ok(bytes)
    }
}

impl<'a> ModuleInput<'a> {
    fn encode(
        value: &'a ModuleDecl,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        // Exhaustive destructuring requires new owned fields to choose their input policy.
        let ModuleDecl {
            prelude,
            identity,
            package_alias,
            dependencies,
            types,
            paths,
            variant_exports,
            exports,
            traits,
            trait_references,
            implementations,
            functions,
            private_functions,
            module_documentation,
            documentation,
            callable_requirements,
            concrete_results,
        } = value;
        check_cancel(cancel)?;
        Ok(Self {
            prelude: *prelude,
            identity,
            package_alias,
            dependencies,
            types: types
                .iter()
                .map(|value| TypeDefRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            paths: paths
                .iter()
                .map(|path| PathDeclarationRecord::encode(path, encoding, cancel))
                .collect::<Result<_, _>>()?,
            variant_exports,
            exports: exports
                .iter()
                .map(|(name, id)| {
                    check_cancel(cancel)?;
                    let namespace = match name.namespace {
                        NameNamespace::Type => 0,
                        NameNamespace::Value => 1,
                    };
                    Ok(((namespace, name.name.as_str()), encoding.reference(*id)?))
                })
                .collect::<Result<_, DefinitionMappingError>>()?,
            traits: traits
                .iter()
                .map(|value| TraitDefRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            trait_references: trait_references
                .iter()
                .map(|(id, contract)| {
                    Ok((
                        encoding.reference(*id)?,
                        TraitDefRecord::encode(contract, encoding, cancel)?,
                    ))
                })
                .collect::<Result<_, DefinitionMappingError>>()?,
            implementations: implementations
                .iter()
                .map(|value| ImplementationInput::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            functions: functions
                .iter()
                .map(|value| FnDeclRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            private_functions: private_functions
                .iter()
                .map(|id| {
                    check_cancel(cancel)?;
                    Ok(encoding.reference(*id)?)
                })
                .collect::<Result<_, DefinitionMappingError>>()?,
            module_documentation,
            documentation: documentation
                .iter()
                .map(|(id, text)| {
                    check_cancel(cancel)?;
                    Ok((encoding.reference(*id)?, text.as_str()))
                })
                .collect::<Result<_, DefinitionMappingError>>()?,
            callable_requirements: callable_requirements
                .iter()
                .map(|(id, requirements)| {
                    check_cancel(cancel)?;
                    Ok((
                        encoding.reference(*id)?,
                        requirements
                            .iter()
                            .map(|value| {
                                NativeCallableRequirementRecord::encode(value, encoding, cancel)
                            })
                            .collect::<Result<_, _>>()?,
                    ))
                })
                .collect::<Result<_, DefinitionMappingError>>()?,
            concrete_results: concrete_results
                .iter()
                .map(|(id, ty)| {
                    check_cancel(cancel)?;
                    Ok((
                        encoding.reference(*id)?,
                        WireType::encode(ty, encoding, cancel)?,
                    ))
                })
                .collect::<Result<_, DefinitionMappingError>>()?,
        })
    }
}

impl ImplementationInput {
    fn encode(
        value: &ImplDecl,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        let ImplDecl {
            generic_params,
            bounds,
            trait_type,
            for_type,
            methods,
        } = value;
        check_cancel(cancel)?;
        Ok(Self {
            generic_params: generic_params
                .iter()
                .map(|value| GenericParamRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            bounds: bounds
                .iter()
                .map(|value| GenericBoundRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            trait_type: trait_type
                .as_ref()
                .map(|value| NominalTyRecord::encode(value, encoding, cancel))
                .transpose()?,
            for_type: WireType::encode(for_type, encoding, cancel)?,
            methods: methods
                .iter()
                .map(|value| FnDeclRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        callable::CallableImplementation,
        declaration::{
            FnDecl,
            module::ModuleDecl,
            names::{ExportName, NameNamespace},
        },
        scalar::BuiltinType,
        ty::Ty,
    };
    use kagari_common::{
        cancellation::CancellationToken,
        identity::{
            DefinitionKind, ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata,
        },
    };

    fn input(reverse: bool) -> DefinitionMetadata<ModuleDecl> {
        let context = DefinitionContext::new().unwrap();
        let identity = ModuleIdentity::single_file("provider");
        let root = context.intern_root(&identity).unwrap();
        let names = if reverse { ["b", "a"] } else { ["a", "b"] };
        if reverse {
            context
                .intern_child(root, DefinitionKind::Function, "unrelated", 0)
                .unwrap();
        }
        let mut module = ModuleDecl::new(identity);
        for name in names {
            let id = context
                .intern_child(root, DefinitionKind::Function, name, 0)
                .unwrap();
            module
                .documentation
                .insert(id, format!("documentation for {name}"));
            module
                .concrete_results
                .insert(id, Ty::Builtin(BuiltinType::I32));
            module
                .exports
                .insert(ExportName::new(NameNamespace::Value, name), id);
            module.private_functions.insert(id);
            module.functions.push(FnDecl {
                host_policy: None,
                name: name.into(),
                method_policy: Default::default(),
                implementation: CallableImplementation::Native(id),
                generic_params: vec![],
                bounds: vec![],
                params: vec![],
                return_type: Ty::Builtin(BuiltinType::I32),
            });
        }
        module.functions.sort_by(|a, b| a.name.cmp(&b.name));
        DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap()
    }

    #[test]
    fn native_compiler_input_uses_portable_complete_contracts() {
        let first = input(false);
        let second = input(true);
        let encode = |value: &ModuleDecl, definitions| {
            value
                .compiler_input_bytes(definitions, &Default::default())
                .unwrap()
        };
        let bytes = encode(first.records(), first.definitions());
        assert_eq!(bytes, encode(second.records(), second.definitions()));
        // Concrete selected results are compiler authority absent from a plain
        // source-signature hash. Visibility/export changes must also invalidate.
        let mut changed = first.records().clone();
        *changed.concrete_results.values_mut().next().unwrap() = Ty::Builtin(BuiltinType::I64);
        assert_ne!(bytes, encode(&changed, first.definitions()));
        changed = first.records().clone();
        changed.private_functions.clear();
        assert_ne!(bytes, encode(&changed, first.definitions()));
        changed = first.records().clone();
        changed.exports.clear();
        assert_ne!(bytes, encode(&changed, first.definitions()));
        assert!(
            first
                .records()
                .compiler_input_bytes(second.definitions(), &Default::default())
                .is_err()
        );
        let cancel = CancellationToken::default();
        cancel.cancel();
        assert!(
            first
                .records()
                .compiler_input_bytes(first.definitions(), &cancel)
                .is_err()
        );
    }
}
