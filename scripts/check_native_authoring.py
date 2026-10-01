"""Prove Rust compile-time native contracts in a separately compiled consumer."""
from pathlib import Path
import json
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "target" / "native-authoring"


def run() -> None:
    OUTPUT.mkdir(parents=True, exist_ok=True)
    (OUTPUT / "src").mkdir(exist_ok=True)
    manifest = OUTPUT / "Cargo.toml"
    manifest.write_text('\n'.join([
        '[workspace]', '[package]', 'name = "native-authoring-contracts"',
        'version = "0.0.0"', 'edition = "2024"', '[dependencies]',
        f'kagari-runtime = {{ path = {json.dumps(str(ROOT / "crates/kagari-runtime"))} }}',
        f'kagari-abi = {{ path = {json.dumps(str(ROOT / "crates/kagari-abi"))} }}',
        f'kagari-native-macros = {{ path = {json.dumps(str(ROOT / "crates/kagari-native-macros"))} }}',
        '[profile.dev]', 'opt-level = 1', '',
    ]))
    (OUTPUT / "Cargo.lock").write_bytes((ROOT / "Cargo.lock").read_bytes())
    cases = {
        "return-type": ('''#[native_module("game::math")]
            mod math { #[native] fn answer() -> usize { true } }''', "E0308"),
        "trait-contract": ('''#[native_module("game::array")]
            mod array {
                use kagari_runtime::native_value::{array::NativeArray, NativeValue, NativeResult};
                #[native_type] struct Array<T: NativeValue>(NativeArray<T>);
                #[native_trait] trait MutableList<T: NativeValue> {
                    fn set(&self, index: usize, value: T) -> NativeResult<()>;
                }
                #[native_impl] impl<T: NativeValue> MutableList<T> for Array<T> {
                    fn set(&self, index: bool, value: T) -> NativeResult<()> { Ok(()) }
                }
            }''', "E0053"),
        "unscoped-reference": ('''#[native_module("game::math")]
            mod math { #[native] fn borrowed(value: &str) -> usize { value.len() } }''', "E0277"),
        "associated-contract": ('''#[native_module("game::source")]
            mod source {
                use kagari_runtime::native_value::{array::NativeArray, NativeValue};
                #[native_type] struct Bag<T: NativeValue>(NativeArray<T>);
                #[native_trait] trait Source {
                    type Item: NativeValue;
                    fn echo(&self, value: Self::Item) -> Self::Item;
                }
                #[native_impl] impl<T: NativeValue> Source for Bag<T> {
                    type Item = T;
                    fn echo(&self, value: bool) -> bool { value }
                }
            }''', "E0053"),
        "callback-argument-pack": ('''#[native_module("game::calls")]
            mod calls {
                use kagari_runtime::native_value::continuation::NativeFn;
                #[native] fn callback(value: NativeFn<usize, i32>) -> i32 { 0 }
            }''', "E0277"),
    }
    positive = """use kagari_runtime as renamed_runtime;
        #[native_module("game::math", runtime = crate::renamed_runtime)]
        pub mod math {
            use crate::renamed_runtime::native_value::NativeValue;
            type Count = usize;
            type Box = u8;
            type Result = u16;
            type Option = u32;
            #[native] pub fn __call(value: Count) -> bool { value > 0 }
            #[native] pub fn identity<T: NativeValue>(value: T) -> T { value }
            pub fn verify() {
                let _: (Box, Result, Option) = (0, 0, 0);
                native_api().unwrap();
            }
        }
    """
    cases["resolved-types-and-hygiene"] = (positive, None)
    for label, (source, diagnostic) in cases.items():
        (OUTPUT / "src/lib.rs").write_text('use kagari_native_macros::native_module;\n' + source + '\n')
        result = subprocess.run([
            "cargo", "check", "--offline", "--manifest-path", str(manifest),
            "--target-dir", str(ROOT / "target"),
        ], cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        (OUTPUT / f"{label}.log").write_text(result.stdout)
        if diagnostic is None:
            assert result.returncode == 0, (label, result.stdout)
            print(f"{label}: actual Rust definitions compile", flush=True)
        else:
            assert result.returncode != 0 and diagnostic in result.stdout, (label, result.stdout)
            print(f"{label}: Rust rejects the invalid native contract", flush=True)


if __name__ == "__main__":
    run()
