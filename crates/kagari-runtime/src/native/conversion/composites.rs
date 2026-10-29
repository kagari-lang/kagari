use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        schema::TypeRegistration,
        types::Type,
        value_handle::ScriptValue,
    },
    value::{EnumTag, Value},
};
use kagari_common::identity::table::{DefinitionId, DefinitionTable};
use kagari_types::{language::binding, ty::Ty};

impl<T: KagariType> KagariType for Option<T> {
    fn register_type(registration: &mut TypeRegistration<'_>) -> NativeResult<Type>
    where
        Self: 'static,
    {
        registration.dependency::<T>()?;
        Self::kagari_type(registration.catalog())
    }

    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog
            .type_reference(&binding::option_declaration())?
            .apply([T::kagari_type(catalog)?])
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        check_enum_type(cx, expected, binding::option_declaration_id, 1)?;
        cx.check_type::<T>(&cx.parameter(expected, 0)?)
    }
}

impl<T: IntoKagari> IntoKagari for Option<T> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<ScriptValue> {
        let (member, fields) = match self {
            None => ("None", vec![]),
            Some(value) => {
                let element = cx.parameter(expected, 0)?;
                ("Some", vec![cx.encode_transport(&element, value)?])
            }
        };
        let value = cx
            .runtime()
            .make_enum_member(cx.owner(), expected, member, fields)?;
        cx.retain_converted(expected, value)
    }
}

impl<T: FromKagari> FromKagari for Option<T> {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ScriptValue,
    ) -> NativeResult<Self> {
        let raw = value.call_argument(cx.runtime(), expected, cx.owner())?;
        let value = &raw;
        match enum_payload(cx, expected, value, "None", "Some")? {
            (false, EnumPayload::Empty) => Ok(None),
            (true, EnumPayload::Single(value)) => {
                let element = cx.parameter(expected, 0)?;
                let protected = cx.retain_converted(&element, value)?;
                cx.decode_value(&element, &protected).map(Some)
            }
            _ => Err(RuntimeError::module_validation("Option conversion payload")),
        }
    }
}

impl<T: KagariType, E: KagariType> KagariType for Result<T, E> {
    fn register_type(registration: &mut TypeRegistration<'_>) -> NativeResult<Type>
    where
        Self: 'static,
    {
        registration.dependency::<T>()?;
        registration.dependency::<E>()?;
        Self::kagari_type(registration.catalog())
    }

    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog
            .type_reference(&binding::result_declaration())?
            .apply([T::kagari_type(catalog)?, E::kagari_type(catalog)?])
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        check_enum_type(cx, expected, binding::result_declaration_id, 2)?;
        cx.check_type::<T>(&cx.parameter(expected, 0)?)?;
        cx.check_type::<E>(&cx.parameter(expected, 1)?)
    }
}

fn check_enum_type(
    cx: &ConversionContext<'_>,
    expected: &TypeArgument,
    declaration: impl Fn(&DefinitionTable) -> Option<DefinitionId>,
    arity: usize,
) -> NativeResult<()> {
    // The standard provider must be installed even for an empty Option or an
    // unselected Result arm. Nominal identity is never inferred from variant names.
    let types = &cx.runtime().native_entries.catalog.types;
    declaration(&types.context().snapshot())
        .and_then(|id| types.get_id(id))
        .ok_or_else(|| RuntimeError::metadata_conflict("unknown native type declaration"))?;
    if let Ty::Enum(nominal) = expected.ty()
        && nominal.arguments.len() == arity
        && declaration(expected.definitions()) == Some(nominal.declaration)
    {
        return Ok(());
    }
    Err(RuntimeError::module_validation(
        "conversion requires the installed standard enum type",
    ))
}

impl<T: IntoKagari, E: IntoKagari> IntoKagari for Result<T, E> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<ScriptValue> {
        let (member, value) = match self {
            Ok(value) => {
                let element = cx.parameter(expected, 0)?;
                ("Ok", cx.encode_transport(&element, value)?)
            }
            Err(value) => {
                let element = cx.parameter(expected, 1)?;
                ("Err", cx.encode_transport(&element, value)?)
            }
        };
        let value = cx
            .runtime()
            .make_enum_member(cx.owner(), expected, member, vec![value])?;
        cx.retain_converted(expected, value)
    }
}

impl<T: FromKagari, E: FromKagari> FromKagari for Result<T, E> {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ScriptValue,
    ) -> NativeResult<Self> {
        let raw = value.call_argument(cx.runtime(), expected, cx.owner())?;
        let value = &raw;
        let (error, payload) = enum_payload(cx, expected, value, "Ok", "Err")?;
        let EnumPayload::Single(value) = payload else {
            return Err(RuntimeError::module_validation("Result conversion payload"));
        };
        let element = cx.parameter(expected, usize::from(error))?;
        let protected = cx.retain_converted(&element, value)?;
        if error {
            cx.decode_value(&element, &protected).map(Err)
        } else {
            cx.decode_value(&element, &protected).map(Ok)
        }
    }
}

// Conversion may invoke a user adapter. Copy only the selected payload before
// releasing the heap view; the enclosing conversion frame retains the input root.
enum EnumPayload {
    Empty,
    Single(Value),
    Invalid,
}

fn enum_payload(
    cx: &ConversionContext<'_>,
    expected: &TypeArgument,
    value: &Value,
    first: &str,
    second: &str,
) -> NativeResult<(bool, EnumPayload)> {
    let Value::Enum(id) = value else {
        return Err(RuntimeError::module_validation(
            "nominal conversion requires an enum",
        ));
    };
    let (actual, payload) = cx
        .runtime()
        .gc()
        .enum_view(*id)
        .map(|view| {
            let EnumTag::Declared(actual) = &view.tag;
            let payload = match view.fields.as_slice() {
                [] => EnumPayload::Empty,
                [value] => EnumPayload::Single(*value),
                _ => EnumPayload::Invalid,
            };
            (actual.clone(), payload)
        })
        .ok_or_else(|| RuntimeError::module_validation("enum conversion identity"))?;
    for (member, choice) in [(first, false), (second, true)] {
        let expected = cx
            .runtime()
            .declared_enum_variant(cx.owner(), expected, member)?;
        if actual.matches_layout(&expected) {
            return Ok((choice, payload));
        }
    }
    Err(RuntimeError::module_validation(
        "enum conversion variant identity",
    ))
}

#[cfg(test)]
mod tests {
    use crate::{
        Runtime,
        error::RuntimeErrorKind,
        native::{
            binding::NativeResult,
            catalog::DeclarationCatalog,
            conversion::{KagariType, context::ConversionContext},
            types::Type,
        },
        test_support::layouts::allocation_owner,
    };

    use kagari_types::language::binding;

    struct UnreachableMapping;

    impl KagariType for UnreachableMapping {
        fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
            panic!("provider/nominal checks must precede child mapping");
        }
    }

    #[test]
    fn enum_mapping_requires_installed_provider_before_child_mapping() {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let cx = ConversionContext::new(&runtime, &owner).unwrap();
        let expected = cx.type_for::<i32>().unwrap();
        for interned_only in [false, true] {
            if interned_only {
                let types = &runtime.native_entries.catalog.types;
                for declaration in [binding::option_declaration(), binding::result_declaration()] {
                    let id = types.context().intern(&declaration).unwrap();
                    assert_eq!(types.context().snapshot().lookup(&declaration), Some(id));
                    assert!(types.get_id(id).is_none());
                }
            }
            for error in [
                cx.check_type::<Option<UnreachableMapping>>(&expected)
                    .unwrap_err(),
                cx.check_type::<Result<UnreachableMapping, UnreachableMapping>>(&expected)
                    .unwrap_err(),
            ] {
                assert_eq!(error.kind(), RuntimeErrorKind::MetadataConflict);
                assert!(error.message().contains("unknown native type declaration"));
            }
        }
    }
}
