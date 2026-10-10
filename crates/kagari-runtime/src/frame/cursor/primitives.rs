//! Only an installed runtime-owned body can execute without the native callback boundary.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{
        cursor::{ExecutionCursor, kernel::RegionError},
        values::operands::OperandWindow,
    },
    module::{execution::layout::Location, linked_execution::LinkedPrimitiveBody},
    native::primitive::{PrimitiveResult, string_byte_length},
};

pub(super) enum NativeContinuation {
    Complete,
    Boundary,
}

impl ExecutionCursor<'_> {
    #[inline(never)]
    pub(super) fn execute_native(
        &mut self,
        index: usize,
    ) -> Result<NativeContinuation, RegionError> {
        let operation = self
            .frame
            .links
            .as_ref()
            .and_then(|links| links.primitive(index))
            .ok_or_else(|| self.invalid())?;
        let Some(operation) = operation else {
            // Arbitrary Rust bodies and result adapters retain ordinary native
            // invocation. Names and argument representations confer no authority.
            #[cfg(feature = "execution-diagnostics")]
            diagnostics::record(Event::SlowBoundary);
            return Ok(NativeContinuation::Boundary);
        };
        let runtime = self.runtime;
        match &operation.body {
            LinkedPrimitiveBody::StringByteLength(source) => {
                execute_primitive(runtime, &mut self.values, operation.destination, |values| {
                    string_byte_length(
                        runtime.gc(),
                        values
                            .read_location(*source)
                            .ok_or_else(|| invalid_operand(runtime))?,
                    )
                })
            }
            LinkedPrimitiveBody::Vector(vector) => {
                let owner = self.frame.loaded();
                execute_primitive(runtime, &mut self.values, operation.destination, |values| {
                    vector.operation.execute(
                        runtime,
                        owner,
                        vector.function.type_signature()?,
                        |index| {
                            vector
                                .arguments
                                .get(index)
                                .and_then(|location| values.read_location(*location))
                                .ok_or_else(|| invalid_operand(runtime))
                        },
                    )
                })
            }
        }?;
        Ok(NativeContinuation::Complete)
    }
}

// Each sealed body retains its result facts through publication. Merging unrelated
// kernels into a Value result before this step erases the scalar body's facts.
// These private operand closures cannot allocate script objects or call foreign code.
#[inline(never)]
fn execute_primitive(
    runtime: &Runtime,
    values: &mut OperandWindow<'_>,
    destination: Option<Location>,
    kernel: impl FnOnce(&OperandWindow<'_>) -> PrimitiveResult,
) -> Result<(), RegionError> {
    runtime.resources().poll_execution()?;
    let result = kernel(values);
    // Preserve post-body cancellation precedence, including on kernel failure.
    runtime.resources().poll_execution()?;
    let value = result.map_err(RegionError::Runtime)?;
    if let Some(destination) = destination {
        if !runtime.gc().validate_value(&value) {
            return Err(invalid_operand(runtime).into());
        }
        values
            .write_location(destination, value)
            .ok_or_else(|| invalid_operand(runtime))?;
    }
    Ok(())
}

fn invalid_operand(runtime: &Runtime) -> RuntimeError {
    runtime
        .resources()
        .quarantine("invalid execution operand slot")
}
