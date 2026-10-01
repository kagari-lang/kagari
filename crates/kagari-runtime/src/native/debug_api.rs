//! Direct debug operations are ordinary optional native functions.
#[kagari_native_macros::native_module("std::debug", runtime = crate)]
pub mod debug {
    use crate::{
        error::{RuntimeError, RuntimeErrorKind},
        native::{NativeAction, NativeContext, NativeInvocationState},
        native_value::{NativeResult, continuation::NativeContinuation, never::NativeNever},
        value::Value,
    };
    use kagari_common::host_interface::standard_log;

    /// Write a message through the installed host.log binding and its permissions.
    /// Arguments are evaluated first; completed host effects survive later traps.
    /// Missing, denied or failed logging traps. The host owns the output destination.
    #[native]
    pub fn print(message: String) -> NativeContinuation<()> {
        NativeContinuation::new(Log {
            message: Some(message),
            entered: false,
        })
    }

    /// Trap with the supplied message when condition is false. Both arguments are eager.
    #[native]
    pub fn assert(condition: bool, message: String) -> NativeResult<()> {
        if condition {
            Ok(())
        } else {
            Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                format!("debug.assert failed: {message}"),
            ))
        }
    }

    /// Always trap with the supplied message and caller trace; no script value returns.
    #[native]
    pub fn panic(message: String) -> NativeResult<NativeNever> {
        Err(RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            format!("debug.panic: {message}"),
        ))
    }

    struct Log {
        message: Option<String>,
        entered: bool,
    }
    impl NativeInvocationState for Log {
        fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            // Enter the charged native frame before invoking external host effects.
            if !self.entered {
                self.entered = true;
                return Ok(NativeAction::Continue);
            }
            let message = self
                .message
                .take()
                .ok_or_else(|| RuntimeError::module_validation("host log already invoked"))?;
            context
                .invoke_host(&standard_log().symbol, &[Value::Str(message)])
                .map(NativeAction::Complete)
        }
    }
}
