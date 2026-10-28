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
        context::CallContext,
        conversion::{IntoKagari, context::ConversionContext},
        types::{TypeRef, VariantRef},
    },
    value::{EnumTag, Value},
};
use std::{cmp::Ordering, collections::BTreeMap};

pub(crate) type EnumRequest = (&'static str, &'static [&'static str]);
pub(crate) const OPTION: EnumRequest = ("Option", &["Some", "None"]);
pub(crate) const RESULT: EnumRequest = ("Result", &["Ok", "Err"]);
pub(crate) const ORDERING: EnumRequest = ("Ordering", &["Less", "Equal", "Greater"]);
pub(crate) const CONTROL_FLOW: EnumRequest = ("ControlFlow", &["Continue", "Break"]);
pub(crate) const BOUND: EnumRequest = ("Bound", &["Included", "Excluded", "Unbounded"]);
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

pub(crate) type Entry = for<'call> fn(&EnumSymbols, &mut CallContext<'call>) -> NativeResult<Value>;

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

    pub(crate) fn option_codec(&self) -> NativeResult<Codec> {
        Ok(Codec::NominalEnum {
            declaration: self.enumeration("Option")?.declaration.clone(),
            arity: 1,
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

/// Bind a fixed Option result, rejecting foreign declarations or wrong arity at
/// registration/linking before the callback can run. Child mappings remain dynamic.
pub(crate) fn pending_option(arguments: usize, entry: Entry) -> PendingBinding {
    PendingBinding::implementation_with_send(move |view| {
        let symbols = EnumSymbols::new(view.catalog(), &[OPTION])?;
        Ok(NativeBinding::new(
            vec![Codec::Value; arguments],
            symbols.option_codec()?,
            move |cx| entry(&symbols, cx),
        ))
    })
}

pub(crate) fn option(
    symbols: &EnumSymbols,
    cx: &CallContext<'_>,
    value: Option<Value>,
) -> NativeResult<Value> {
    let (some, none) = symbols.option_members()?;
    cx.allocate_enum(
        &cx.result_type_argument()?,
        if value.is_some() { some } else { none },
        value.into_iter().collect(),
    )
}

/// Publish a native Option result whose Rust mapping was checked before effects.
/// Child conversion retains reference items (including a Map entry tuple) until
/// allocation finishes. The outer enum uses the terminal native-result handoff;
/// callers must not allocate or reenter after this returns an unrooted Value.
pub(crate) fn converted_option<T: IntoKagari>(
    symbols: &EnumSymbols,
    cx: &CallContext<'_>,
    conversion: &mut ConversionContext<'_>,
    result_type: &TypeArgument,
    value: Option<T>,
) -> NativeResult<Value> {
    let value = value
        .map(|value| {
            let element = conversion.parameter(result_type, 0)?;
            conversion.encode_value(&element, value)
        })
        .transpose()?;
    let (some, none) = symbols.option_members()?;
    cx.allocate_enum(
        result_type,
        if value.is_some() { some } else { none },
        value.into_iter().collect(),
    )
}

pub(crate) fn allocate(
    symbols: &EnumSymbols,
    cx: &CallContext<'_>,
    ty: &TypeArgument,
    name: &str,
    member: &str,
    fields: Vec<Value>,
) -> NativeResult<Value> {
    let variant = symbols
        .enumeration(name)?
        .variants
        .get(member)
        .ok_or_else(|| RuntimeError::metadata_conflict("unprepared enum variant"))?;
    cx.allocate_enum(ty, variant, fields)
}

/// The reader must not allocate on the script heap or invoke callbacks.
pub(crate) fn read<R>(
    symbols: &EnumSymbols,
    cx: &CallContext<'_>,
    value: &Value,
    name: &str,
    read: impl FnOnce(&str, &[Value]) -> R,
) -> NativeResult<R> {
    let Value::Enum(id) = value else {
        return Err(RuntimeError::module_validation("library enum value"));
    };
    let view = cx
        .heap()
        .enum_view(*id)
        .ok_or_else(|| RuntimeError::module_validation("library enum handle"))?;
    let EnumTag::Declared(layout) = &view.tag;
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
    Ok(read(member, &view.fields))
}

/// Copy at most one payload; its caller roots it before allocation or reentry.
pub(crate) fn read_option(
    symbols: &EnumSymbols,
    cx: &CallContext<'_>,
    value: &Value,
) -> NativeResult<Option<Value>> {
    read(symbols, cx, value, "Option", |member, fields| {
        match (member, fields) {
            ("None", []) => Ok(None),
            ("Some", [value]) => Ok(Some(*value)),
            _ => Err(RuntimeError::module_validation("library Option member")),
        }
    })?
}

pub(crate) fn ordering(
    symbols: &EnumSymbols,
    cx: &CallContext<'_>,
    value: &Value,
) -> NativeResult<Ordering> {
    read(symbols, cx, value, "Ordering", |member, fields| {
        match (member, fields) {
            ("Less", []) => Ok(Ordering::Less),
            ("Equal", []) => Ok(Ordering::Equal),
            ("Greater", []) => Ok(Ordering::Greater),
            _ => Err(RuntimeError::module_validation("library Ordering member")),
        }
    })?
}
