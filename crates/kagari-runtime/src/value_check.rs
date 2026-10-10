//! Runtime value/type compatibility consumes checked layouts and lexical type facts.
//! Heap storage supplies descriptors; the collector does not resolve type policy.
use crate::{
    frame::types::{bindings::TypeBindings, compatibility::TypeView},
    gc::GcHeap,
    module::LoadedModule,
    numeric,
    value::{EnumTag, Value},
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::representation::semantic_representation;
use kagari_types::ty::Ty;

pub(crate) fn matches_type(
    heap: &GcHeap,
    value: &Value,
    ty: &Ty<DefinitionId>,
    owner: &LoadedModule,
) -> bool {
    matches_view(heap, value, TypeView::new(ty, owner, None))
}

pub(crate) fn matches_type_in(
    heap: &GcHeap,
    value: &Value,
    ty: &Ty<DefinitionId>,
    owner: &LoadedModule,
    environment: Option<&TypeBindings>,
) -> bool {
    matches_view(heap, value, TypeView::new(ty, owner, environment))
}

/// One matcher for raw lexical expressions and admitted closed type facts. Project
/// container children without discarding their closed result or supplying scope.
/// Tuple descent follows the checked type tree; no per-value worklist is needed.
pub(crate) fn matches_view(heap: &GcHeap, value: &Value, view: TypeView<'_>) -> bool {
    if let Ty::Parameter { owner, position } = view.ty {
        return view
            .environment
            .and_then(|environment| environment.argument(owner, *position))
            .is_some_and(|argument| argument.matches_heap(heap, value, view.owner));
    }
    let Some(view) = view.normalized() else {
        return false;
    };
    match (value, view.ty) {
        (value, Ty::Builtin(kind)) => {
            if kind.integer_layout().is_some() {
                numeric::read_integer(*kind, value).is_ok()
            } else {
                value.has_representation(semantic_representation(view.ty))
            }
        }
        (Value::Range(id), Ty::Range(_, _)) => heap
            .range(*id)
            .is_some_and(|range| view.closed().is_some_and(|ty| range.matches(&ty))),
        (Value::Closure(id), Ty::Function { params, result }) => {
            heap.closure_snapshot(*id).is_some_and(|snapshot| {
                snapshot.matches_function(params, result, view.owner, view.environment)
            })
        }
        (Value::Tuple(id), Ty::Tuple(types)) => heap.tuple(*id).is_some_and(|values| {
            values.len() == types.len()
                && values.iter().enumerate().all(|(index, value)| {
                    view.parameter(index)
                        .is_some_and(|view| matches_view(heap, value, view))
                })
        }),
        (Value::Struct(id), Ty::Struct(expected)) => {
            heap.struct_layout(*id).is_some_and(|actual| {
                if view.environment.is_none() {
                    view.owner
                        .find_struct_layout(expected)
                        .is_some_and(|current| actual.matches(&current))
                } else {
                    actual.matches_view(view)
                }
            })
        }
        (Value::Enum(id), Ty::Enum(_)) => heap.enum_view(*id).is_some_and(
            |value| matches!(&value.tag, EnumTag::Declared(actual) if actual.matches_view(view)),
        ),
        (Value::Interface(id), Ty::Trait(_)) => heap
            .interface_snapshot(*id)
            .is_some_and(|actual| actual.matches_type(view.ty, view.owner, view.environment)),
        (Value::GcHandle(id), Ty::NativeObject(_)) => {
            heap.matches_native_type(*id, view.ty, view.owner, view.environment)
        }
        (Value::GcHandle(id), Ty::Iter(_)) => view
            .parameter(0)
            .is_some_and(|element| heap.matches_iter_type(*id, element)),
        (Value::Array(id), Ty::Array(_, _)) => heap.array_contract(*id).is_some_and(|contract| {
            view.parameter(0)
                .is_some_and(|element| contract.matches_view(element))
        }),
        (Value::Map(id), Ty::Map { .. }) => {
            heap.map_contract(*id).is_some_and(|(key, value, _)| {
                view.parameter(0)
                    .is_some_and(|expected| key.matches_view(expected))
                    && view
                        .parameter(1)
                        .is_some_and(|expected| value.matches_view(expected))
            })
        }
        (Value::Set(id), Ty::Set(_, _)) => heap.set_contract(*id).is_some_and(|(contract, _)| {
            view.parameter(0)
                .is_some_and(|element| contract.matches_view(element))
        }),
        (_, Ty::Projection { .. } | Ty::SelfType(_)) => view
            .closed()
            .is_some_and(|ty| matches_type(heap, value, &ty, view.owner)),
        _ => false,
    }
}
