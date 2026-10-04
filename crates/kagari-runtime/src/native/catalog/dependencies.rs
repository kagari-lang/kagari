//! Retain referenced contracts, not arbitrary authority from an authoring view.
use crate::{error::RuntimeError, native::catalog::DeclarationCatalog};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionPath};
use kagari_contract::types::proofs::{ProofCatalog, implementation::Implementation};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, NativeDeclaration, module::ModuleDecl},
    ty::{
        Constraint, GenericBound, NominalTy, Ty,
        matching::{ImplementationPattern, match_pattern},
    },
};
use std::{collections::BTreeSet, iter};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Reference {
    Type(DefinitionPath),
    Trait(DefinitionPath),
    Template(DefinitionPath),
    Obligation(Ty, NominalTy),
    Implementation(DefinitionPath),
}

#[derive(Default)]
struct References {
    pending: Vec<Reference>,
}

/// A transitive closure over one immutable authoring catalog. Extending it keeps
/// exact binding requirements while reusing already visited module references.
#[derive(Clone, Default)]
pub(crate) struct DependencyClosure {
    pub(crate) catalog: DeclarationCatalog<DefinitionPath>,
    seen: BTreeSet<Reference>,
}

impl References {
    fn nominal(&mut self, nominal: &NominalTy) -> Result<(), RuntimeError> {
        self.pending
            .push(Reference::Trait(nominal.declaration.clone()));
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
                Ty::NativeObject(nominal)
                | Ty::Trait(nominal)
                | Ty::Struct(nominal)
                | Ty::Enum(nominal) => {
                    if matches!(ty, Ty::NativeObject(_)) {
                        self.pending
                            .push(Reference::Type(nominal.declaration.clone()));
                    }
                    if matches!(ty, Ty::Trait(_)) {
                        self.pending
                            .push(Reference::Trait(nominal.declaration.clone()));
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
                    self.pending
                        .push(Reference::Trait(interface.declaration.clone()));
                    pending.push(receiver);
                    pending.extend(&interface.arguments);
                    pending.extend(interface.associated_types.values());
                    pending.extend(arguments);
                }
                Ty::Array(item, _) | Ty::Set(item, _) | Ty::Iter(item) | Ty::Range(item, _) => {
                    pending.push(item)
                }
                Ty::Map { key, value, .. } => {
                    pending.push(key);
                    pending.push(value);
                }
                Ty::Tuple(items) | Ty::StandardEnum { args: items, .. } => pending.extend(items),
                Ty::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Ty::Builtin(_) | Ty::Parameter { .. } | Ty::SelfType(_) | Ty::Host { .. } => {}
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
                .push(Reference::Template(application.declaration.clone()));
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
            for argument in &requirement.arguments {
                self.ty(argument)?;
            }
        }
        Ok(())
    }
}

impl DeclarationCatalog<DefinitionPath> {
    pub(crate) fn dependency_closure<'a>(
        &self,
        traits: impl IntoIterator<Item = DefinitionPath>,
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
                references.bounds(&ty.bounds)?;
            }
            for implementation in &module.implementations {
                if let Some(interface) = &implementation.trait_type {
                    references.nominal(interface)?;
                }
                references.ty(&implementation.for_type)?;
                references.bounds(&implementation.bounds)?;
            }
        }
        self.resolve_dependencies(references, DependencyClosure::default())
    }

    pub(crate) fn binding_dependencies<'a>(
        &self,
        base: &DependencyClosure,
        declarations: impl IntoIterator<Item = &'a NativeDeclaration>,
    ) -> Result<Self, RuntimeError> {
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
        while let Some(reference) = references.pending.pop() {
            if !closure.seen.insert(reference.clone()) {
                continue;
            }
            match reference {
                Reference::Obligation(receiver, interface) => {
                    for (id, implementation) in self.implementations.iter() {
                        let Some(implemented) = &implementation.trait_type else {
                            continue;
                        };
                        if match_pattern(
                            ImplementationPattern {
                                storage_access: self
                                    .traits
                                    .get(&implemented.declaration)
                                    .and_then(|contract| contract.storage_access),
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
                            references
                                .pending
                                .push(Reference::Implementation(id.clone()));
                        }
                    }
                }
                Reference::Implementation(id) => {
                    let implementation = self.implementations.get(&id).ok_or_else(|| {
                        RuntimeError::metadata_conflict("missing native implementation dependency")
                    })?;
                    references.bounds(&implementation.bounds)?;
                    references.ty(&implementation.for_type)?;
                    if let Some(interface) = &implementation.trait_type {
                        references.nominal(interface)?;
                    }
                    for method in &implementation.methods {
                        references
                            .pending
                            .push(Reference::Template(ModuleDecl::method_id(
                                &id,
                                &method.name,
                            )));
                    }
                    closure
                        .catalog
                        .insert_implementation(id, implementation.clone())?;
                }
                Reference::Type(id) => {
                    let declaration = self.types.get(&id).ok_or_else(|| {
                        RuntimeError::metadata_conflict(
                            "native storage type is absent from the declaration catalog",
                        )
                    })?;
                    references.bounds(&declaration.bounds)?;
                    closure.catalog.insert_type(id, declaration.clone())?;
                }
                Reference::Trait(id) => {
                    let contract = self.get(&id).ok_or_else(|| {
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
                    closure.catalog.insert(id, contract.clone())?;
                }
                Reference::Template(id) => {
                    let declaration = self.declarations.get(&id).ok_or_else(|| {
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

    pub(crate) fn validate_callable_contracts(&self) -> Result<(), RuntimeError> {
        let implementations: Vec<_> = self.implementations.iter().collect();
        let catalog = ProofCatalog::new(
            implementations
                .iter()
                .map(|(declaration, implementation)| Implementation::Native {
                    declaration,
                    implementation,
                })
                .collect(),
            vec![],
            [],
            self.traits
                .iter()
                .map(|(id, contract)| (id.clone(), contract)),
            self.declarations.values(),
            &CancellationToken::default(),
        )
        .map_err(|_| {
            RuntimeError::metadata_conflict(
                "native template defaults or selected calls differ from their declared contracts",
            )
        })?;
        for declaration in self.declarations.values() {
            if let Some(receiver) = &declaration.concrete_result {
                let Ty::Trait(interface) = &declaration.function.return_type else {
                    return Err(RuntimeError::metadata_conflict(
                        "native concrete result requires an interface return",
                    ));
                };
                if !catalog
                    .constraints_hold(
                        receiver,
                        &[Constraint::Trait(interface.clone())],
                        &declaration.function.bounds,
                        &CancellationToken::default(),
                    )
                    .map_err(|_| RuntimeError::metadata_conflict("invalid native result proof"))?
                {
                    return Err(RuntimeError::metadata_conflict(
                        "native concrete result does not implement its return interface",
                    ));
                }
            }
        }
        Ok(())
    }
}
