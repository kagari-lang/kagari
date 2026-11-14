//! Binding-local symbolic enum handles prepared from the exact sealed catalog.
//! These retain declaration metadata only. Allocation/read validation still uses
//! the invoking runtime's generation-pinned layouts, never cached runtime roots.
use crate::identity;
use kagari_common::identity::DefinitionKind;
use kagari_runtime::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        assembly::authoring::PendingBinding,
        binding::{Codec, NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        context::NativeCall,
        types::{TypeRef, VariantRef},
    },
    value::view::ValueView,
};
use std::collections::BTreeMap;

pub(crate) type EnumRequest = (&'static str, &'static [&'static str]);
pub(crate) const OPTION: EnumRequest = ("Option", &["Some", "None"]);
pub(crate) const RESULT: EnumRequest = ("Result", &["Ok", "Err"]);

pub(crate) const CONTROL_FLOW: EnumRequest = ("ControlFlow", &["Continue", "Break"]);
pub(crate) const PARSE_ERROR: EnumRequest = (
    "ParseError",
    &[
        "Empty",
        "InvalidDigit",
        "OutOfRange",
        "InvalidRadix",
        "InvalidSyntax",
    ],
);
pub(crate) const CONVERSION_ERROR: EnumRequest = ("TryFromIntError", &["OutOfRange"]);

pub(crate) type Entry =
    for<'call> fn(&EnumSymbols, &mut NativeCall<'call>) -> NativeResult<ValueView<'call>>;

struct Enumeration {
    declaration: TypeRef,
    variants: BTreeMap<String, VariantRef>,
}

/// Only enums used by one callback family are retained. Preparation is one-shot;
/// calls never enter the standard bundle or a global declaration lookup.
pub(crate) struct EnumSymbols {
    enumerations: BTreeMap<&'static str, Enumeration>,
    option: Option<(VariantRef, VariantRef)>,
}

impl EnumSymbols {
    pub(crate) fn new(catalog: &DeclarationCatalog, names: &[EnumRequest]) -> NativeResult<Self> {
        let mut enumerations = BTreeMap::new();
        for &(name, members) in names {
            let declaration =
                catalog.type_reference(&identity::type_id(name, DefinitionKind::Enum))?;
            let variants = members
                .iter()
                .map(|member| {
                    declaration
                        .variant(member)
                        .map(|handle| ((*member).into(), handle))
                })
                .collect::<NativeResult<_>>()?;
            enumerations.insert(
                name,
                Enumeration {
                    declaration,
                    variants,
                },
            );
        }
        let option = enumerations
            .get("Option")
            .map(|enumeration| -> NativeResult<_> {
                Ok((
                    enumeration.declaration.variant("Some")?,
                    enumeration.declaration.variant("None")?,
                ))
            })
            .transpose()?;
        Ok(Self {
            enumerations,
            option,
        })
    }

    pub(crate) fn option_members(&self) -> NativeResult<(&VariantRef, &VariantRef)> {
        self.option
            .as_ref()
            .map(|(some, none)| (some, none))
            .ok_or_else(|| {
                RuntimeError::metadata_conflict("Option members were not prepared for this binding")
            })
    }

    fn enumeration(&self, name: &str) -> NativeResult<&Enumeration> {
        self.enumerations.get(name).ok_or_else(|| {
            RuntimeError::metadata_conflict("enum was not prepared for this binding")
        })
    }
}

pub(crate) fn pending(
    arguments: usize,
    names: &'static [EnumRequest],
    entry: Entry,
) -> PendingBinding {
    PendingBinding::implementation_with_send(move |view| {
        let symbols = EnumSymbols::new(view.catalog(), names)?;
        Ok(NativeBinding::new(
            vec![Codec::Value; arguments],
            Codec::Value,
            move |cx| entry(&symbols, cx),
        ))
    })
}

pub(crate) fn option<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
    value: Option<ValueView<'call>>,
) -> NativeResult<ValueView<'call>> {
    let (some, none) = symbols.option_members()?;
    cx.allocate_enum(
        &cx.result_type_argument()?,
        if value.is_some() { some } else { none },
        value.as_slice(),
    )
}

pub(crate) fn allocate<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
    ty: &TypeArgument,
    name: &str,
    member: &str,
    fields: &[ValueView<'_>],
) -> NativeResult<ValueView<'call>> {
    let variant = symbols
        .enumeration(name)?
        .variants
        .get(member)
        .ok_or_else(|| RuntimeError::metadata_conflict("unprepared enum variant"))?;
    cx.allocate_enum(ty, variant, fields)
}

/// Validate nominal identity, then publish payload views before invoking the reader.
pub(crate) fn read<'call, R>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
    value: &ValueView<'call>,
    name: &str,
    read: impl FnOnce(&str, &[ValueView<'call>]) -> R,
) -> NativeResult<R> {
    let (layout, count) = cx.enum_shape(value)?;
    let enumeration = &symbols.enumeration(name)?.declaration;
    let expected = enumeration
        .definitions()
        .resolve(enumeration.id())
        .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
    let actual = layout
        .module()
        .definitions()
        .resolve(layout.layout().declaration)
        .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
    if actual.module() != expected.module() || !actual.segments().eq(expected.segments()) {
        return Err(RuntimeError::module_validation("foreign library enum"));
    }
    let member = layout
        .module()
        .definitions()
        .resolve(layout.variant().declaration)
        .ok()
        .and_then(|member| member.segments().last().map(|segment| segment.name))
        .ok_or_else(|| RuntimeError::module_validation("library enum member"))?;
    if count == 0 {
        return Ok(read(member, &[]));
    }
    if count == 1 {
        return Ok(read(member, &[cx.enum_field(value, 0)?]));
    }
    let mut fields = Vec::new();
    fields
        .try_reserve_exact(count)
        .map_err(|_| RuntimeError::resource_limit("library enum fields"))?;
    for index in 0..count {
        fields.push(cx.enum_field(value, index)?);
    }
    Ok(read(member, &fields))
}

/// Return at most one payload protected by the enclosing Native call.
pub(crate) fn read_option<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
    value: &ValueView<'call>,
) -> NativeResult<Option<ValueView<'call>>> {
    read(symbols, cx, value, "Option", |member, fields| {
        match (member, fields) {
            ("None", []) => Ok(None),
            ("Some", [value]) => Ok(Some(*value)),
            _ => Err(RuntimeError::module_validation("library Option member")),
        }
    })?
}
