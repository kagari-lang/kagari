//! Retain referenced contracts, not arbitrary authority from an authoring view.
use crate::{
    error::RuntimeError,
    native::catalog::{DeclarationCatalog, ownership::insert_contract},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionId},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, NativeDeclaration, TypeDef, TypeDefKind, module::ModuleDecl},
    ty::{
        Constraint, GenericBound, NominalTy, Ty,
        matching::{ImplementationPattern, match_pattern},
    },
};
use std::{collections::BTreeSet, iter};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Reference {
    Type(DefinitionId),
    External(DefinitionId),
    Enum(NominalTy),
    Trait(DefinitionId),
    Template(DefinitionId),
    Obligation(Ty, NominalTy),
    Implementation(DefinitionId),
}

#[derive(Default)]
struct References {
    pending: Vec<Reference>,
}

/// A transitive closure over one explicitly scoped catalog. Extending it keeps
/// exact binding requirements while reusing already visited module references.
#[derive(Clone)]
pub(crate) struct DependencyClosure {
    pub(crate) catalog: DeclarationCatalog,
    seen: BTreeSet<Reference>,
}

impl References {
    fn nominal(&mut self, nominal: &NominalTy) -> Result<(), RuntimeError> {
        self.pending.push(Reference::Trait(nominal.declaration));
        for ty in nominal
            .arguments
            .iter()
            .chain(nominal.associated_types.values())
        {
            self.ty(ty)?;
        }
        Ok(())
    }

    fn ty(&mut self, ty: &Ty) -> Result<(), RuntimeError> {
        if !ty.within_wire_limits() {
            return Err(RuntimeError::metadata_conflict(
                "oversized native dependency type",
            ));
        }
        let mut pending = vec![ty];
        while let Some(ty) = pending.pop() {
            match ty {
                Ty::Host(id) => self.pending.push(Reference::External(*id)),
                Ty::Reference { target, .. } => pending.push(target),
                Ty::NativeObject(nominal)
                | Ty::Trait(nominal)
                | Ty::Struct(nominal)
                | Ty::Enum(nominal) => {
                    if matches!(ty, Ty::NativeObject(_) | Ty::Struct(_)) {
                        self.pending.push(Reference::Type(nominal.declaration));
                    }
                    if matches!(ty, Ty::Enum(_)) {
                        self.pending.push(Reference::Enum(nominal.clone()));
                    }
                    if matches!(ty, Ty::Trait(_)) {
                        self.pending.push(Reference::Trait(nominal.declaration));
                    }
                    pending.extend(&nominal.arguments);
                    pending.extend(nominal.associated_types.values());
                }
                Ty::Projection {
                    receiver,
                    interface,
                    arguments,
                    ..
                } => {
                    self.pending.push(Reference::Trait(interface.declaration));
                    pending.push(receiver);
                    pending.extend(&interface.arguments);
                    pending.extend(interface.associated_types.values());
                    pending.extend(arguments);
                }
                Ty::Array(item) | Ty::Set(item, _) | Ty::Iter(item) | Ty::Range(item, _) => {
                    pending.push(item)
                }
                Ty::Map { key, value, .. } => {
                    pending.push(key);
                    pending.push(value);
                }
                Ty::Tuple(items) => pending.extend(items),
                Ty::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Ty::Builtin(_) | Ty::Parameter { .. } | Ty::SelfType(_) => {}
            }
        }
        Ok(())
    }

    fn constraints(&mut self, constraints: &[Constraint]) -> Result<(), RuntimeError> {
        for constraint in constraints {
            if let Constraint::Trait(nominal) = constraint {
                self.nominal(nominal)?;
            }
        }
        Ok(())
    }

    fn bounds(&mut self, bounds: &[GenericBound]) -> Result<(), RuntimeError> {
        for bound in bounds {
            self.ty(&bound.ty)?;
            self.constraints(&bound.constraints)?;
            for constraint in &bound.constraints {
                if let Constraint::Trait(interface) = constraint {
                    self.pending
                        .push(Reference::Obligation(bound.ty.clone(), interface.clone()));
                }
            }
        }
        Ok(())
    }

    fn function(&mut self, function: &FnDecl) -> Result<(), RuntimeError> {
        self.bounds(&function.bounds)?;
        for ty in function
            .params
            .iter()
            .map(|param| &param.ty)
            .chain(iter::once(&function.return_type))
        {
            self.ty(ty)?;
        }
        if let CallableImplementation::NativeDefault(application) = &function.implementation {
            self.pending
                .push(Reference::Template(application.declaration));
            for argument in &application.arguments {
                self.ty(argument)?;
            }
        }
        Ok(())
    }

    fn declaration(&mut self, declaration: &NativeDeclaration) -> Result<(), RuntimeError> {
        self.function(&declaration.function)?;
        if let Some(receiver) = &declaration.concrete_result {
            self.ty(receiver)?;
            if let Ty::Trait(interface) = &declaration.function.return_type {
                self.pending
                    .push(Reference::Obligation(receiver.clone(), interface.clone()));
            }
        }
        for requirement in &declaration.callable_requirements {
            self.ty(&requirement.receiver)?;
            self.nominal(&requirement.interface)?;
            // A callback on a constructed receiver can use an ordinary impl
            // without an identical explicit function bound. Retain that impl
            // and its transitive bindings for source-free contract validation.
            self.pending.push(Reference::Obligation(
                requirement.receiver.clone(),
                requirement.interface.clone(),
            ));
            for argument in &requirement.arguments {
                self.ty(argument)?;
            }
        }
        Ok(())
    }

    fn type_definition(&mut self, declaration: &TypeDef) -> Result<(), RuntimeError> {
        self.bounds(&declaration.bounds)?;
        for ty in declaration.fields.iter().map(|field| &field.ty).chain(
            declaration
                .variants
                .iter()
                .flat_map(|variant| &variant.payload),
        ) {
            self.ty(ty)?;
        }
        Ok(())
    }
}

impl DeclarationCatalog {
    pub(crate) fn dependency_closure<'a>(
        &self,
        traits: impl IntoIterator<Item = DefinitionId>,
        declarations: impl IntoIterator<Item = &'a NativeDeclaration>,
        modules: impl IntoIterator<Item = &'a ModuleDecl>,
    ) -> Result<DependencyClosure, RuntimeError> {
        let mut references = References::default();
        references
            .pending
            .extend(traits.into_iter().map(Reference::Trait));
        for declaration in declarations {
            references.declaration(declaration)?;
        }
        for module in modules {
            for ty in &module.types {
                references.type_definition(ty)?;
            }
            for implementation in &module.implementations {
                if let Some(interface) = &implementation.trait_type {
                    references.nominal(interface)?;
                }
                references.ty(&implementation.for_type)?;
                references.bounds(&implementation.bounds)?;
            }
        }
        self.resolve_dependencies(
            references,
            DependencyClosure {
                catalog: Self::in_context(self.types.context().clone()),
                seen: BTreeSet::new(),
            },
        )
    }

    pub(crate) fn binding_dependencies<'a>(
        &self,
        base: &DependencyClosure,
        declarations: impl IntoIterator<Item = &'a NativeDeclaration>,
    ) -> Result<Self, RuntimeError> {
        if base.catalog.definitions().id() != self.definitions().id() {
            return Err(RuntimeError::metadata_conflict(
                "foreign native dependency seed",
            ));
        }
        let mut references = References::default();
        for declaration in declarations {
            references.declaration(declaration)?;
        }
        Ok(self.resolve_dependencies(references, base.clone())?.catalog)
    }

    fn resolve_dependencies(
        &self,
        mut references: References,
        mut closure: DependencyClosure,
    ) -> Result<DependencyClosure, RuntimeError> {
        let definitions = self.definitions();
        while let Some(reference) = references.pending.pop() {
            if !closure.seen.insert(reference.clone()) {
                continue;
            }
            match reference {
                Reference::External(id) => {
                    if !self
                        .types
                        .get_id(id)
                        .is_some_and(|declaration| matches!(declaration.kind, TypeDefKind::Host(_)))
                    {
                        return Err(RuntimeError::metadata_conflict(
                            "external type is absent from the declaration catalog",
                        ));
                    }
                    references.pending.push(Reference::Type(id));
                }
                Reference::Enum(applied) => {
                    let declaration = self.types.get_id(applied.declaration).ok_or_else(|| {
                        RuntimeError::metadata_conflict(
                            "enum is absent from the declaration catalog",
                        )
                    })?;
                    if declaration.kind != TypeDefKind::Enum
                        || declaration.generic_params.len() != applied.arguments.len()
                        || !applied.associated_types.is_empty()
                    {
                        return Err(RuntimeError::metadata_conflict(
                            "invalid registered enum application",
                        ));
                    }
                    references
                        .pending
                        .push(Reference::Type(applied.declaration));
                }
                Reference::Obligation(receiver, interface) => {
                    for (id, implementation) in self.implementations.entries() {
                        let Some(implemented) = &implementation.trait_type else {
                            continue;
                        };
                        if match_pattern(
                            ImplementationPattern {
                                parameters: &implementation.generic_params,
                                receiver: &implementation.for_type,
                                interface: implemented,
                            },
                            &interface,
                            &receiver,
                            &CancellationToken::default(),
                        )
                        .map_err(|_| {
                            RuntimeError::metadata_conflict(
                                "invalid native implementation dependency",
                            )
                        })?
                        .is_some()
                        {
                            references.pending.push(Reference::Implementation(id));
                        }
                    }
                }
                Reference::Implementation(id) => {
                    let implementation = self.implementations.get_id(id).ok_or_else(|| {
                        RuntimeError::metadata_conflict("missing native implementation dependency")
                    })?;
                    references.bounds(&implementation.bounds)?;
                    references.ty(&implementation.for_type)?;
                    if let Some(interface) = &implementation.trait_type {
                        references.nominal(interface)?;
                    }
                    for method in &implementation.methods {
                        references.pending.push(Reference::Template(
                            definitions
                                .lookup_child(id, DefinitionKind::Method, &method.name, 0)
                                .ok_or_else(|| {
                                    RuntimeError::metadata_conflict(
                                        "missing native method identity",
                                    )
                                })?,
                        ));
                    }
                    closure.catalog.insert_implementation(id, implementation)?;
                }
                Reference::Type(id) => {
                    let declaration = self.types.get_id(id).ok_or_else(|| {
                        RuntimeError::metadata_conflict(
                            "native storage type is absent from the declaration catalog",
                        )
                    })?;
                    references.type_definition(declaration)?;
                    insert_contract(
                        &mut closure.catalog.types,
                        id,
                        declaration.clone(),
                        "conflicting native storage type contracts",
                    )?;
                }
                Reference::Trait(id) => {
                    let contract = self.traits.get_id(id).ok_or_else(|| {
                        RuntimeError::metadata_conflict(
                            "native trait dependency is absent from the catalog",
                        )
                    })?;
                    references.bounds(&contract.bounds)?;
                    for parent in &contract.supertraits {
                        references.nominal(parent)?;
                    }
                    for member in &contract.associated_types {
                        references.bounds(&member.parameter_bounds)?;
                        references.constraints(&member.bounds)?;
                    }
                    for member in &contract.associated_consts {
                        references.ty(&member.ty)?;
                    }
                    for method in &contract.methods {
                        references.function(method)?;
                    }
                    insert_contract(
                        &mut closure.catalog.traits,
                        id,
                        contract.clone(),
                        "conflicting native trait contracts",
                    )?;
                }
                Reference::Template(id) => {
                    let declaration = self.declarations.get_id(id).ok_or_else(|| {
                        RuntimeError::metadata_conflict(
                            "native default template dependency is absent from the catalog",
                        )
                    })?;
                    references.declaration(declaration)?;
                    closure.catalog.insert_declaration(declaration.clone())?;
                }
            }
        }
        Ok(closure)
    }
}

#[cfg(test)]
mod tests {
    use super::DeclarationCatalog;
    use kagari_common::identity::{
        DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
        metadata::DefinitionMetadata,
    };
    use kagari_types::{
        callable::{CallableImplementation, MethodPolicy},
        declaration::{
            FieldDef, FnDecl, Param, TypeDef, TypeDefKind,
            host::{
                HostFieldPolicy, HostReflectionPolicy, HostTypeOwnership, HostTypePolicy,
                PathAccess,
            },
            module::ModuleDecl,
        },
        scalar::BuiltinType,
        ty::Ty,
        visibility::Visibility,
    };
    use std::slice;

    #[test]
    fn external_dependencies_close_recursive_schemas_and_reject_dto_identities() {
        let context = DefinitionContext::new().unwrap();
        let mut module = ModuleDecl::new(ModuleIdentity {
            package: PackageId("example".into()),
            path: vec!["roots".into()],
        });
        let root_kind = TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Metadata,
            path_access: PathAccess::ReadOnly,
        });
        for (name, kind) in [
            ("Player", root_kind),
            ("Stats", root_kind),
            ("Dto", TypeDefKind::Struct),
        ] {
            module.types.push(TypeDef {
                name: name.into(),
                kind,
                generic_params: vec![],
                bounds: vec![],
                fields: vec![],
                variants: vec![],
            });
        }
        module.collect_definitions(&context).unwrap();
        let id = |name| {
            module
                .definition(DefinitionKind::Struct, name, &context.snapshot())
                .unwrap()
        };
        let player = id("Player");
        let stats = id("Stats");
        let root = context.snapshot().lookup_root(&module.identity).unwrap();
        let entry = context
            .intern_child(root, DefinitionKind::Function, "identity", 0)
            .unwrap();
        for (name, ty, visibility) in [
            ("parent", Ty::Host(player), Visibility::Private),
            ("stats", Ty::Host(stats), Visibility::Public),
        ] {
            module.types[0].fields.push(FieldDef {
                name: name.into(),
                ty,
                visibility,
                mutable: false,
                host_policy: Some(HostFieldPolicy {
                    readable: true,
                    path_access: PathAccess::ReadOnly,
                }),
            });
        }
        module.functions.push(FnDecl {
            name: "identity".into(),
            implementation: CallableImplementation::Native(entry),
            generic_params: vec![],
            bounds: vec![],
            host_policy: None,
            method_policy: MethodPolicy::default(),
            params: vec![Param {
                name: "player".into(),
                ty: Ty::Host(player),
                mutable: false,
            }],
            return_type: Ty::Builtin(BuiltinType::Unit),
        });
        module.collect_definitions(&context).unwrap();
        let metadata =
            DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap();
        let catalog = DeclarationCatalog::from_declarations([&metadata]).unwrap();
        let declaration = catalog.declarations.values().next().unwrap();
        let closure = catalog.dependency_closure([], [declaration], []).unwrap();
        for name in ["Player", "Stats"] {
            let (id, schema) = catalog
                .types
                .entries()
                .find(|(_, schema)| schema.name == name)
                .unwrap();
            assert_eq!(closure.catalog.types.get_id(id), Some(schema));
        }
        assert!(
            closure
                .catalog
                .types
                .values()
                .all(|schema| schema.name != "Dto")
        );
        let (dto, _) = catalog
            .types
            .entries()
            .find(|(_, schema)| schema.name == "Dto")
            .unwrap();
        let mut forged = declaration.clone();
        forged.function.params[0].ty = Ty::Host(dto);
        let empty = catalog.dependency_closure([], [], []).unwrap();
        assert!(
            catalog
                .binding_dependencies(&empty, slice::from_ref(&forged))
                .is_err()
        );
    }
}
