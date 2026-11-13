//! Bounded detached Host traces never retain managed sources or control loans.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind, fault::EngineFault},
    execution::roots::{ExecutionRoots, SessionId},
    range::RangeValue,
    session::{HostCallTrace, SessionState, TraceValue},
    value::{
        aggregates::tuples::TupleObject, carriers::ScopedValue, kind::ObjectKind,
        strings::StringObject, view::ValueView,
    },
};
use kagari_abi::representation::ValueType;
use kagari_gc::{access::Scope, error::GcError};

impl TraceValue {
    pub(in crate::session) fn capture<'gc>(
        cx: &Scope<'gc>,
        value: ScopedValue<'gc>,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Self, GcError> {
        if *remaining == 0 {
            return Ok(Self::Opaque("trace value budget".into()));
        }
        *remaining -= 1;
        if let Some((kind, bits)) = value.scalar_payload() {
            return Ok(match kind {
                ValueType::Unit => Self::Unit,
                ValueType::Bool => Self::Bool(bits != 0),
                ValueType::Char => {
                    Self::Char(char::from_u32(bits as u32).ok_or(GcError::Representation)?)
                }
                ValueType::I32 => Self::I32(bits as i32),
                ValueType::I64 => Self::I64(bits as i64),
                ValueType::U64 => Self::U64(bits),
                ValueType::F32 => Self::F32Bits(bits as u32),
                ValueType::F64 => Self::F64Bits(bits),
                _ => return Err(GcError::Representation),
            });
        }
        let Some(kind) = value.kind() else {
            return Err(GcError::Representation);
        };
        if value.as_runtime_ephemeral().is_some() {
            return Ok(Self::Opaque("ephemeral".into()));
        }
        let object = value.object().ok_or(GcError::Representation)?;
        Ok(match kind {
            ObjectKind::String => {
                let object = cx.downcast::<StringObject>(object)?;
                cx.read(object, |value| Self::Str {
                    prefix: value.as_str().chars().take(256).collect(),
                    truncated: value.as_str().chars().nth(256).is_some(),
                })
            }
            ObjectKind::Tuple if depth < 4 => {
                let object = cx.downcast::<TupleObject>(object)?;
                let count = cx.read(object, TupleObject::len);
                let mut elements = Vec::new();
                elements.try_reserve_exact(count.min(16))?;
                for index in 0..count.min(16) {
                    let value = TupleObject::get(cx, object, index)?;
                    elements.push(Self::capture(cx, value, depth + 1, remaining)?);
                }
                Self::Tuple {
                    elements,
                    truncated: count > 16,
                }
            }
            ObjectKind::Tuple => Self::Opaque("tuple depth limit".into()),
            ObjectKind::HostRoot => Self::Opaque("host root".into()),
            ObjectKind::HostPath => Self::Opaque("host path".into()),
            ObjectKind::Ephemeral => Self::Opaque("ephemeral".into()),
            ObjectKind::Range => {
                let object = cx.downcast::<RangeValue>(object)?;
                cx.read(object, |range| Self::Opaque(format!("{range:?}")))
            }
            kind => {
                let label = match kind {
                    ObjectKind::Array => "array",
                    ObjectKind::Enum => "enum",
                    ObjectKind::Struct => "struct",
                    ObjectKind::Native => "gc",
                    ObjectKind::Interface => "interface",
                    ObjectKind::Closure => "closure",
                    ObjectKind::Cell => "cell",
                    _ => return Err(GcError::Representation),
                };
                Self::Opaque(format!("{label}:{object:p}"))
            }
        })
    }
}

impl SessionState {
    fn admits_host_trace(&self) -> bool {
        if !self.options.record_host_calls {
            return false;
        }
        if self.host_calls.borrow().len() >= 10_000 {
            self.dropped_host_calls
                .set(self.dropped_host_calls.get().saturating_add(1));
            return false;
        }
        true
    }

    pub(crate) fn begin_host_call(&self, trace: HostCallTrace) -> usize {
        let mut calls = self.host_calls.borrow_mut();
        // Runtime admitted the bounded slot before closed capture. No callback
        // can append between that check and this detached-data publication.
        assert!(calls.len() < 10_000);
        let index = calls.len();
        calls.push(trace);
        index
    }

    pub(crate) fn finish_host_call(
        &self,
        index: usize,
        result: Result<TraceValue, RuntimeErrorKind>,
    ) -> bool {
        if let Some(call) = self.host_calls.borrow_mut().get_mut(index) {
            call.outcome = Some(result);
            true
        } else {
            false
        }
    }
}

impl Runtime {
    pub(crate) fn begin_host_call_trace(
        &mut self,
        symbol: impl FnOnce() -> Result<String, RuntimeError>,
        arguments: &[ValueView<'_>],
    ) -> Result<Option<(SessionId, usize)>, RuntimeError> {
        let Some(id) = self
            .resources
            .active_session()
            .map(|session| session.logical_root.get())
        else {
            return Ok(None);
        };
        let admitted = self
            .resources
            .sessions
            .get(id)
            .ok_or_else(|| {
                self.resources.quarantine(EngineFault::HostInvariant(
                    "Host trace lost its root session",
                ))
            })?
            .admits_host_trace();
        if !admitted {
            return Ok(None);
        }
        let symbol = symbol()?;
        let result = self.memory.scope(&self.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let mut remaining = 128;
                let mut values = Vec::new();
                values.try_reserve_exact(arguments.len().min(32))?;
                for value in arguments.iter().take(32) {
                    let value = value.get(slots)?;
                    values.push(TraceValue::capture(
                        slots.scope(),
                        value,
                        0,
                        &mut remaining,
                    )?);
                }
                Ok(HostCallTrace {
                    symbol: symbol.chars().take(256).collect(),
                    symbol_truncated: symbol.chars().nth(256).is_some(),
                    arguments: values,
                    omitted_arguments: arguments.len().saturating_sub(32),
                    outcome: None,
                })
            })
        });
        let trace = result.map_err(|error| self.managed_memory_error(error))?;
        let session = self.resources.sessions.get(id).ok_or_else(|| {
            self.resources.quarantine(EngineFault::HostInvariant(
                "Host trace lost its root session",
            ))
        })?;
        Ok(Some((id, session.begin_host_call(trace))))
    }

    pub(crate) fn finish_host_call_trace(
        &mut self,
        trace: Option<(SessionId, usize)>,
        result: Result<&ValueView<'_>, &RuntimeError>,
    ) -> Result<(), RuntimeError> {
        let Some((id, index)) = trace else {
            return Ok(());
        };
        let outcome = match result {
            Err(error) => Err(error.kind()),
            Ok(value) => {
                let result = self.memory.scope(&self.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| {
                        let value = value.get(slots)?;
                        TraceValue::capture(slots.scope(), value, 0, &mut 128)
                    })
                });
                Ok(result.map_err(|error| self.managed_memory_error(error))?)
            }
        };
        let session = self.resources.sessions.get(id).ok_or_else(|| {
            self.resources.quarantine(EngineFault::HostInvariant(
                "Host trace lost its root session",
            ))
        })?;
        let recorded = session.finish_host_call(index, outcome);
        drop(session);
        if !recorded {
            return Err(self.resources.quarantine(EngineFault::HostInvariant(
                "Host trace lost its admitted record",
            )));
        }
        Ok(())
    }
}
