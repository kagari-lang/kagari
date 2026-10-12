//! Invoke the comparison retained by interface construction, without trait search.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    native::context::{ScriptCall, ScriptInvoker},
    value::Value,
    value_semantics,
};
use std::slice;

impl Runtime {
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
        let required = comparison.operation.requirement();
        let operation = operations
            .operation(&self.gc, required)
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
