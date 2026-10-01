//! Application natives reuse host invocation and the uninhabited result adapter.
use kagari_native_macros::native_module;

#[native_module("game::diagnostics")]
pub mod diagnostics {
    use kagari_runtime::{
        error::{RuntimeError, RuntimeErrorKind},
        native::{NativeAction, NativeContext, NativeInvocationState},
        native_value::{NativeResult, continuation::NativeContinuation, never::NativeNever},
        value::Value,
    };

    /// Application-owned UTF-8 representation for a library-free installation.
    #[native_type]
    pub type Text = String;

    /// Call a declared, permitted application host service through the common boundary.
    #[native]
    pub fn echo(text: Text) -> NativeContinuation<Text> {
        NativeContinuation::new(Echo {
            text: Some(text),
            entered: false,
        })
    }

    /// A real uninhabited Rust result needs no standard library declaration.
    #[native]
    pub fn fatal() -> NativeResult<NativeNever> {
        Err(RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "application fatal",
        ))
    }

    struct Echo {
        text: Option<String>,
        entered: bool,
    }
    impl NativeInvocationState for Echo {
        fn advance(&mut self, context: &mut NativeContext<'_>) -> NativeResult<NativeAction> {
            if !self.entered {
                self.entered = true;
                return Ok(NativeAction::Continue);
            }
            let text = self
                .text
                .take()
                .ok_or_else(|| RuntimeError::module_validation("echo already invoked"))?;
            context
                .invoke_host("app.echo", &[Value::Str(text)])
                .map(NativeAction::Complete)
        }
    }
}
