//! Stable Rust sorting, with one atomic publication and synchronous comparisons.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        callable::CallableHandle,
        context::CallContext,
        declarations::{CallableRequirement, FunctionDecl},
        language::LanguageContracts,
        sequence_edit::SequenceEdit,
        types::Type,
        views::SequenceMutHandle,
    },
    value::{EnumTag, Value},
    value_semantics,
};
use kagari_abi::{scalar::BuiltinType, standard::RuntimePrimitive, types::AbiType};
use std::{
    cmp::Ordering,
    panic::{AssertUnwindSafe, catch_unwind},
};

pub(super) fn register(
    module: &mut ModuleBuilder,
    language: &LanguageContracts,
) -> NativeResult<()> {
    let sort = module.define_function(FunctionDecl::new("sort").documentation("Stably sort values using T: Ord. Shared aliases observe the new order; comparator failure preserves the original slots."))?;
    let selected = module.function(&sort, |function| {
        let item = function.type_parameter("T")?.ty();
        function.parameter("values", language.array_list(item.clone()));
        function.bound(item.clone(), language.ord().apply([]));
        Ok(function.requires(CallableRequirement::method(
            item,
            language.ord().method("cmp")?,
        )))
    })?;
    module.bind(
        sort,
        move |cx: &mut CallContext<'_>, mut values: SequenceMutHandle<'_>| -> NativeResult<()> {
            let compare = cx.selected(selected)?;
            cx.poll()?;
            if compare.primitive == Some(RuntimePrimitive::ValueCmp)
                && primitive_sort(&mut values, &compare.params[0])?
            {
                return Ok(());
            }
            values.edit(|mut working| {
                if compare.primitive == Some(RuntimePrimitive::ValueCmp) {
                    let order = stable_order(&working, |left, right| {
                        value_semantics::builtin_order(cx.heap(), &left, &right)?.ok_or_else(|| {
                            RuntimeError::module_validation("total ordering returned unordered")
                        })
                    })?;
                    return working.reorder(order);
                }
                let order = stable_order(&working, |left, right| {
                    let value = cx.call_values(compare, &[left, right])?;
                    decode_ordering(cx, value)
                })?;
                working.reorder(order)
            })
        },
    )?;
    let sort_by = module.define_function(FunctionDecl::new("sort_by").documentation("Stably sort values using compare. Calls run synchronously; failure stops further callbacks and preserves the original slots."))?;
    module.function(&sort_by, |function| {
        let item = function.type_parameter("T")?.ty();
        function.parameter("values", language.array_list(item.clone()));
        function.parameter(
            "compare",
            Type::function([item.clone(), item], language.ordering()),
        );
        Ok(())
    })?;
    module.bind(
        sort_by,
        |cx: &mut CallContext<'_>,
         mut values: SequenceMutHandle<'_>,
         compare: CallableHandle<'_>|
         -> NativeResult<()> {
            values.edit(|mut working| {
                let order = stable_order(&working, |left, right| {
                    let value = compare.call_values(cx, &[left, right])?;
                    decode_ordering(cx, value)
                })?;
                working.reorder(order)
            })
        },
    )
}

fn primitive_sort(values: &mut SequenceMutHandle<'_>, ty: &AbiType) -> NativeResult<bool> {
    macro_rules! scalar {
        ($($variant:ident:$rust:ty),+) => {
            match ty {
                $(AbiType::Builtin(BuiltinType::$variant) => values.with_slice_mut::<$rust, _>(|slice| { slice.sort(); Ok(()) })?,)+
                _ => return Ok(false),
            }
        };
    }
    scalar!(Unit:(), Bool:bool, I8:i8, I16:i16, I32:i32, I64:i64, ISize:isize,
        U8:u8, U16:u16, U32:u32, U64:u64, USize:usize);
    Ok(true)
}

fn decode_ordering(cx: &CallContext<'_>, value: Value) -> NativeResult<Ordering> {
    let Value::Enum(id) = value else {
        return Err(invalid_ordering());
    };
    match cx
        .heap()
        .enum_snapshot(id)
        .ok_or_else(invalid_ordering)?
        .tag
    {
        EnumTag::OrderingLess => Ok(Ordering::Less),
        EnumTag::OrderingEqual => Ok(Ordering::Equal),
        EnumTag::OrderingGreater => Ok(Ordering::Greater),
        _ => Err(invalid_ordering()),
    }
}
fn invalid_ordering() -> RuntimeError {
    RuntimeError::module_validation("comparator returned invalid Ordering")
}

fn stable_order(
    values: &SequenceEdit<'_>,
    mut compare: impl FnMut(Value, Value) -> NativeResult<Ordering>,
) -> NativeResult<Vec<usize>> {
    let mut order = Vec::new();
    order
        .try_reserve_exact(values.len())
        .map_err(|_| RuntimeError::resource_limit("sort working indices"))?;
    order.extend(0..values.len());
    let mut failure = None;
    // Rust's infallible sort may finish bookkeeping after failure. No user
    // comparator is invoked again, and its partial permutation is never committed.
    // A comparator that violates total ordering may make std's sort panic.
    let sorted = catch_unwind(AssertUnwindSafe(|| {
        order.sort_by(|&left, &right| {
            if failure.is_some() {
                return Ordering::Equal;
            }
            match values
                .get(left)
                .and_then(|left| values.get(right).and_then(|right| compare(left, right)))
            {
                Ok(ordering) => ordering,
                Err(error) => {
                    failure = Some(error);
                    Ordering::Equal
                }
            }
        })
    }));
    if let Some(error) = failure {
        return Err(error);
    }
    if sorted.is_err() {
        return Err(RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "sort comparator does not define a consistent order",
        ));
    }
    Ok(order)
}
