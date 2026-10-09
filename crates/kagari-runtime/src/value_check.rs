//! Runtime value/type compatibility consumes checked layouts and selected scopes.
//! Heap storage supplies descriptors; the collector does not resolve type policy.
use crate::{
    frame::types::bindings::TypeBindings,
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
    if let Ty::Builtin(kind) = ty {
        return if kind.integer_layout().is_some() {
            numeric::read_integer(*kind, value).is_ok()
        } else {
            value.has_representation(semantic_representation(ty))
        };
    }
    let mut pending = vec![(*value, ty)];
    while let Some((value, ty)) = pending.pop() {
        let matches = match (value, ty) {
            (value, Ty::Builtin(_)) => matches_type(heap, &value, ty, owner),
            (Value::Range(value), Ty::Range(_, _)) => heap.range(value).is_some_and(|range| range.matches(ty)),
            (Value::Closure(id), Ty::Function { params, result }) => heap
                .closure_snapshot(id)
                .is_some_and(|snapshot| snapshot.matches_function(params, result, owner, None)),
            (Value::Tuple(id), Ty::Tuple(types)) => {
                let Some(values) = heap.tuple(id) else { return false; };
                if values.len() != types.len() { return false; }
                pending.extend(values.iter().copied().zip(types));
                true
            }
            (Value::Struct(id), Ty::Struct(expected)) => heap.struct_layout(id).is_some_and(|layout| {
                owner.find_struct_layout(expected).is_some_and(|current| layout.matches(&current))
            }),
            (Value::Enum(id), Ty::Enum(_)) => heap.enum_snapshot(id).is_some_and(|value| {
                matches!(value.tag, EnumTag::Declared(layout) if layout.matches_type(ty, owner, None))
            }),
            (Value::Interface(id), Ty::Trait(_)) => heap.interface_snapshot(id)
                .is_some_and(|value| value.matches_type(ty, owner, None)),
            (Value::GcHandle(id), Ty::NativeObject(_)) => heap.matches_native_type(id, ty, owner, None),
            (Value::GcHandle(id), Ty::Iter(element)) => heap.matches_iter_type(id, element, owner, None),
            (Value::Array(id), Ty::Array(element, _)) => heap.array_contract(id)
                .is_some_and(|contract| contract.matches(element, owner)),
            (Value::Map(id), Ty::Map { key, value, .. }) => heap.map_contract(id)
                .is_some_and(|(a, b, _)| a.matches(key, owner) && b.matches(value, owner)),
            (Value::Set(id), Ty::Set(element, _)) => heap.set_contract(id)
                .is_some_and(|(contract, _)| contract.matches(element, owner)),
            _ => false,
        };
        if !matches {
            return false;
        }
    }
    true
}

pub(crate) fn matches_type_in(
    heap: &GcHeap,
    value: &Value,
    ty: &Ty<DefinitionId>,
    owner: &LoadedModule,
    environment: Option<&TypeBindings>,
) -> bool {
    if ty.is_concrete() {
        return matches_type(heap, value, ty, owner);
    }
    if let Ty::Parameter {
        owner: binder,
        position,
    } = ty
    {
        return environment
            .and_then(|environment| environment.argument(binder, *position))
            .is_some_and(|argument| argument.matches_heap(heap, value, owner));
    }
    if let (Value::Tuple(id), Ty::Tuple(types)) = (value, ty) {
        let Some(values) = heap.tuple(*id) else {
            return false;
        };
        return values.len() == types.len()
            && values
                .iter()
                .zip(types)
                .all(|(value, ty)| matches_type_in(heap, value, ty, owner, environment));
    }
    if let (Value::Closure(id), Ty::Function { params, result }) = (value, ty) {
        return heap
            .closure_snapshot(*id)
            .is_some_and(|closure| closure.matches_function(params, result, owner, environment));
    }
    if let (Value::Interface(id), Ty::Trait(_)) = (value, ty) {
        return heap
            .interface_snapshot(*id)
            .is_some_and(|actual| actual.matches_type(ty, owner, environment));
    }
    if let (Value::GcHandle(id), Ty::Iter(element)) = (value, ty) {
        return heap.matches_iter_type(*id, element, owner, environment);
    }
    if let (Value::GcHandle(id), Ty::NativeObject(_)) = (value, ty) {
        return heap.matches_native_type(*id, ty, owner, environment);
    }
    if let (Value::Map(id), Ty::Map { key, value, .. }) = (value, ty) {
        return heap.map_contract(*id).is_some_and(|(a, b, _)| {
            a.matches_scoped(key, owner, environment) && b.matches_scoped(value, owner, environment)
        });
    }
    if let (Value::Set(id), Ty::Set(element, _)) = (value, ty) {
        return heap
            .set_contract(*id)
            .is_some_and(|(contract, _)| contract.matches_scoped(element, owner, environment));
    }
    if let (Value::Array(id), Ty::Array(element, _)) = (value, ty) {
        return heap
            .array_contract(*id)
            .is_some_and(|contract| contract.matches_scoped(element, owner, environment));
    }
    if let (Value::Struct(id), Ty::Struct(_)) = (value, ty) {
        return heap
            .struct_layout(*id)
            .is_some_and(|actual| actual.matches_type(ty, owner, environment));
    }
    if let (Value::Enum(id), Ty::Enum(_)) = (value, ty) {
        return heap.enum_snapshot(*id).is_some_and(|snapshot| matches!(snapshot.tag, EnumTag::Declared(actual) if actual.matches_type(ty, owner, environment)));
    }
    match environment {
        Some(environment) => environment
            .resolve(ty)
            .is_ok_and(|ty| matches_type(heap, value, &ty, owner)),
        None => matches_type(heap, value, ty, owner),
    }
}
