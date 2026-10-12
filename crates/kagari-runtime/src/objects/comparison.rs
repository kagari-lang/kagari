//! Invoke the comparison retained by interface construction, without trait search.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::{arguments::type_parameter, compatibility::TypeView},
    gc::interfaces::{InterfaceResultBinding, ReceiverComparisonBinding},
    native::context::{ScriptCall, ScriptInvoker},
    objects::calls::resolve_requirement,
    value::Value,
    value_semantics,
};
use kagari_contract::callable::{receiver::object_receiver, witness::OperationWitness};
use kagari_types::ty::Ty;
use std::slice;

mod conditional;

impl Runtime {
    pub(super) fn receiver_comparison_binding(
        &self,
        binding: &InterfaceResultBinding,
        expression: &Ty,
        arguments: &[Ty],
    ) -> Result<Option<ReceiverComparisonBinding>, RuntimeError> {
        let invalid = || RuntimeError::module_validation("receiver comparison application");
        let receiver = self
            .type_arguments(
                &binding.owner,
                binding.environment.as_ref().map(|env| env.types.clone()),
                slice::from_ref(expression),
            )?
            .pop()
            .ok_or_else(invalid)?;
        if !object_receiver(receiver.ty()) {
            return Ok(None);
        }
        let identity = receiver.identity(&binding.owner).ok_or_else(invalid)?;
        if binding.owner.bytecode.interface_tables[binding.table]
            .comparison
            .as_ref()
            .is_some_and(|comparison| {
                comparison.conditions.is_empty()
                    && matches!(comparison.operation, OperationWitness::SharedMethod(_))
            })
        {
            // The checked selection is valid for this template's binders. Its
            // active environment retains the actual arguments' supplying owners;
            // no closed application or new implementation selection is needed.
            return Ok(Some(ReceiverComparisonBinding::Selected(binding.clone())));
        }
        let declaration = binding.owner.bytecode.interface_tables[binding.table].declaration;
        let mut scopes = self
            .construction_scope(&binding.owner, binding.environment.as_ref())?
            .programs()
            .to_vec();
        let mut pending = vec![receiver];
        while let Some(argument) = pending.pop() {
            if let Some(owner) = argument.supplying_owner()
                && !scopes
                    .iter()
                    .any(|scope| scope.program_identity() == owner.program_identity())
            {
                scopes.push(owner.clone());
            }
            for index in 0.. {
                if type_parameter(argument.ty(), index).is_none() {
                    break;
                }
                pending.push(argument.parameter(self, &binding.owner, index)?);
            }
        }
        if let Some(binding) = self.bind_conditional_comparison(binding, &scopes)? {
            return Ok(Some(binding));
        }
        // Only exact, prechecked applications of this implementation are eligible.
        // Structural Ty equality alone loses the generation supplying a nominal
        // element. An old shared body may be constructing a new caller's Vec<T>.
        for scope in scopes {
            let module = scope.definition(declaration)?.module();
            let owner = scope
                .members()
                .find(|owner| &owner.bytecode.identity == module)
                .ok_or_else(invalid)?;
            for (index, table) in owner.bytecode.interface_tables.iter().enumerate() {
                if table.declaration != declaration || table.arguments != arguments {
                    continue;
                }
                let Some(comparison) = &table.comparison else {
                    continue;
                };
                if !comparison.conditions.is_empty() {
                    // Guarded selections cannot execute until their actual
                    // argument eligibility is supplied. Absence proves nothing.
                    continue;
                }
                let required = comparison.operation.requirement();
                let selected =
                    self.resolve_type_arguments(&owner, slice::from_ref(&required.receiver))?;
                if selected[0].identity(&owner).as_ref() != Some(&identity) {
                    continue;
                }
                return Ok(Some(ReceiverComparisonBinding::Selected(
                    InterfaceResultBinding {
                        owner,
                        table: index,
                        arguments: arguments.to_vec(),
                        environment: binding.environment.clone(),
                    },
                )));
            }
        }
        Ok(None)
    }

    pub(crate) fn compare_interface_values(
        &self,
        left: Value,
        right: Value,
        invoke: Option<ScriptInvoker>,
    ) -> Result<Value, RuntimeError> {
        let invalid = || RuntimeError::module_validation("retained receiver comparison");
        let operands = self
            .gc
            .root_execution_values(vec![left, right])
            .ok_or_else(invalid)?;
        let interface = match (left, right) {
            (Value::Interface(id), _) | (_, Value::Interface(id)) => id,
            _ => return Err(invalid()),
        };
        let snapshot = self.gc.interface_snapshot(interface).ok_or_else(invalid)?;
        self.validate_interface_receiver(&snapshot)?;
        let binding = snapshot.comparison.clone().ok_or_else(|| {
            RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "interface receiver does not support object comparison",
            )
        })?;
        let unwrap = |value| -> Result<Value, RuntimeError> {
            if let Value::Interface(id) = value {
                let view = self.gc.interface_snapshot(id).ok_or_else(invalid)?;
                self.validate_interface_receiver(&view)?;
                if view.comparison.is_none() {
                    return Err(invalid());
                }
                Ok(view.data)
            } else {
                Ok(value)
            }
        };
        let left = unwrap(left)?;
        let right = unwrap(right)?;
        let ReceiverComparisonBinding::Selected(binding) = binding else {
            return value_semantics::identity_equal(&self.gc, &left, &right).map(Value::Bool);
        };
        let receiver = snapshot.concrete_type.clone();
        let compatible = [left, right].iter().all(|value| {
            self.matches_type_in(
                value,
                &snapshot.concrete_expression,
                &snapshot.implementation,
                snapshot
                    .environment
                    .as_ref()
                    .map(|environment| environment.types.as_ref()),
            )
        });
        drop(snapshot);
        if !compatible {
            return value_semantics::identity_equal(&self.gc, &left, &right).map(Value::Bool);
        }
        let comparison = binding
            .owner
            .bytecode
            .interface_tables
            .get(binding.table)
            .and_then(|table| table.comparison.as_ref())
            .ok_or_else(invalid)?;
        let operations = self.bind_operations_in(
            &binding.owner,
            binding.environment.clone(),
            slice::from_ref(&comparison.operation),
        )?;
        let required = resolve_requirement(
            binding.environment.as_ref(),
            comparison.operation.requirement(),
        )?;
        let receiver_identity = TypeView::new(
            &comparison.operation.requirement().receiver,
            &binding.owner,
            binding.environment.as_ref().map(|env| env.types.as_ref()),
        )
        .identity()
        .ok_or_else(invalid)?;
        let operation = operations
            .operation(&self.gc, &required, &receiver_identity)
            .ok_or_else(invalid)?;
        let root = self.root_value(left).ok_or_else(invalid)?;
        let method = RootedInterfaceMethod::from_comparison(
            self,
            root,
            operation,
            left,
            receiver,
            required.interface.clone(),
        )?;
        let method = self.apply_interface_method(method, &[], Default::default())?;
        let invoke = invoke.ok_or_else(|| {
            RuntimeError::module_validation("comparison has no execution backend")
        })?;
        let result = invoke(
            self,
            &method.implementation(self)?,
            ScriptCall::Interface(&method),
            &[left, right],
        )?;
        drop(operands);
        if !matches!(result, Value::Bool(_)) {
            return Err(invalid());
        }
        Ok(result)
    }
}
