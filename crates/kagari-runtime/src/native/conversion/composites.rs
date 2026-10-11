use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        collections::vector::storage::declaration,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        types::Type,
    },
    value::{EnumTag, Value},
};
use kagari_common::identity::DefinitionPath;
use kagari_types::{language::binding, ty::Ty};

impl<T: KagariType> KagariType for Vec<T> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog
            .type_reference(&declaration())?
            .apply([T::kagari_type(catalog)?])
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        if !matches!(expected.ty(), Ty::NativeObject(nominal)
            if binding::matches(&nominal.declaration, &declaration(), Some(expected.definitions())) && nominal.arguments.len() == 1)
        {
            return Err(RuntimeError::module_validation(
                "Vec conversion requires a registered Vec type",
            ));
        }
        cx.check_type::<T>(&cx.parameter(expected, 0)?)
    }
}

impl<T: IntoKagari> IntoKagari for Vec<T> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        cx.check_elements(self.len())?;
        let element = cx.parameter(expected, 0)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(self.len())
            .map_err(|_| RuntimeError::resource_limit("Vec conversion capacity"))?;
        for item in self {
            values.push(cx.encode_value(&element, item)?);
        }
        cx.runtime().allocate_sequence(cx.owner(), expected, values)
    }
}

impl<T: FromKagari> FromKagari for Vec<T> {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        let Value::GcHandle(array) = value else {
            return Err(RuntimeError::module_validation(
                "Vec conversion requires a nominal sequence",
            ));
        };
        let count =
            cx.runtime().gc().sequence_len(*array).ok_or_else(|| {
                RuntimeError::module_validation("Vec conversion sequence identity")
            })?;
        cx.check_elements(count)?;
        // Snapshot before user-defined child conversion can reenter or mutate an alias.
        let mut snapshot = Vec::new();
        snapshot
            .try_reserve_exact(count)
            .map_err(|_| RuntimeError::resource_limit("Vec conversion snapshot"))?;
        for index in 0..count {
            cx.poll()?;
            snapshot.push(
                cx.runtime()
                    .gc()
                    .sequence_get(*array, index)
                    .ok_or_else(|| {
                        RuntimeError::module_validation("Vec conversion sequence element")
                    })?,
            );
        }
        let roots = cx
            .runtime()
            .gc()
            .root_execution_values(snapshot)
            .ok_or_else(|| RuntimeError::module_validation("Vec conversion snapshot roots"))?;
        let element = cx.parameter(expected, 0)?;
        let mut result = Vec::new();
        result
            .try_reserve_exact(count)
            .map_err(|_| RuntimeError::resource_limit("Vec conversion output"))?;
        for index in 0..count {
            let value = roots
                .get(cx.runtime().gc(), index)
                .ok_or_else(|| RuntimeError::module_validation("Vec conversion snapshot value"))?;
            result.push(cx.decode_value(&element, &value)?);
        }
        Ok(result)
    }
}

impl<T: KagariType> KagariType for Option<T> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog
            .type_reference(&binding::option_declaration())?
            .apply([T::kagari_type(catalog)?])
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        check_enum_type(cx, expected, &binding::option_declaration(), 1)?;
        cx.check_type::<T>(&cx.parameter(expected, 0)?)
    }
}

impl<T: IntoKagari> IntoKagari for Option<T> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        let (member, fields) = match self {
            None => ("None", vec![]),
            Some(value) => {
                let element = cx.parameter(expected, 0)?;
                ("Some", vec![cx.encode_value(&element, value)?])
            }
        };
        cx.runtime()
            .make_enum_member(cx.owner(), expected, member, fields)
    }
}

impl<T: FromKagari> FromKagari for Option<T> {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        match enum_payload(cx, expected, value, "None", "Some")? {
            (false, EnumPayload::Empty) => Ok(None),
            (true, EnumPayload::Single(value)) => {
                let element = cx.parameter(expected, 0)?;
                cx.decode_value(&element, &value).map(Some)
            }
            _ => Err(RuntimeError::module_validation("Option conversion payload")),
        }
    }
}

impl<T: KagariType, E: KagariType> KagariType for Result<T, E> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog
            .type_reference(&binding::result_declaration())?
            .apply([T::kagari_type(catalog)?, E::kagari_type(catalog)?])
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        check_enum_type(cx, expected, &binding::result_declaration(), 2)?;
        cx.check_type::<T>(&cx.parameter(expected, 0)?)?;
        cx.check_type::<E>(&cx.parameter(expected, 1)?)
    }
}

fn check_enum_type(
    cx: &ConversionContext<'_>,
    expected: &TypeArgument,
    declaration: &DefinitionPath,
    arity: usize,
) -> NativeResult<()> {
    // The standard provider must be installed even for an empty Option or an
    // unselected Result arm. Nominal identity is never inferred from variant names.
    cx.runtime()
        .native_entries
        .catalog
        .type_reference(declaration)?;
    if let Ty::Enum(nominal) = expected.ty()
        && nominal.arguments.len() == arity
        && expected
            .definitions()
            .resolve(nominal.declaration)
            .is_ok_and(|view| view.to_path() == *declaration)
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
    ) -> NativeResult<Value> {
        let (member, value) = match self {
            Ok(value) => {
                let element = cx.parameter(expected, 0)?;
                ("Ok", cx.encode_value(&element, value)?)
            }
            Err(value) => {
                let element = cx.parameter(expected, 1)?;
                ("Err", cx.encode_value(&element, value)?)
            }
        };
        cx.runtime()
            .make_enum_member(cx.owner(), expected, member, vec![value])
    }
}

impl<T: FromKagari, E: FromKagari> FromKagari for Result<T, E> {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        let (error, payload) = enum_payload(cx, expected, value, "Ok", "Err")?;
        let EnumPayload::Single(value) = payload else {
            return Err(RuntimeError::module_validation("Result conversion payload"));
        };
        let element = cx.parameter(expected, usize::from(error))?;
        if error {
            cx.decode_value(&element, &value).map(Err)
        } else {
            cx.decode_value(&element, &value).map(Ok)
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
