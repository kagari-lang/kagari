//! Function-owned layout operands and exact environment applications.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::environments::EnvironmentId,
    frame::types::TypeEnvironment,
    module::{
        EnumVariantRef, LoadedModule, StructLayoutRef, execution::ExecutionFunction,
        linked_execution::LinkedFunction,
    },
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, EnumId, StructId},
    module::BytecodeFunction,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::Ty;
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

#[derive(Debug, Clone)]
pub(crate) enum AggregateLayout {
    Struct(StructLayoutRef),
    Enum(EnumVariantRef),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Aggregate {
    Struct(StructId),
    Enum(EnumId, u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LayoutOperand {
    aggregate: Aggregate,
    arguments: Box<[Ty<DefinitionId>]>,
}

#[derive(Debug)]
enum LinkedLayout {
    Closed(AggregateLayout),
    Scoped {
        operand: LayoutOperand,
        index: usize,
    },
}

/// Pure layout facts own provenance, never executable environment leases or Values.
#[derive(Debug)]
pub(crate) struct AppliedLayouts {
    entries: Box<[OnceLock<AggregateLayout>]>,
    #[cfg(debug_assertions)]
    environment: EnvironmentId,
}

#[derive(Debug)]
pub(crate) struct FunctionLayouts {
    sites: Box<[(usize, usize)]>,
    operands: Box<[LinkedLayout]>,
    scoped_count: usize,
}

impl LinkedFunction {
    pub(crate) fn ready_field_layout(&self, pc: usize) -> Option<&StructLayoutRef> {
        let AggregateLayout::Struct(layout) = self
            .layouts
            .as_ref()?
            .ready(self.applied_layouts.as_deref(), pc)?
        else {
            return None;
        };
        Some(layout)
    }
}

impl FunctionLayouts {
    pub(super) fn link(
        runtime: &Runtime,
        owner: &LoadedModule,
        function: &BytecodeFunction<DefinitionId>,
        prepared: &ExecutionFunction,
    ) -> Result<Option<Arc<Self>>, RuntimeError> {
        if !prepared.has_layout_operands {
            return Ok(None);
        }
        let mut sites = Vec::new();
        let mut operands = Vec::new();
        let mut identities = HashMap::new();
        let mut scoped_count = 0;
        for (pc, instruction) in function.instructions.iter().enumerate() {
            let (aggregate, arguments) = match instruction {
                BytecodeInstruction::MakeStruct {
                    structure,
                    arguments,
                    ..
                } => (Aggregate::Struct(*structure), arguments),
                BytecodeInstruction::MakeEnum {
                    enumeration,
                    arguments,
                    variant,
                    ..
                }
                | BytecodeInstruction::TestEnumVariant {
                    enumeration,
                    arguments,
                    variant,
                    ..
                }
                | BytecodeInstruction::ReadEnumPayload {
                    enumeration,
                    arguments,
                    variant,
                    ..
                } => (Aggregate::Enum(*enumeration, *variant), arguments),
                BytecodeInstruction::ReadAggregateField { field, .. }
                | BytecodeInstruction::WriteAggregateField { field, .. }
                    if prepared
                        .fields
                        .binary_search_by_key(&pc, |field| field.pc)
                        .is_ok_and(|index| !prepared.fields[index].concrete) =>
                {
                    (Aggregate::Struct(field.structure), &field.arguments)
                }
                _ => continue,
            };
            let operand = LayoutOperand {
                aggregate,
                arguments: arguments.clone().into_boxed_slice(),
            };
            let index = if let Some(&index) = identities.get(&operand) {
                index
            } else {
                let linked = if arguments.iter().all(Ty::is_concrete) {
                    LinkedLayout::Closed(operand.prepare(runtime, owner, None)?)
                } else {
                    let index = scoped_count;
                    scoped_count += 1;
                    LinkedLayout::Scoped {
                        operand: operand.clone(),
                        index,
                    }
                };
                let index = operands.len();
                operands.push(linked);
                identities.insert(operand, index);
                index
            };
            sites.push((pc, index));
        }
        Ok((!sites.is_empty()).then(|| {
            Arc::new(Self {
                sites: sites.into_boxed_slice(),
                operands: operands.into_boxed_slice(),
                scoped_count,
            })
        }))
    }

    pub(super) fn is_scoped(&self) -> bool {
        self.scoped_count != 0
    }

    pub(super) fn application(&self, environment: EnvironmentId) -> Arc<AppliedLayouts> {
        #[cfg(not(debug_assertions))]
        let _ = environment;
        Arc::new(AppliedLayouts {
            entries: (0..self.scoped_count).map(|_| OnceLock::new()).collect(),
            #[cfg(debug_assertions)]
            environment,
        })
    }

    fn operand(&self, pc: usize) -> Option<&LinkedLayout> {
        let site = self.sites.binary_search_by_key(&pc, |&(pc, _)| pc).ok()?;
        self.operands.get(self.sites[site].1)
    }

    /// Read admitted facts without preparing metadata inside a closed cursor.
    pub(crate) fn ready<'a>(
        &'a self,
        active: Option<&'a AppliedLayouts>,
        pc: usize,
    ) -> Option<&'a AggregateLayout> {
        match self.operand(pc)? {
            LinkedLayout::Closed(layout) => Some(layout),
            LinkedLayout::Scoped { index, .. } => active?.entries.get(*index)?.get(),
        }
    }

    pub(crate) fn resolve<'a>(
        &'a self,
        runtime: &Runtime,
        owner: &LoadedModule,
        environment: Option<&TypeEnvironment>,
        active: Option<&'a AppliedLayouts>,
        pc: usize,
    ) -> Result<&'a AggregateLayout, RuntimeError> {
        let operand = self
            .operand(pc)
            .ok_or_else(|| RuntimeError::module_validation("missing prepared layout operand"))?;
        match operand {
            LinkedLayout::Closed(layout) => Ok(layout),
            LinkedLayout::Scoped { operand, index } => {
                let environment = environment
                    .ok_or_else(|| RuntimeError::module_validation("missing layout environment"))?;
                // Frame admission validates this exact generation and roots its graph.
                // Function entry selects the descriptor for that exact environment.
                let applied = active.ok_or_else(|| {
                    RuntimeError::module_validation("missing function layout application")
                })?;
                #[cfg(debug_assertions)]
                debug_assert_eq!(applied.environment, environment.id);
                let entry = &applied.entries[*index];
                if entry.get().is_none() {
                    let layout = operand.prepare(runtime, owner, Some(environment))?;
                    // Preparation performs no collection or callback. Only successful
                    // immutable facts are published, at the original instruction.
                    let _ = entry.set(layout);
                }
                Ok(entry.get().expect("published layout operand"))
            }
        }
    }
}

impl LayoutOperand {
    fn prepare(
        &self,
        runtime: &Runtime,
        owner: &LoadedModule,
        environment: Option<&TypeEnvironment>,
    ) -> Result<AggregateLayout, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::LayoutOperandPreparation);
        let declaration = match self.aggregate {
            Aggregate::Struct(id) => owner
                .bytecode
                .structures
                .get(id.index())
                .map(|layout| layout.declaration),
            Aggregate::Enum(id, _) => owner
                .bytecode
                .enumerations
                .get(id.index())
                .map(|layout| layout.declaration),
        }
        .ok_or_else(|| RuntimeError::module_validation("invalid layout operand"))?;
        let (arguments, scope);
        let types;
        if environment.is_none() {
            arguments = self.arguments.as_ref();
            scope = None;
        } else {
            let supplied = runtime.type_arguments(
                owner,
                environment.map(|scope| scope.types.clone()),
                &self.arguments,
            )?;
            scope = runtime.prepare_layout_scope(owner, declaration, &supplied)?;
            types = supplied
                .iter()
                .map(|argument| argument.ty().clone())
                .collect::<Vec<_>>();
            arguments = &types;
        }
        match self.aggregate {
            Aggregate::Struct(id) => runtime
                .modules
                .applied_struct_layout(owner, id, arguments, scope)
                .map(AggregateLayout::Struct),
            Aggregate::Enum(id, variant) => runtime
                .modules
                .applied_enum_variant(owner, id, arguments, variant, scope)
                .map(AggregateLayout::Enum),
        }
        .ok_or_else(|| RuntimeError::module_validation("invalid layout application"))
    }
}
