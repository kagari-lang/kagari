//! Safe user conversion may reenter through another handle before a Vec edit.
use crate::{
    collections::vector::{ScriptVec, VecValues},
    tests::runtime_boundary::support::compile_program,
};
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{IntoKagari, KagariType, context::ConversionContext},
        module::NativeModule,
        typed::NativeContext,
        types::Type,
        value_handle::ScriptValue,
    },
};

struct Reentrant {
    alias: ScriptVec<i32>,
    clear: bool,
}

impl KagariType for Reentrant {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        i32::kagari_type(catalog)
    }
}

impl IntoKagari for Reentrant {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<ScriptValue> {
        let mut native = NativeContext::new(cx.runtime(), cx.owner())?;
        if self.clear {
            self.alias.clear(&mut native)?;
        } else {
            self.alias.push(&mut native, 2)?;
        }
        native.collect_garbage()?;
        3i32.into_kagari(cx, expected)
    }
}

#[rstest::rstest]
#[case("push", false, &[1, 2, 3], false)]
#[case("push", true, &[3], false)]
#[case("insert", false, &[1, 3, 2], false)]
#[case("insert", true, &[], true)]
#[case("set", false, &[3, 2], false)]
#[case("set", true, &[], true)]
fn reentrant_vector_conversion_preserves_edits_and_charge(
    #[case] operation: &str,
    #[case] clear: bool,
    #[case] expected: &[i32],
    #[case] fails: bool,
) {
    let modules = crate::modules().unwrap();
    let program = compile_program(
        &modules,
        "reentrant.kgr",
        "fn signature(value: Vec<i32>) {}",
    );
    let mut runtime = Runtime::default();
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let program = runtime.load_program("reentrant", program).unwrap();
    let owner = runtime.select_program(&program).unwrap();
    let initial_units = runtime.resources().counters().current_heap_units;
    {
        let mut conversion = ConversionContext::new(&runtime, &owner).unwrap();
        let root = conversion.encode(VecValues(vec![1i32])).unwrap();
        let values: ScriptVec<Reentrant> = conversion.decode(&root).unwrap();
        let alias: ScriptVec<i32> = conversion.decode(&root).unwrap();
        let mut native = NativeContext::new(&runtime, &owner).unwrap();
        let before = runtime.resources().counters().current_heap_units;
        let input = Reentrant {
            alias: alias.clone(),
            clear,
        };
        let result = match operation {
            "push" => values.push(&mut native, input),
            "insert" => values.insert(&mut native, 1, input),
            "set" => values.set(&mut native, 0, input),
            _ => unreachable!(),
        };
        if fails {
            assert_eq!(
                result.unwrap_err().kind(),
                RuntimeErrorKind::IndexOutOfBounds
            );
        } else {
            result.unwrap();
        }
        // Reading and collecting also prove the safe converter did not quarantine the runtime.
        assert_eq!(
            conversion.decode::<VecValues<i32>>(&root).unwrap().0,
            expected
        );
        assert_eq!(
            runtime.resources().counters().current_heap_units,
            before - 1 + expected.len()
        );
        runtime.collect_garbage().unwrap();
        alias.push(&mut native, 4).unwrap();
        assert_eq!(alias.get(&mut native, expected.len()).unwrap(), Some(4));
    }
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        runtime.resources().counters().current_heap_units,
        initial_units
    );
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[rstest::rstest]
#[case(0, 1, true, &[2, 1])]
#[case(0, 0, false, &[1, 2])]
#[case(0, 2, false, &[1, 2])]
fn host_swap_invalidates_traversal_only_after_reordering(
    #[case] left: usize,
    #[case] right: usize,
    #[case] invalidates: bool,
    #[case] expected: &[i32],
) {
    let modules = crate::modules().unwrap();
    let program = compile_program(&modules, "swap.kgr", "fn signature(value: Vec<i32>) {}");
    let mut runtime = Runtime::default();
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let program = runtime.load_program("swap", program).unwrap();
    let owner = runtime.select_program(&program).unwrap();
    let mut conversion = ConversionContext::new(&runtime, &owner).unwrap();
    let root = conversion.encode(VecValues(vec![1i32, 2])).unwrap();
    let values: ScriptVec<i32> = conversion.decode(&root).unwrap();
    let alias = values.clone();
    let mut native = NativeContext::new(&runtime, &owner).unwrap();
    let mut visited = Vec::new();
    let result = values.for_each(&mut native, |cx, value| {
        visited.push(value);
        if visited.len() == 1 {
            let swapped = alias.swap(cx, left, right);
            if right == 2 {
                assert_eq!(
                    swapped.unwrap_err().kind(),
                    RuntimeErrorKind::IndexOutOfBounds
                );
            } else {
                swapped?;
            }
        }
        Ok(())
    });
    if invalidates {
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("structure changed")
        );
        assert_eq!(visited, [1]);
    } else {
        result.unwrap();
        assert_eq!(visited, [1, 2]);
    }
    assert_eq!(
        conversion.decode::<VecValues<i32>>(&root).unwrap().0,
        expected
    );
}
