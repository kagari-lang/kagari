//! Function-owned layout operands and exact environment applications.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::environments::EnvironmentId,
    frame::types::TypeEnvironment,
    module::{
        EnumVariantRef, ExecutableModule, StructLayoutRef,
        execution::{
            ExecutionFunction,
            aggregates::{AggregateKind, AggregateOperand, LayoutSite},
        },
        linked_execution::LinkedFunction,
    },
};
use kagari_bytecode::module::BytecodeFunction;
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
struct LayoutOperand {
    aggregate: AggregateKind,
    arguments: Box<[Ty]>,
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
    pub(crate) fn ready_field_layout(&self, site: LayoutSite) -> Option<&StructLayoutRef> {
        let AggregateLayout::Struct(layout) = self
            .layouts
            .as_ref()?
            .ready(self.applied_layouts.as_deref(), site)?
        else {
            return None;
        };
        Some(layout)
    }
}

impl FunctionLayouts {
    pub(super) fn link(
        runtime: &Runtime,
        owner: &ExecutableModule,
        function: &BytecodeFunction,
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
            let Some(input) = AggregateOperand::from_instruction(instruction) else {
                continue;
            };
            let operand = LayoutOperand {
                aggregate: input.kind,
                arguments: input.arguments.into(),
            };
            let index = if let Some(&index) = identities.get(&operand) {
                index
            } else {
                let linked = if input.arguments.iter().all(Ty::is_concrete) {
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

    pub(crate) fn site(&self, pc: usize) -> Option<LayoutSite> {
        self.sites
            .binary_search_by_key(&pc, |&(pc, _)| pc)
            .ok()
            .map(LayoutSite)
    }

    fn operand(&self, site: LayoutSite) -> Option<&LinkedLayout> {
        self.operands.get(self.sites.get(site.0)?.1)
    }

    /// Read admitted facts without preparing metadata inside a closed cursor.
    pub(crate) fn ready<'a>(
        &'a self,
        active: Option<&'a AppliedLayouts>,
        site: LayoutSite,
    ) -> Option<&'a AggregateLayout> {
        match self.operand(site)? {
            LinkedLayout::Closed(layout) => Some(layout),
            LinkedLayout::Scoped { index, .. } => active?.entries.get(*index)?.get(),
        }
    }

    pub(crate) fn resolve<'a>(
        &'a self,
        runtime: &Runtime,
        owner: &ExecutableModule,
        environment: Option<&TypeEnvironment>,
        active: Option<&'a AppliedLayouts>,
        site: LayoutSite,
    ) -> Result<&'a AggregateLayout, RuntimeError> {
        let operand = self
            .operand(site)
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
        owner: &ExecutableModule,
        environment: Option<&TypeEnvironment>,
    ) -> Result<AggregateLayout, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::LayoutOperandPreparation);
        let declaration = match self.aggregate {
            AggregateKind::Struct(id) => owner
                .bytecode
                .structures
                .get(id.index())
                .map(|layout| layout.declaration),
            AggregateKind::Enum(id, _) => owner
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
            AggregateKind::Struct(id) => runtime
                .modules
                .applied_struct_layout(owner, id, arguments, scope)
                .map(AggregateLayout::Struct),
            AggregateKind::Enum(id, variant) => runtime
                .modules
                .applied_enum_variant(owner, id, arguments, variant, scope)
                .map(AggregateLayout::Enum),
        }
        .ok_or_else(|| RuntimeError::module_validation("invalid layout application"))
    }
}
