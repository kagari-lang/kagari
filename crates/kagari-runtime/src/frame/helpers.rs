//! Slow Runtime calls borrow caller coordinates and publish one direct result.
use crate::{
    Runtime, builtin,
    error::{RuntimeError, fault::EngineFault},
    execution_metadata::managed::sources::MetadataSource,
    frame::FrameStack,
    module::managed::source::RevisionSource,
    native::conversion::{arguments::ArgumentValues, context::ConversionContext},
    reflection::{self, Operation},
    value::place::ResultPlace,
};
use kagari_bytecode::instruction::{CallTarget, RuntimeHelper, Slot};

impl FrameStack {
    pub(crate) fn invoke_runtime_call(
        &self,
        runtime: &mut Runtime,
        target: &CallTarget,
        args: &[Slot],
        destination: Option<Slot>,
    ) -> Result<(), RuntimeError> {
        self.validate_top(runtime)?;
        let prepared = (|| {
            let frame = self.current(runtime)?;
            let mut locations = Vec::new();
            locations
                .try_reserve_exact(args.len())
                .map_err(|_| RuntimeError::resource_limit("Runtime call coordinates"))?;
            for (index, &slot) in args.iter().enumerate() {
                if index.is_multiple_of(1024) {
                    runtime.resources.poll_closed()?;
                }
                locations.push(frame.slot_location(&runtime.resources, slot, true)?);
            }
            Ok::<_, RuntimeError>((
                frame.loaded().clone(),
                frame.admitted_environment().cloned(),
                frame.slots(),
                locations,
                destination
                    .map(|slot| frame.slot_location(&runtime.resources, slot, false))
                    .transpose()?,
            ))
        })();
        let (owner, environment, frame, locations, destination) =
            prepared.map_err(|error| runtime.resources.admit_execution_error(error))?;
        let arguments = ArgumentValues::prepared(
            frame.banks.execution,
            frame.banks.managed,
            frame.banks.scalars,
            &locations,
        );
        let place = ResultPlace::frame(&frame, destination.as_ref());
        let published = match target {
            CallTarget::RuntimePrimitive(primitive) => {
                let mut conversion = ConversionContext::for_selected(
                    runtime,
                    &owner,
                    self.session,
                    RevisionSource::Execution(frame.body),
                    environment
                        .as_ref()
                        .map(MetadataSource::AdmittedEnvironment),
                )?;
                builtin::invoke_in(&mut conversion, *primitive, arguments, place)
                    .map_err(RuntimeError::from)?
            }
            CallTarget::RuntimeHelper(helper) => {
                let operation = match helper {
                    RuntimeHelper::ReflectTypeOf => Operation::TypeOf,
                    RuntimeHelper::ReflectGetField(name) => Operation::GetField(name),
                    RuntimeHelper::ReflectSetField(name) => Operation::SetField(name),
                    RuntimeHelper::ReflectSetIndex => Operation::SetIndex,
                    RuntimeHelper::DynamicCall => {
                        runtime.resources.ensure_execution_allowed()?;
                        return Err(RuntimeError::from(EngineFault::HostInvariant(
                            "runtime_helper_dynamic_call",
                        )));
                    }
                };
                reflection::invoke(runtime, operation, arguments, place)?
            }
            _ => {
                return Err(runtime
                    .resources
                    .quarantine(EngineFault::PreparedCallReachedCanonicalDispatch));
            }
        };
        published.admit_frame(&frame, destination.as_ref())?;
        self.complete_source_write(runtime);
        Ok(())
    }
}
