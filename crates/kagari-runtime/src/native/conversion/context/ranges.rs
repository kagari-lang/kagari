//! Range operations consume protected scalars and immutable declared enum facts.
use crate::{
    error::{RuntimeError, trap::TrapReason},
    frame::types::arguments::TypeArgument,
    module::EnumVariantRef,
    native::{binding::NativeResult, conversion::context::ConversionContext},
    range::RangeValue,
    value::{
        aggregates::enumerations::EnumObject, carriers::ScopedValue, kind::ObjectKind,
        scalar::elements::Primitive, view::ValueView,
    },
};
use kagari_gc::error::GcError;
use kagari_types::{language::binding, scalar::BuiltinType, ty::Ty};
use std::ops::Bound;

#[derive(Clone, Copy)]
enum IndexBoundMember {
    Unbounded,
    Included,
    Excluded,
}

impl<'runtime> ConversionContext<'runtime> {
    /// Construct an immutable range. Endpoints are copied once from checked
    /// sources; integer/domain and endpoint-presence validation precede allocation.
    pub fn allocate_range(
        &mut self,
        applied: &TypeArgument,
        start: Option<&ValueView<'_>>,
        end: Option<&ValueView<'_>>,
    ) -> NativeResult<ValueView<'runtime>> {
        self.poll()?;
        applied.validate(self.runtime)?;
        let execution = self.execution_id()?;
        self.with_values(|slots| {
            let start = start.map(|value| value.get(slots)).transpose()?;
            let end = end.map(|value| value.get(slots)).transpose()?;
            let range = match RangeValue::new(applied.ty(), start, end) {
                Ok(range) => range,
                Err(error) => return Ok(Err(RuntimeError::from(error))),
            };
            let range = range.alloc(slots.scope())?;
            let value = ScopedValue::managed(range);
            let location = slots.push_local(execution, value)?;
            ValueView::protected(value, Some(location)).map(Ok)
        })?
    }

    /// Produce the declared Bound variant with its original exact scalar type.
    /// No range payload loan survives enum construction or origin capture.
    pub fn range_bound(
        &mut self,
        value: &ValueView<'_>,
        range: &TypeArgument,
        bound: &TypeArgument,
        upper: bool,
    ) -> NativeResult<ValueView<'runtime>> {
        self.poll()?;
        range.validate(self.runtime)?;
        bound.validate(self.runtime)?;
        let selected = self.with_values(|slots| {
            let value = value.get(slots)?;
            if value.kind() != Some(ObjectKind::Range) {
                return Ok(Err(invalid()));
            }
            let object = slots
                .scope()
                .downcast::<RangeValue>(value.object().ok_or(GcError::Representation)?)?;
            let range_value = slots.scope().read(object, |range| *range);
            let selected =
                match range_value.bound(bound.definitions(), range.ty(), bound.ty(), upper) {
                    Ok(selected) => selected,
                    Err(error) => return Ok(Err(RuntimeError::from(error))),
                };
            // All range endpoints are scalar; no managed source publication is
            // required, and the immutable descriptor is no longer borrowed.
            let selected = match selected {
                Bound::Included(value) => Bound::Included(ValueView::protected(value, None)?),
                Bound::Excluded(value) => Bound::Excluded(ValueView::protected(value, None)?),
                Bound::Unbounded => Bound::Unbounded,
            };
            Ok(Ok(selected))
        })??;
        match selected {
            Bound::Included(value) => self.allocate_enum_member(bound, "Included", &[value]),
            Bound::Excluded(value) => self.allocate_enum_member(bound, "Excluded", &[value]),
            Bound::Unbounded => self.allocate_enum_member(bound, "Unbounded", &[]),
        }
    }

    /// Decode a declared Bound<u64> without narrowing its logical endpoint.
    /// The consuming memory operation owns machine conversion and bounds policy.
    /// Unit variants still require the complete applied element identity.
    pub fn index_bound(&mut self, value: &ValueView<'_>) -> NativeResult<Bound<u64>> {
        self.with_values(|slots| {
            let value = value.get(slots)?;
            if value.kind() != Some(ObjectKind::Enum) {
                return Ok(Err(invalid()));
            }
            let object = slots
                .scope()
                .downcast::<EnumObject>(value.object().ok_or(GcError::Representation)?)?;
            let member = slots.scope().read(object, |object| {
                let layout = object.layout();
                if !is_index_bound(layout) {
                    return None;
                }
                match (
                    layout
                        .module()
                        .definition_name(layout.variant().declaration),
                    object.len(),
                ) {
                    (Some("Unbounded"), 0) => Some(IndexBoundMember::Unbounded),
                    (Some("Included"), 1) => Some(IndexBoundMember::Included),
                    (Some("Excluded"), 1) => Some(IndexBoundMember::Excluded),
                    _ => None,
                }
            });
            let mut endpoint = || {
                let value = EnumObject::get(slots.scope(), object, 0)?;
                Ok::<_, GcError>(u64::decode(value).map_err(|_| invalid()))
            };
            match member {
                Some(IndexBoundMember::Unbounded) => Ok(Ok(Bound::Unbounded)),
                Some(IndexBoundMember::Included) => {
                    endpoint().map(|value| value.map(Bound::Included))
                }
                Some(IndexBoundMember::Excluded) => {
                    endpoint().map(|value| value.map(Bound::Excluded))
                }
                None => Ok(Err(invalid())),
            }
        })?
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::from(TrapReason::InvalidRangeValue)
}

fn is_index_bound(layout: &EnumVariantRef) -> bool {
    if !binding::matches(
        &layout.layout().declaration,
        &binding::bound_declaration(),
        layout.module().definitions(),
    ) {
        return false;
    }
    if layout.layout().arguments.len() != 1 {
        return false;
    }
    // Unit variants carry no payload witness: retain exact applied identity.
    // The owner projects existing lexical facts without a synthetic Ty/Vec.
    layout
        .argument_type(0)
        .and_then(|element| element.closed())
        .is_some_and(|element| element.as_ref() == &Ty::Builtin(BuiltinType::U64))
}
