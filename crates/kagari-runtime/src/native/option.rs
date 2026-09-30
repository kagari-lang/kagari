use crate::{
    LoadedModule, Runtime, RuntimeError, RuntimeErrorKind,
    gc::RootSet,
    native::{NativeAction, callback},
    value::{EnumTag, Value},
};
use kagari_abi::native_import::NativeSignature;

/// Keep the former Test/Branch/Read-or-Call/Move/Jump logical operations distinct.
/// The enclosing Call charges Test on entry; each later stage charges separately.
enum Phase {
    Branch(bool),
    Select(bool),
    Waiting,
    Move,
    Jump,
}

pub(super) struct OptionFallback {
    phase: Phase,
}

fn invalid() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "native option contract mismatch",
    )
}

impl OptionFallback {
    pub(super) fn start(runtime: &Runtime, value: &Value) -> Result<Self, RuntimeError> {
        let Value::Enum(id) = value else {
            return Err(invalid());
        };
        let snapshot = runtime.gc().enum_snapshot(*id).ok_or_else(invalid)?;
        let some = match snapshot.tag {
            EnumTag::OptionSome if snapshot.fields.len() == 1 => true,
            EnumTag::OptionNone if snapshot.fields.is_empty() => false,
            _ => return Err(invalid()),
        };
        Ok(Self {
            phase: Phase::Branch(some),
        })
    }

    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        implementation: &LoadedModule,
        signature: &NativeSignature,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match self.phase {
            Phase::Branch(some) => {
                self.phase = Phase::Select(some);
                Ok(NativeAction::Continue)
            }
            Phase::Select(true) => {
                let Some(Value::Enum(id)) = roots.get(0) else {
                    return Err(invalid());
                };
                let snapshot = runtime.gc().enum_snapshot(id).ok_or_else(invalid)?;
                let value = snapshot.fields.into_iter().next().ok_or_else(invalid)?;
                if !runtime.matches_interface_method_abi(&value, &signature.result, implementation)
                {
                    return Err(invalid());
                }
                roots.set(runtime.gc(), 2, value).ok_or_else(invalid)?;
                self.phase = Phase::Move;
                Ok(NativeAction::Continue)
            }
            Phase::Select(false) => {
                let value = roots.get(1).ok_or_else(invalid)?;
                let request = callback(runtime, &value, &signature.params[1], vec![])?;
                self.phase = Phase::Waiting;
                Ok(NativeAction::Callback(request))
            }
            Phase::Waiting => Err(RuntimeError::module_validation(
                "native callback has not returned",
            )),
            Phase::Move => {
                let value = roots.get(2).ok_or_else(invalid)?;
                if !runtime.matches_interface_method_abi(&value, &signature.result, implementation)
                {
                    return Err(invalid());
                }
                self.phase = Phase::Jump;
                Ok(NativeAction::Publish(value))
            }
            Phase::Jump => Ok(NativeAction::Finish),
        }
    }

    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
        value: Value,
    ) -> Result<(), RuntimeError> {
        if !matches!(self.phase, Phase::Waiting) {
            return Err(RuntimeError::module_validation(
                "unexpected native callback result",
            ));
        }
        roots.set(runtime.gc(), 2, value).ok_or_else(invalid)?;
        self.phase = Phase::Move;
        Ok(())
    }
}
