use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        storage_type::StorageType,
        types::Type,
    },
    value::{EnumTag, Value},
};
use kagari_common::identity::DefinitionPath;
use kagari_types::{collection::CollectionAccess, language::binding, ty::Ty};
use std::sync::Arc;

impl<T: KagariType> KagariType for Vec<T> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        Ok(Type::from_semantic(Ty::Array(
            Box::new(T::kagari_type(catalog)?.abi().clone()),
            CollectionAccess::Mutable,
        )))
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        if !matches!(expected.ty(), Ty::Array(_, CollectionAccess::Mutable)) {
            return Err(RuntimeError::module_validation(
                "Vec conversion requires a mutable array type",
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
        let contract = Arc::new(StorageType::prepare_scoped(element, cx.owner())?);
        cx.runtime()
            .gc()
            .alloc_array_with_contract(contract, values)
            .map(Value::Array)
    }
}

impl<T: FromKagari> FromKagari for Vec<T> {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        let Value::Array(array) = value else {
            return Err(RuntimeError::module_validation(
                "Vec conversion requires an array",
            ));
        };
        let count = cx
            .runtime()
            .gc()
            .array_len(*array)
            .ok_or_else(|| RuntimeError::module_validation("Vec conversion array identity"))?;
        cx.check_elements(count)?;
        // Snapshot before user-defined child conversion can reenter or mutate an alias.
        let mut snapshot = Vec::new();
        snapshot
            .try_reserve_exact(count)
            .map_err(|_| RuntimeError::resource_limit("Vec conversion snapshot"))?;
        for index in 0..count {
            cx.poll()?;
            snapshot.push(
                cx.runtime().gc().array_get(*array, index).ok_or_else(|| {
                    RuntimeError::module_validation("Vec conversion array element")
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
            (false, fields) if fields.is_empty() => Ok(None),
            (true, fields) if fields.len() == 1 => {
                let element = cx.parameter(expected, 0)?;
                cx.decode_value(&element, &fields[0]).map(Some)
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
        let (error, fields) = enum_payload(cx, expected, value, "Ok", "Err")?;
        if fields.len() != 1 {
            return Err(RuntimeError::module_validation("Result conversion payload"));
        }
        let element = cx.parameter(expected, usize::from(error))?;
        if error {
            cx.decode_value(&element, &fields[0]).map(Err)
        } else {
            cx.decode_value(&element, &fields[0]).map(Ok)
        }
    }
}

fn enum_payload(
    cx: &ConversionContext<'_>,
    expected: &TypeArgument,
    value: &Value,
    first: &str,
    second: &str,
) -> NativeResult<(bool, Vec<Value>)> {
    let Value::Enum(id) = value else {
        return Err(RuntimeError::module_validation(
            "nominal conversion requires an enum",
        ));
    };
    let snapshot = cx
        .runtime()
        .gc()
        .enum_snapshot(*id)
        .ok_or_else(|| RuntimeError::module_validation("enum conversion identity"))?;
    let EnumTag::Declared(actual) = snapshot.tag;
    for (member, choice) in [(first, false), (second, true)] {
        let expected = cx
            .runtime()
            .declared_enum_variant(cx.owner(), expected, member)?;
        if actual.matches_layout(&expected) {
            return Ok((choice, snapshot.fields));
        }
    }
    Err(RuntimeError::module_validation(
        "enum conversion variant identity",
    ))
}
