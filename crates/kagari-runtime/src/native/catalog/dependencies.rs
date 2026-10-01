//! Retain referenced contracts, not arbitrary authority from an authoring view.
use crate::{error::RuntimeError, native::catalog::NativeCatalog};
use kagari_abi::{
    callable::CallableImplementation,
    native_api::NativeModule,
    types::{
        AbiType, ConstraintAbi, FunctionAbi, GenericBoundAbi, NativeDeclaration, NominalAbiType,
        proofs::ProofCatalog,
    },
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionId};
use std::{collections::BTreeSet, iter};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Reference {
    Trait(DefinitionId),
    Template(DefinitionId),
}

#[derive(Default)]
struct References {
    pending: Vec<Reference>,
}
impl References {
    fn nominal(&mut self, nominal: &NominalAbiType) -> Result<(), RuntimeError> {
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
    fn ty(&mut self, ty: &AbiType) -> Result<(), RuntimeError> {
        if !ty.within_wire_limits() {
            return Err(RuntimeError::metadata_conflict(
                "oversized native dependency type",
            ));
        }
        let mut pending = vec![ty];
        while let Some(ty) = pending.pop() {
            match ty {
                AbiType::Trait(nominal) | AbiType::Struct(nominal) | AbiType::Enum(nominal) => {
                    if matches!(ty, AbiType::Trait(_)) {
                        self.pending
                            .push(Reference::Trait(nominal.declaration.clone()));
                    }
                    pending.extend(&nominal.arguments);
                    pending.extend(nominal.associated_types.values());
                }
                AbiType::Projection {
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
                AbiType::Array(item, _)
                | AbiType::Set(item, _)
                | AbiType::Iter(item)
                | AbiType::Range(item, _) => pending.push(item),
                AbiType::Map { key, value, .. } => {
                    pending.push(key);
                    pending.push(value);
                }
                AbiType::Tuple(items) | AbiType::StandardEnum { args: items, .. } => {
                    pending.extend(items)
                }
                AbiType::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                AbiType::Builtin(_)
                | AbiType::Parameter { .. }
                | AbiType::SelfType(_)
                | AbiType::Host { .. } => {}
            }
        }
        Ok(())
    }
    fn constraints(&mut self, constraints: &[ConstraintAbi]) -> Result<(), RuntimeError> {
        for constraint in constraints {
            if let ConstraintAbi::Trait(nominal) = constraint {
                self.nominal(nominal)?;
            }
        }
        Ok(())
    }
    fn bounds(&mut self, bounds: &[GenericBoundAbi]) -> Result<(), RuntimeError> {
        for bound in bounds {
            self.ty(&bound.ty)?;
            self.constraints(&bound.constraints)?;
        }
        Ok(())
    }
    fn function(&mut self, function: &FunctionAbi) -> Result<(), RuntimeError> {
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

impl NativeCatalog {
    pub(crate) fn dependencies<'a>(
        &self,
        traits: impl IntoIterator<Item = &'a DefinitionId>,
        declarations: impl IntoIterator<Item = &'a NativeDeclaration>,
        modules: impl IntoIterator<Item = &'a NativeModule>,
    ) -> Result<Self, RuntimeError> {
        let mut references = References::default();
        references
            .pending
            .extend(traits.into_iter().cloned().map(Reference::Trait));
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
        let mut result = Self::default();
        let mut seen = BTreeSet::new();
        while let Some(reference) = references.pending.pop() {
            if !seen.insert(reference.clone()) {
                continue;
            }
            match reference {
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
                    result.insert(id, contract.clone())?;
                }
                Reference::Template(id) => {
                    let declaration = self.declarations.get(&id).ok_or_else(|| {
                        RuntimeError::metadata_conflict(
                            "native default template dependency is absent from the catalog",
                        )
                    })?;
                    references.declaration(declaration)?;
                    result.insert_declaration(declaration.clone())?;
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn validate_defaults(&self) -> Result<(), RuntimeError> {
        if !self
            .traits
            .values()
            .flat_map(|contract| &contract.methods)
            .any(|method| {
                matches!(
                    method.implementation,
                    CallableImplementation::NativeDefault(_)
                )
            })
        {
            return Ok(());
        }
        ProofCatalog::new(
            vec![],
            vec![],
            [],
            self.traits
                .iter()
                .map(|(id, contract)| (id.clone(), contract)),
            self.declarations.values(),
            &CancellationToken::default(),
        )
        .map_err(|_| {
            RuntimeError::metadata_conflict("native default differs from its registered template")
        })?;
        Ok(())
    }
}
