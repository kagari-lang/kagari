//! Compare a qualified installed callee with the operation the verified instruction invokes.
use super::{AppliedCall, Expression, Value, invalid};
use crate::{module::VerifiedProgram, native::binding::NativeResult};
use kagari_bytecode::module::BytecodeModule;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    callable::interface::InterfaceCallContract,
    types::{
        PublicItem,
        proofs::{ProofCatalog, implementation::Implementation},
        trait_contract,
    },
};
use kagari_types::{
    callable::Signature,
    declaration::{FnDecl, requirement::NativeCallableRequirement, script::ScriptExpr},
    language::Protocol,
    ty::{
        NominalTy, Ty,
        substitution::{TypeSubstitution, resolve_associated_outputs},
    },
};
use std::{collections::HashSet, slice};

#[cfg(test)]
mod tests;

pub(super) fn contains(body: &ScriptExpr) -> bool {
    match body {
        ScriptExpr::TraitMethod { .. } => true,
        ScriptExpr::Call {
            callee, arguments, ..
        } => contains(callee) || arguments.iter().any(contains),
        _ => false,
    }
}

/// Only the calling executable module and its transitive dependencies can supply evidence.
/// Built once per validation pass for a module that uses qualified Script expressions.
pub(super) fn proof_scope<'a>(
    module: &'a BytecodeModule,
    program: &'a VerifiedProgram,
) -> NativeResult<ProofCatalog<'a>> {
    let definitions = program.definitions();
    let mut pending = vec![module];
    let mut visited = HashSet::new();
    let mut implementations = Vec::new();
    let mut enumerations = Vec::new();
    let mut contracts = Vec::new();
    let mut natives = Vec::new();
    while let Some(module) = pending.pop() {
        if !visited.insert(&module.identity) {
            continue;
        }
        for dependency in &module.dependencies {
            pending.push(
                program
                    .modules()
                    .get(dependency.index())
                    .ok_or_else(invalid)?,
            );
        }
        enumerations.extend(&module.enumerations);
        natives.extend(&module.native_declarations);
        contracts.extend(
            module
                .trait_contracts
                .iter()
                .map(|contract| (contract.declaration, &contract.abi)),
        );
        let root = definitions
            .lookup_root(&module.identity)
            .ok_or_else(invalid)?;
        for item in &module.public_items {
            match item {
                PublicItem::InterfaceTable(table) => {
                    implementations.push(Implementation::Interface(table))
                }
                PublicItem::Trait(contract) => contracts.push((
                    definitions
                        .lookup_child(root, DefinitionKind::Trait, &contract.name, 0)
                        .ok_or_else(invalid)?,
                    contract,
                )),
                _ => (),
            }
        }
    }
    ProofCatalog::new(
        implementations,
        enumerations,
        contracts,
        natives,
        definitions,
        &Default::default(),
    )
    .map_err(|_| invalid())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct QualifiedMethod {
    pub receiver: Ty,
    pub interface: NominalTy,
    pub member: DefinitionId,
}

impl QualifiedMethod {
    pub(super) fn checked_slot(
        call: &InterfaceCallContract,
        owner: &BytecodeModule,
        definitions: &DefinitionTable,
    ) -> NativeResult<(Self, Signature)> {
        let interface = &call.interface;
        let contract = trait_contract(
            &owner.identity,
            &owner.public_items,
            &owner.trait_contracts,
            &interface.declaration,
            definitions,
        )
        .ok_or_else(invalid)?;
        let method = contract
            .methods
            .get(call.method_slot as usize)
            .ok_or_else(invalid)?;
        let member = definitions
            .lookup_child(
                interface.declaration,
                DefinitionKind::Method,
                &method.name,
                0,
            )
            .ok_or_else(invalid)?;
        let signature = call
            .signature(contract, &Default::default(), definitions)
            .map_err(|_| invalid())?;
        Ok((
            Self {
                member,
                interface: interface.clone(),
                receiver: call
                    .receiver
                    .clone()
                    .unwrap_or_else(|| Ty::Trait(interface.clone())),
            },
            Signature {
                params: signature.params,
                result: signature.result,
            },
        ))
    }

    /// Infer only method-owned arguments, after applying the independently named
    /// receiver and trait arguments. No type argument comes from the actual instruction.
    fn arguments(
        &self,
        expression: &Expression<'_, '_>,
        method: &FnDecl,
        explicit: &[Ty],
        values: &[Value],
        expected: Option<&Ty>,
    ) -> NativeResult<Vec<Ty>> {
        if explicit.len() == method.generic_params.len() {
            return Ok(explicit.to_vec());
        }
        if !explicit.is_empty() {
            return Err(invalid());
        }
        let cancel = CancellationToken::default();
        let mut substitution =
            TypeSubstitution::for_owner(&self.interface.declaration, &self.interface.arguments);
        substitution.bind_receiver(&self.interface.declaration, &self.receiver);
        let apply = |ty: &Ty| {
            let ty = substitution.apply(ty, &cancel).map_err(|_| invalid())?;
            expression.normalize_type(
                &resolve_associated_outputs(&ty, &self.interface, &self.receiver, &cancel)
                    .map_err(|_| invalid())?,
            )
        };
        expression.inferred_arguments(method, values, expected, apply)
    }
}

impl Expression<'_, '_> {
    /// A closed specialization may call an impl supplied by its caller's module.
    /// Its executable dependencies must actually prove the substituted installed bounds;
    /// the specialization's identity arguments do not themselves constitute evidence.
    pub(super) fn check_specialization(&self, actual: Signature) -> NativeResult<()> {
        let catalog = self.proofs.ok_or_else(invalid)?;
        let cancel = CancellationToken::default();
        for bound in self
            .application
            .apply_bounds(&self.function.bounds, catalog.definitions(), &cancel)
            .map_err(|_| invalid())?
        {
            if !catalog
                .constraints_hold(&bound.ty, &bound.constraints, &[], &cancel)
                .map_err(|_| invalid())?
            {
                return Err(invalid());
            }
        }
        let applied = |ty: &Ty| {
            catalog
                .normalize(&self.apply_type(ty)?, &cancel)
                .map_err(|_| invalid())
        };
        let params = self
            .function
            .params
            .iter()
            .map(|param| applied(&param.ty))
            .collect::<NativeResult<Vec<_>>>()?;
        if actual.params != params || actual.result != applied(&self.function.return_type)? {
            return Err(invalid());
        }
        Ok(())
    }

    pub(super) fn qualified(
        &self,
        receiver: &Ty,
        interface: &NominalTy,
        member: DefinitionId,
    ) -> NativeResult<QualifiedMethod> {
        let Ty::Trait(interface) = self.apply_type(&Ty::Trait(interface.clone()))? else {
            return Err(invalid());
        };
        Ok(QualifiedMethod {
            receiver: self.apply_type(receiver)?,
            interface,
            member,
        })
    }

    // The identity specialization emits no call. Remove just that operation from the
    // installed expression; its input (including any nested effects) is still checked.
    pub(super) fn fold_identity_calls(&self, expression: &mut ScriptExpr) -> NativeResult<()> {
        let ScriptExpr::Call {
            callee,
            arguments,
            type_arguments,
        } = expression
        else {
            return Ok(());
        };
        self.fold_identity_calls(callee)?;
        for argument in &mut *arguments {
            self.fold_identity_calls(argument)?;
        }
        if let ScriptExpr::TraitMethod {
            receiver,
            interface,
            member,
        } = callee.as_ref()
            && arguments.len() == 1
            && type_arguments.is_empty()
        {
            let method = self.qualified(receiver, interface, *member)?;
            if method.receiver.is_concrete()
                && method.interface.arguments.as_slice() == slice::from_ref(&method.receiver)
                && method
                    .implicit_from(self.proofs.ok_or_else(invalid)?, vec![])?
                    .is_some()
            {
                *expression = arguments.remove(0);
            }
        }
        Ok(())
    }

    pub(super) fn authenticate_method(
        &self,
        method: &QualifiedMethod,
        explicit: &[Ty],
        call: &AppliedCall,
        values: &[Value],
        expected: Option<&Ty>,
    ) -> NativeResult<()> {
        let catalog = self.proofs.ok_or_else(invalid)?;
        let definitions = catalog.definitions();
        let contract = catalog
            .trait_contract(&method.interface.declaration)
            .ok_or_else(invalid)?;
        let declared = contract
            .methods
            .iter()
            .find(|candidate| {
                definitions.lookup_child(
                    method.interface.declaration,
                    DefinitionKind::Method,
                    &candidate.name,
                    0,
                ) == Some(method.member)
            })
            .ok_or_else(invalid)?;
        let arguments = method.arguments(self, declared, explicit, values, expected)?;
        method.check_target(catalog, arguments, call)
    }
}

impl QualifiedMethod {
    fn requirement(&self, arguments: Vec<Ty>) -> NativeCallableRequirement {
        NativeCallableRequirement {
            receiver: self.receiver.clone(),
            interface: self.interface.clone(),
            member: self.member,
            arguments,
        }
    }

    fn implicit_from(
        &self,
        catalog: &ProofCatalog<'_>,
        arguments: Vec<Ty>,
    ) -> NativeResult<Option<Signature>> {
        if Protocol::from_definition(&self.interface.declaration, catalog.definitions())
            != Some(Protocol::From)
        {
            return Ok(None);
        }
        catalog
            .implicit_callable_signature(&self.requirement(arguments), &[], &Default::default())
            .map_err(|_| invalid())
    }

    fn check_target(
        &self,
        catalog: &ProofCatalog<'_>,
        arguments: Vec<Ty>,
        call: &AppliedCall,
    ) -> NativeResult<()> {
        let matches = match &call.callee {
            // Linked verification checked the operation witnesses and the call's
            // generic scope. Match the actual slot, not just its applied signature.
            Value::TraitMethod(slot) => {
                slot.member == self.member
                    && slot.receiver == self.receiver
                    && slot.interface.satisfies(&self.interface)
                    && call.types == arguments
            }
            Value::Convert => {
                self.implicit_from(catalog, arguments)?.as_ref() == Some(&call.signature)
            }
            Value::Definition(target) => {
                let required = self.requirement(arguments);
                catalog
                    .select_callable(&required, &Default::default())
                    .map_err(|_| invalid())?
                    .is_some_and(|selection| {
                        Some(&selection.implementation) == call.entry.as_ref()
                            && selection.instance.declaration == *target
                            && selection.instance.arguments == call.types
                            && selection.signature == call.signature
                    })
            }
            _ => false,
        };
        matches.then_some(()).ok_or_else(invalid)
    }
}
