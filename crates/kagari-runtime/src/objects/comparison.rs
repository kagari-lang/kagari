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
use kagari_contract::callable::receiver::{ReceiverComparison, object_receiver};
use kagari_types::ty::Ty;
use std::slice;

mod conditional;
mod dispatch;

impl Runtime {
    pub(super) fn receiver_comparison_binding(
        &self,
        binding: &InterfaceResultBinding,
        expression: &Ty,
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
        if matches!(
            binding.owner.bytecode.interface_tables[binding.table].comparison,
            Some(ReceiverComparison::Identity(_))
        ) {
            return Ok(Some(ReceiverComparisonBinding::Identity));
        }
        if binding.owner.bytecode.interface_tables[binding.table]
            .comparison
            .as_ref()
            .and_then(|comparison| comparison.selection())
            .is_some_and(|comparison| {
                comparison.conditions.is_empty()
                    && TypeView::new(
                        &comparison.operation.requirement().receiver,
                        &binding.owner,
                        binding.environment.as_ref().map(|env| env.types.as_ref()),
                    )
                    .identity()
                    .as_ref()
                        == Some(identity.as_ref())
            })
        {
            // The checked selection is valid for this template's binders. Its
            // active environment retains the actual arguments' supplying owners;
            // no closed application or new implementation selection is needed.
            return Ok(Some(ReceiverComparisonBinding::Selected(binding.clone())));
        }
        let mut scopes = self
            .construction_scope(&binding.owner, binding.environment.as_ref())?
            .programs()
            .to_vec();
        let mut pending = vec![receiver.clone()];
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
        if let Some(ReceiverComparison::Dispatch {
            implementations, ..
        }) = &binding.owner.bytecode.interface_tables[binding.table].comparison
        {
            return self.bind_receiver_dispatch(binding, &receiver, implementations, &scopes);
        }
        self.bind_conditional_comparison(binding, &scopes)
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
            .and_then(|table| table.comparison.as_ref()?.selection())
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
