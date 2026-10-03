//! A library-owned lazy iterator: retained state only between synchronous next calls.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        callable::{CallableHandle, StoredCallable},
        context::CallContext,
        cursor::NativeCursor,
        declarations::FunctionDecl,
        language::LanguageContracts,
        storage::{NativePayload, NativeStorage},
        types::Type,
        views::{SequenceHandle, ValueHandle},
    },
    value::{EnumTag, Value},
};
use std::{cell::Cell, rc::Rc};

#[derive(Debug)]
struct Mapped {
    source: NativeCursor,
    mapper: StoredCallable,
    active: Rc<Cell<bool>>,
}

impl NativePayload for Mapped {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        self.mapper.trace(visit);
    }

    fn iteration_sources<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        self.source.trace(visit);
    }

    fn units(&self) -> usize {
        2
    }
}

struct Active(Rc<Cell<bool>>);

impl Drop for Active {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

pub(super) fn register(
    module: &mut ModuleBuilder,
    language: &LanguageContracts,
) -> NativeResult<()> {
    let mut declaration = module.define_type("MapIterator");
    declaration.type_parameter("T")?;
    declaration.type_parameter("U")?;
    declaration.native_storage(NativeStorage::payload::<Mapped>())?;
    let iterator = declaration.finish()?;
    module.implement(iterator.clone(), |group| {
        let item = group.parameter("U")?.ty();
        group.trait_impl(language.iterator().apply([]), |methods| {
            methods.associated_type("Item", item)?;
            methods.bind("next", next)
        })
    })?;
    let map = module.define_function(FunctionDecl::new("map").documentation(
        "Lazily transform an ArrayList. Each next consumes one source item and invokes the mapper synchronously. Copies share cursor progress; completed effects survive failure."
    ))?;
    module.function(&map, |function| {
        let input = function.type_parameter("T")?.ty();
        let output = function.type_parameter("U")?.ty();
        function.parameter("values", language.array_list(input.clone()));
        function.parameter("mapper", Type::function([input.clone()], output.clone()));
        function.returns(iterator.apply([input, output])?);
        Ok(())
    })?;
    module.bind(
        map,
        |cx: &mut CallContext<'_>,
         _source: SequenceHandle<'_>,
         mapper: CallableHandle<'_>|
         -> NativeResult<Value> {
            let source = cx.sequence_cursor(0)?;
            cx.allocate_result_payload(Mapped {
                source,
                mapper: mapper.store(),
                active: Rc::new(Cell::new(false)),
            })
        },
    )
}

fn next(cx: &mut CallContext<'_>, receiver: ValueHandle<'_>) -> NativeResult<Value> {
    let (source, mapper, active) = receiver.with_payload::<Mapped, _>(|mapped| {
        Ok((
            mapped.source.clone(),
            mapped.mapper.clone(),
            mapped.active.clone(),
        ))
    })?;
    if active.replace(true) {
        return Err(RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "recursive next on the same lazy iterator",
        ));
    }
    let _active = Active(active);
    let output = match source.next(cx)? {
        Some(item) => Some(mapper.call_values(cx, &[item])?),
        None => None,
    };
    let tag = if output.is_some() {
        EnumTag::OptionSome
    } else {
        EnumTag::OptionNone
    };
    cx.heap()
        .alloc_enum(tag, output.into_iter().collect())
        .map(Value::Enum)
}
