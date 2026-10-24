//! A retained value with its exact declared type for generic native callbacks.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::roots::RootedValue,
    module::{ExecutableModule, ModuleEpochRetention, retention::ProgramLease},
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        typed::NativeContext,
        types::Type,
    },
    value::Value,
};

/// Forward an arbitrary declared generic value without copying its object graph
/// or exposing raw storage. This does not add an untyped Kagari value: each handle
/// keeps its exact applied type and access view, checked on every conversion.
#[derive(Debug, Clone)]
pub struct ScriptValue {
    root: RootedValue,
    argument: TypeArgument,
    owner: ExecutableModule,
    _program: ProgramLease,
}

impl ScriptValue {
    pub fn type_argument(&self) -> &TypeArgument {
        &self.argument
    }

    /// Admit the root's scope and handle without recursively examining its value.
    /// The codec must subsequently check its shape and every child under its budget.
    #[cfg(feature = "serde")]
    pub(crate) fn snapshot_value(&self, cx: &ConversionContext<'_>) -> NativeResult<Value> {
        cx.poll()?;
        cx.runtime().validate_executable_module(&self.owner)?;
        self.argument.validate_for(cx.runtime(), cx.owner())?;
        let value = self
            .root
            .value(cx.runtime().gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired script value"))?;
        Ok(value)
    }

    #[cfg(feature = "serde")]
    pub(crate) fn from_kagari_with_poll(
        cx: &ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
        poll: &mut dyn FnMut() -> NativeResult<()>,
    ) -> NativeResult<Self> {
        cx.check_type::<Self>(expected)?;
        cx.check_value_with_poll(expected, value, poll)?;
        Self::retain_checked(cx, expected, value)
    }

    fn retain_checked(
        cx: &ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        let root = cx
            .runtime()
            .root_value(*value)
            .ok_or_else(|| RuntimeError::module_validation("script value retention"))?;
        let program = cx
            .runtime()
            .retain_program(cx.owner(), ModuleEpochRetention::RuntimeValue)
            .ok_or_else(|| RuntimeError::module_validation("script value program retention"))?;
        Ok(Self {
            root,
            argument: expected.clone(),
            owner: cx.owner().clone(),
            _program: program,
        })
    }

    pub fn decode<T: FromKagari>(&self, cx: &mut NativeContext<'_>) -> NativeResult<T> {
        let mut conversion = ConversionContext::new(cx.runtime(), &self.owner)?;
        let value = self
            .root
            .value(cx.runtime().gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired script value"))?;
        conversion.decode_prepared(&self.argument, &value)
    }
}

impl KagariType for ScriptValue {
    fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
        Err(RuntimeError::module_validation(
            "script value requires a declared contextual type",
        ))
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        expected.validate(cx.runtime())
    }
}

impl FromKagari for ScriptValue {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        cx.check_type::<Self>(expected)?;
        cx.check_value(expected, value)?;
        Self::retain_checked(cx, expected, value)
    }
}

#[cfg(all(test, feature = "serde"))]
mod snapshot_tests {
    use super::ScriptValue;
    use crate::{
        Runtime,
        native::conversion::{FromKagari, context::ConversionContext},
        state::{
            self,
            data::{SchemaVersion, SnapshotHeader},
            error::SnapshotErrorKind,
            limits::SnapshotOptions,
        },
        test_support::layouts::allocation_owner,
        value::Value,
    };

    #[test]
    fn snapshot_root_admission_defers_tuple_children_to_the_bounded_walk() {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
        let expected = cx.type_for::<(i32, (i32, i32))>().unwrap();
        let raw = cx.encode((1i32, (2i32, 3i32))).unwrap();
        let root = ScriptValue::from_kagari(&mut cx, &expected, &raw.value(runtime.gc()).unwrap())
            .unwrap();
        let bad_child = runtime
            .gc()
            .alloc_tuple(vec![Value::I32(2), Value::Bool(true)])
            .unwrap();
        let bad_child = runtime.root_value(bad_child).unwrap();
        let bad = runtime
            .gc()
            .alloc_tuple(vec![Value::I32(1), bad_child.value(runtime.gc()).unwrap()])
            .unwrap();
        // Only this private test can replace the retained handle without its type
        // check. Public construction cannot forge this mismatched declared type.
        root.root.set(runtime.gc(), bad).unwrap();
        let header = SnapshotHeader {
            schema: SchemaVersion::new("root", 1).unwrap(),
            source_deployment: "source".into(),
            state_owner: "owner".into(),
            state_revision: 1,
        };
        let options = SnapshotOptions::default();
        options.cancellation.cancel();
        assert_eq!(
            state::export(&mut cx, &root, header.clone(), &options)
                .unwrap_err()
                .kind,
            SnapshotErrorKind::Cancelled
        );
        let mut options = SnapshotOptions::default();
        options.limits.max_work = 1;
        assert_eq!(
            state::export(&mut cx, &root, header.clone(), &options)
                .unwrap_err()
                .kind,
            SnapshotErrorKind::LimitExceeded { limit: "work" }
        );
        let baseline = runtime.gc().active_roots();
        let error = state::export(&mut cx, &root, header, &SnapshotOptions::default()).unwrap_err();
        assert_eq!(error.kind, SnapshotErrorKind::Runtime);
        assert_eq!(error.path.to_string(), "$[1][1]");
        assert_eq!(runtime.gc().active_roots(), baseline);
    }
}

impl IntoKagari for ScriptValue {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        cx.check_type::<Self>(expected)?;
        if !self
            .argument
            .view(&self.owner)
            .compatible(expected.view(cx.owner()))
        {
            return Err(RuntimeError::module_validation(
                "script value declared type differs from its destination",
            ));
        }
        let value = self
            .root
            .value(cx.runtime().gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired script value"))?;
        cx.check_value(expected, &value)?;
        Ok(value)
    }
}
