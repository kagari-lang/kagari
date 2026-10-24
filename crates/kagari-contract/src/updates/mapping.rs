//! Identity traversal uses the existing scoped importer, including impl correspondence.
use crate::updates::{
    BodyApplication, BodyClassification, BodyRecord, BodyRequirement, CallRequirement,
    CanonicalInstruction, CanonicalOperand, CurrentTarget, DemandEnvelope, DemandRequests,
    Dependency, InterfaceRequest, ProtectedRoot, SourceRecord, UpdateEvidence,
    admission::nested,
    limits::{Budget, Limits, MAX_APPLICATIONS, UpdateError},
};
use crate::{
    callable::{
        construction::evidence::ConstructionWitness, generic::GenericBody,
        interface::InterfaceCallContract, shared::SharedCall, witness::OperationWitness,
    },
    native_import::NativeImport,
    types::ConcreteFunctionIdentity,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};
use kagari_types::{
    callable::Signature,
    ty::{NominalTy, Ty},
};

trait Value: Sized {
    fn visit_value(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError>;

    fn map_value(&self, mapper: &mut DefinitionMapper<'_>) -> Result<Self, DefinitionMappingError>;
}

macro_rules! delegate {
    ($($ty:ty),*) => { $(
        impl Value for $ty {
            fn visit_value(&self, visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>, cancel: &CancellationToken) -> Result<(), DefinitionMappingError> {
                self.visit_definition_ids(visit, cancel)
            }

            fn map_value(&self, mapper: &mut DefinitionMapper<'_>) -> Result<Self, DefinitionMappingError> {
                self.remap_definitions(mapper)
            }
        }
    )* };
}

delegate!(
    DemandRequests,
    InterfaceRequest,
    NominalTy,
    Ty,
    Signature,
    ConcreteFunctionIdentity,
    GenericBody,
    SharedCall,
    InterfaceCallContract,
    NativeImport,
    OperationWitness,
    ConstructionWitness,
    UpdateEvidence,
    SourceRecord,
    BodyApplication,
    BodyRecord,
    BodyRequirement,
    CanonicalInstruction,
    ProtectedRoot,
    CurrentTarget,
    DemandEnvelope,
    BodyClassification,
    CanonicalOperand,
    CallRequirement,
    Dependency
);

impl Value for DefinitionId {
    fn visit_value(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(self)
    }

    fn map_value(&self, mapper: &mut DefinitionMapper<'_>) -> Result<Self, DefinitionMappingError> {
        mapper.reference(self)
    }
}

impl<T: Value> Value for Vec<T> {
    fn visit_value(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        if self.len() > 1_000_000 {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        for value in self {
            check_cancel(cancel)?;
            value.visit_value(visit, cancel)?;
        }
        Ok(())
    }

    fn map_value(&self, mapper: &mut DefinitionMapper<'_>) -> Result<Self, DefinitionMappingError> {
        if self.len() > 1_000_000 {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        self.iter().map(|value| value.map_value(mapper)).collect()
    }
}

impl<T: Value> Value for Option<T> {
    fn visit_value(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        if let Some(value) = self {
            value.visit_value(visit, cancel)?;
        }
        Ok(())
    }

    fn map_value(&self, mapper: &mut DefinitionMapper<'_>) -> Result<Self, DefinitionMappingError> {
        self.as_ref()
            .map(|value| value.map_value(mapper))
            .transpose()
    }
}

impl<T: Value> Value for Box<T> {
    fn visit_value(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.as_ref().visit_value(visit, cancel)
    }

    fn map_value(&self, mapper: &mut DefinitionMapper<'_>) -> Result<Self, DefinitionMappingError> {
        Ok(Box::new(self.as_ref().map_value(mapper)?))
    }
}

fn guard_error(error: UpdateError) -> DefinitionMappingError {
    match error {
        UpdateError::Cancelled => DefinitionMappingError::Cancelled,
        _ => DefinitionMappingError::LimitExceeded,
    }
}

fn evidence_guard(
    value: &UpdateEvidence,
    cancel: &CancellationToken,
) -> Result<(), DefinitionMappingError> {
    let mut budget = Budget::new(Limits::default(), cancel).map_err(guard_error)?;
    if value.bodies.len() > MAX_APPLICATIONS || value.targets.len() > MAX_APPLICATIONS {
        return Err(DefinitionMappingError::LimitExceeded);
    }
    for count in [
        value.sources.len(),
        value.bodies.len(),
        value.roots.len(),
        value.targets.len(),
        value.selections.len(),
    ] {
        budget.records(count).map_err(guard_error)?;
    }
    for source in &value.sources {
        source_plain(source, &mut budget).map_err(guard_error)?;
    }
    for body in &value.bodies {
        body_plain(body, &mut budget).map_err(guard_error)?;
    }
    Ok(())
}

fn source_guard(
    value: &SourceRecord,
    cancel: &CancellationToken,
) -> Result<(), DefinitionMappingError> {
    source_plain(
        value,
        &mut Budget::new(Limits::default(), cancel).map_err(guard_error)?,
    )
    .map_err(guard_error)
}

fn body_guard(
    value: &BodyRecord,
    cancel: &CancellationToken,
) -> Result<(), DefinitionMappingError> {
    body_plain(
        value,
        &mut Budget::new(Limits::default(), cancel).map_err(guard_error)?,
    )
    .map_err(guard_error)
}

fn source_plain(value: &SourceRecord, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
    budget.records(value.tokens.len())?;
    nested(value.inputs.len())?;
    nested(value.calls.len())?;
    nested(value.dependencies.len())?;
    for token in &value.tokens {
        budget.bytes(token.text.len())?;
    }
    for operand in &value.inputs {
        operand_plain(operand, budget)?;
    }
    Ok(())
}

fn body_plain(value: &BodyRecord, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
    budget.records(value.instructions.len())?;
    budget.records(value.actual_instructions.len())?;
    nested(value.requirement.calls.len())?;
    nested(value.dependencies.len())?;
    for instruction in value.instructions.iter().chain(&value.actual_instructions) {
        nested(instruction.operands.len())?;
        budget.records(instruction.operands.len())?;
        for operand in &instruction.operands {
            operand_plain(operand, budget)?;
        }
    }
    Ok(())
}

fn operand_plain(value: &CanonicalOperand, budget: &mut Budget<'_>) -> Result<(), UpdateError> {
    budget.check()?;
    if let CanonicalOperand::Bytes(bytes) = value {
        nested(bytes.len())?;
        budget.bytes(bytes.len())?;
    }
    Ok(())
}

macro_rules! record {
    ($name:ident { $($mapped:ident),* ; $($plain:ident),* } $(guard $guard:ident)?) => {
        impl DefinitionReferences for $name {
            fn visit_definition_ids(
                &self,
                visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
                cancel: &CancellationToken,
            ) -> Result<(), DefinitionMappingError> {
                check_cancel(cancel)?;
                $($guard(self, cancel)?;)?
                $(self.$mapped.visit_value(visit, cancel)?;)*
                Ok(())
            }
        }

        impl RemapDefinitions for $name {
            fn remap_definitions(&self, mapper: &mut DefinitionMapper<'_>) -> Result<Self, DefinitionMappingError> {
                mapper.check()?;
                $($guard(self, mapper.cancellation())?;)?
                Ok(Self {
                    $($mapped: self.$mapped.map_value(mapper)?,)*
                    $($plain: self.$plain.clone(),)*
                })
            }
        }
    };
}

macro_rules! variants {
    ($name:ident { $($mapped:ident),* ; $($plain:ident),* }) => {
        impl DefinitionReferences for $name {
            fn visit_definition_ids(
                &self,
                visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
                cancel: &CancellationToken,
            ) -> Result<(), DefinitionMappingError> {
                check_cancel(cancel)?;
                match self {
                    $(Self::$mapped(value) => value.visit_value(visit, cancel),)*
                    $(Self::$plain(_) => Ok(()),)*
                }
            }
        }

        impl RemapDefinitions for $name {
            fn remap_definitions(&self, mapper: &mut DefinitionMapper<'_>) -> Result<Self, DefinitionMappingError> {
                mapper.check()?;
                Ok(match self {
                    $(Self::$mapped(value) => Self::$mapped(value.map_value(mapper)?),)*
                    $(Self::$plain(value) => Self::$plain(value.clone()),)*
                })
            }
        }
    };
}

record!(UpdateEvidence { sources, bodies, roots, targets, selections; } guard evidence_guard);
record!(SourceRecord { declaration, generic, inputs, calls, dependencies; tokens } guard source_guard);
record!(BodyApplication { body, arguments; });
record!(BodyRecord { requirement, classification, signature, captures, instructions, actual_instructions, dependencies; format, actual_format } guard body_guard);
record!(BodyRequirement { origin, generic, calls; });
record!(CanonicalInstruction { operands; opcode });
record!(ProtectedRoot { declaration, dependencies, selections; kind });
record!(CurrentTarget { application, body; module, function });
record!(DemandEnvelope { applications, bodies, requirements; });
record!(DemandRequests { functions, shared, interfaces, constructions, layouts; });
record!(InterfaceRequest { interface, receiver, arguments; method_slot });
variants!(BodyClassification { Named, Closure, Async, Resume, Adapter, StaticInitializer; });
variants!(CanonicalOperand { Declaration, Type, Application; Local, Block, Unsigned, Signed, Bytes, Call });
variants!(CallRequirement { Direct, Shared, Interface, RetainedBody, Native, Operation, Construction, Layout; Unknown });
variants!(Dependency { Source, Body; Unknown });
