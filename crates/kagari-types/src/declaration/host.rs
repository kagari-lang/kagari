//! External object access policy attached to shared declarations, never signatures.
use crate::{
    callable::CallableImplementation,
    declaration::{FieldDef, FnDecl},
    decode_limits,
    language::binding,
    scalar::BuiltinType,
    ty::Ty,
};
use kagari_common::identity::table::DefinitionTable;
use serde::{Deserialize, Serialize};
use std::iter;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostPassingStyle {
    Owned,
    SharedBorrow,
    UniqueBorrow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HostFunctionEffects {
    pub may_allocate: bool,
    pub may_trap: bool,
    /// Reads only host-provided immutable configuration, with value-only inputs
    /// and results. This does not authorize general service access or mutation.
    pub may_read_immutable_configuration: bool,
    pub may_call_host_services: bool,
    pub may_mutate_host_state: bool,
    pub may_suspend: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostTypeOwnership {
    Opaque,
    Owned,
    HostRoot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostReflectionPolicy {
    Hidden,
    TypeNameOnly,
    Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PathAccess {
    None,
    ReadOnly,
    ReadWrite,
}

/// External identity eligibility, reflection and manual path capabilities. Rust
/// data stays outside the heap; this policy grants no generated Rust reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostTypePolicy {
    pub ownership: HostTypeOwnership,
    pub reflection: HostReflectionPolicy,
    pub path_access: PathAccess,
}

/// A shared field declaration already owns name/type/visibility/write access.
/// This policy only supplies external readable/path capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostFieldPolicy {
    pub readable: bool,
    pub path_access: PathAccess,
}

impl HostFieldPolicy {
    pub(crate) fn valid_for(self, field: &FieldDef) -> bool {
        (self.path_access == PathAccess::None || self.readable)
            && (self.path_access != PathAccess::ReadWrite || field.mutable)
    }
}

/// Manual external-ID adapters use these passing styles at the common native
/// boundary. Parameter names/types and the result belong solely to FnDecl.
///
/// For `heal(player: Player, amount: i32) -> i32`, `[UniqueBorrow, Owned]`
/// reserves the Player identity before the callback. It does not create &mut Player.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostCallPolicy {
    #[serde(deserialize_with = "decode_limits::nested")]
    pub passing: Vec<HostPassingStyle>,
    pub effects: HostFunctionEffects,
}

impl HostCallPolicy {
    pub(crate) fn valid_for(&self, function: &FnDecl, table: &DefinitionTable) -> bool {
        self.passing.len() == function.params.len()
            && self.passing.len() <= decode_limits::MAX_NESTED_RECORDS
            && matches!(function.implementation, CallableImplementation::Native(_))
            && function.generic_params.is_empty()
            && function.method_policy.receiver_access.is_none()
            && self
                .passing
                .iter()
                .zip(&function.params)
                .all(|(passing, param)| {
                    !param.ty.contains_reference()
                        && (*passing == HostPassingStyle::Owned
                            || matches!(param.ty, Ty::Host(_))
                            || (*passing == HostPassingStyle::SharedBorrow
                                && param.ty == Ty::Builtin(BuiltinType::String)))
                })
            && !function.return_type.contains_reference()
            && (!self.effects.may_read_immutable_configuration
                || (self
                    .passing
                    .iter()
                    .all(|passing| *passing == HostPassingStyle::Owned)
                    && function
                        .params
                        .iter()
                        .map(|param| &param.ty)
                        .chain(iter::once(&function.return_type))
                        .all(|ty| immutable_value(ty, table))))
    }
}

pub(crate) fn function_policy_valid(function: &FnDecl, table: &DefinitionTable) -> bool {
    if let Some(policy) = &function.host_policy {
        return policy.valid_for(function, table);
    }
    if !matches!(function.implementation, CallableImplementation::Native(_)) {
        return true;
    }
    let mut pending = function
        .params
        .iter()
        .map(|parameter| &parameter.ty)
        .chain(iter::once(&function.return_type))
        .collect::<Vec<_>>();
    while let Some(ty) = pending.pop() {
        match ty {
            Ty::Host(_) => return false,
            Ty::Projection {
                receiver,
                interface,
                arguments,
                ..
            } => {
                pending.push(receiver);
                pending.extend(&interface.arguments);
                pending.extend(interface.associated_types.values());
                pending.extend(arguments);
            }
            Ty::Tuple(items) => pending.extend(items),
            Ty::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            Ty::Reference { target, .. } => pending.push(target),
            Ty::Array(item) | Ty::Set(item, _) | Ty::Iter(item) | Ty::Range(item, _) => {
                pending.push(item)
            }
            Ty::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
            Ty::Struct(nominal)
            | Ty::Enum(nominal)
            | Ty::NativeObject(nominal)
            | Ty::Trait(nominal) => {
                pending.extend(&nominal.arguments);
                pending.extend(nominal.associated_types.values());
            }
            Ty::Builtin(_) | Ty::Parameter { .. } | Ty::SelfType(_) => {}
        }
    }
    true
}

fn immutable_value(ty: &Ty, table: &DefinitionTable) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        match ty {
            Ty::Builtin(_) => {}
            Ty::Tuple(items) => pending.extend(items),
            Ty::Enum(nominal)
                if nominal.associated_types.is_empty()
                    && ((nominal.arguments.len() == 1
                        && binding::matches(
                            &nominal.declaration,
                            &binding::option_declaration(),
                            table,
                        ))
                        || (nominal.arguments.len() == 2
                            && binding::matches(
                                &nominal.declaration,
                                &binding::result_declaration(),
                                table,
                            ))) =>
            {
                pending.extend(&nominal.arguments);
            }
            _ => return false,
        }
    }
    true
}
