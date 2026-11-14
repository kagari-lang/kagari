use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        schema::TypeRegistration,
        types::Type,
    },
    value::view::ValueView,
};
use kagari_common::identity::DefinitionPath;
use kagari_types::{language::binding, ty::Ty};

// Rust ownership indirection is transparent to the script type and layout.
impl<T: KagariType> KagariType for Box<T> {
    fn register_type(registration: &mut TypeRegistration<'_>) -> NativeResult<Type>
    where
        Self: 'static,
    {
        registration.dependency::<T>()
    }

    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        T::kagari_type(catalog)
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        T::check_type(cx, expected)
    }
}

impl<T: IntoKagari> IntoKagari for Box<T> {
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        // The enclosing conversion frame already owns type/root/depth checks.
        (*self).into_kagari(cx, expected)
    }
}

impl<T: FromKagari> FromKagari for Box<T> {
    const PRESERVES_IDENTITY: bool = T::PRESERVES_IDENTITY;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<Self> {
        // Box adds no script graph node; reentering would report a false cycle.
        T::from_kagari(cx, expected, value).map(Box::new)
    }
}

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
        check_enum_type(cx, expected, &binding::option_declaration(), 1)?;
        cx.check_type::<T>(&cx.parameter(expected, 0)?)
    }
}

impl<T: IntoKagari> IntoKagari for Option<T> {
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        match self {
            None => cx.allocate_enum_member(expected, "None", &[]),
            Some(value) => {
                let element = cx.parameter(expected, 0)?;
                let value = cx.encode_value(&element, value)?;
                cx.allocate_enum_member(expected, "Some", &[value])
            }
        }
    }
}

impl<T: FromKagari> FromKagari for Option<T> {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ValueView<'_>,
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
        && expected.definitions().lookup(declaration) == Some(nominal.declaration)
    {
        return Ok(());
    }
    Err(RuntimeError::module_validation(
        "conversion requires the installed standard enum type",
    ))
}

impl<T: IntoKagari, E: IntoKagari> IntoKagari for Result<T, E> {
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
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
        cx.allocate_enum_member(expected, member, &[value])
    }
}

impl<T: FromKagari, E: FromKagari> FromKagari for Result<T, E> {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ValueView<'_>,
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
enum EnumPayload<'call> {
    Empty,
    Single(ValueView<'call>),
    Invalid,
}

fn enum_payload<'call>(
    cx: &mut ConversionContext<'call>,
    expected: &TypeArgument,
    value: &ValueView<'_>,
    first: &str,
    second: &str,
) -> NativeResult<(bool, EnumPayload<'call>)> {
    let (actual, length) = cx.enum_shape(value)?;
    for (member, choice) in [(first, false), (second, true)] {
        let expected = cx
            .runtime()
            .declared_enum_variant(cx.owner(), expected, member)?;
        if actual.matches_layout(&expected) {
            let payload = match length {
                0 => EnumPayload::Empty,
                1 => EnumPayload::Single(cx.enum_element(value, 0)?),
                _ => EnumPayload::Invalid,
            };
            return Ok((choice, payload));
        }
    }
    Err(RuntimeError::module_validation(
        "enum conversion variant identity",
    ))
}
