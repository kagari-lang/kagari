//! Method applications reuse closed call witnesses from the installed program.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::managed::{environments::EnvironmentObject, sources::MetadataSource},
    frame::types::{
        EnvironmentRecord,
        arguments::{ScopedSignature, TypeArgument},
        compatibility::TypeView,
        construction::ConstructionScope,
        operations::OperationBindings,
    },
    module::{ExecutableModule, ModuleKey},
    native::{
        binding::NativeResult, conversion::context::ConversionContext,
        interfaces::binding::InterfaceMember,
    },
};
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
use kagari_contract::{callable::interface::InterfaceCallContract, ids::FunctionRef};
use kagari_gc::root::Root;
use kagari_types::ty::Ty;
use std::slice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ApplicationKey(ModuleKey, FunctionRef, usize);

pub(super) struct ApplicationEvidence {
    pub(super) key: ApplicationKey,
    owner: ExecutableModule,
    contract: InterfaceCallContract,
}

#[derive(Debug)]
pub(super) struct AppliedMethod {
    pub(super) key: ApplicationKey,
    pub(super) arguments: Vec<TypeArgument>,
    pub(super) operations: OperationBindings,
    pub(super) constructions: ConstructionScope,
    pub(super) environment: Root<EnvironmentObject>,
}

fn missing() -> RuntimeError {
    RuntimeError::module_validation("generic interface method has no checked closed call witness")
}

impl ApplicationEvidence {
    pub(super) fn find(
        runtime: &Runtime,
        member: &InterfaceMember,
        arguments: &[TypeArgument],
    ) -> NativeResult<Self> {
        for argument in arguments {
            argument.validate(runtime)?;
        }
        for owner in member.owner.members() {
            for function in &owner.bytecode.functions {
                if function.metadata.semantic.generic.is_some() {
                    continue;
                }
                for (offset, instruction) in function.instructions.iter().enumerate() {
                    let BytecodeInstruction::Call {
                        callee: CallTarget::InterfaceMethod { contract, .. },
                        ..
                    } = instruction
                    else {
                        continue;
                    };
                    let interface = Ty::Trait(contract.interface.clone());
                    if contract.receiver.is_some()
                        || contract.method_slot as usize != member.slot
                        || !interface.is_concrete()
                        || contract.arguments.len() != arguments.len()
                        || !member
                            .interface
                            .view(&member.owner)
                            .compatible(TypeView::new(&interface, &owner, None))
                        || !contract
                            .arguments
                            .iter()
                            .zip(arguments)
                            .all(|(ty, argument)| {
                                ty.is_concrete()
                                    && argument
                                        .view(&member.owner)
                                        .compatible(TypeView::new(ty, &owner, None))
                            })
                    {
                        continue;
                    }
                    return Ok(Self {
                        key: ApplicationKey(owner.key(), function.id, offset),
                        owner: owner.clone(),
                        contract: *contract.clone(),
                    });
                }
            }
        }
        Err(missing())
    }

    pub(super) fn signature(
        &self,
        runtime: &Runtime,
        member: &InterfaceMember,
    ) -> NativeResult<ScopedSignature> {
        let signature = self
            .contract
            .signature(&member.contract, &Default::default(), member.definitions())
            .map_err(|_| missing())?;
        let params = signature.params;
        let result = signature.result;
        Ok(ScopedSignature {
            params: runtime
                .resolve_type_arguments(&self.owner, params.get(1..).ok_or_else(missing)?)?,
            result: runtime
                .resolve_type_arguments(&self.owner, slice::from_ref(&result))?
                .pop()
                .ok_or_else(missing)?,
        })
    }

    pub(super) fn prepare(
        &self,
        runtime: &mut Runtime,
        member: &InterfaceMember,
    ) -> NativeResult<AppliedMethod> {
        let arguments = runtime.resolve_type_arguments(&self.owner, &self.contract.arguments)?;
        let parameters = member.contract.methods[member.slot].generic_params.clone();
        let mut conversion = ConversionContext::new(runtime, &self.owner)?;
        let execution = conversion.execution_id()?;
        let mut protected = vec![MetadataSource::Revision {
            owner: &self.owner,
            source: member.origin(),
        }];
        let operations = conversion.runtime_mut().bind_operations_in(
            execution,
            &self.owner,
            None,
            &self.contract.operations,
            &mut protected,
        )?;
        let mut environment =
            EnvironmentRecord::new(self.owner.definitions(), parameters, arguments.clone())?;
        let constructions = conversion.runtime_mut().bind_constructions_in(
            execution,
            &self.owner,
            None,
            &self.contract.constructions,
            &mut protected,
        )?;
        environment.set_constructions(constructions.clone());
        environment.extend_operations(operations.clone());
        let environment =
            conversion
                .runtime_mut()
                .alloc_environment(execution, environment, &protected)?;
        let environment = conversion.runtime_mut().retain_environment(&environment)?;
        Ok(AppliedMethod {
            key: self.key,
            arguments,
            operations,
            constructions,
            environment,
        })
    }
}
