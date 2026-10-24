use std::{fs, path::PathBuf};

use rstest::rstest;

use self::support::{Environment, Fixture};

mod contract;

mod support;
mod typed;

#[rstest]
fn bare(
    #[base_dir = "tests/fixtures/ui/bare"]
    #[files("*.kgr")]
    path: PathBuf,
) {
    let text = fs::read_to_string(&path).expect("read exact UTF-8 UI source");
    Fixture::new(&text, "test.kg", Environment::Bare).compare(&path);
}

#[rstest]
fn standard(
    #[base_dir = "tests/fixtures/ui/standard"]
    #[files("*.kgr")]
    path: PathBuf,
) {
    let text = fs::read_to_string(&path).expect("read exact UTF-8 UI source");
    Fixture::new(&text, "test.kg", Environment::Standard).compare(&path);
}

#[rstest]
fn local_functions(
    #[base_dir = "tests/fixtures/ui/local_functions"]
    #[files("*.kgr")]
    path: PathBuf,
) {
    let text = fs::read_to_string(&path).expect("read exact UTF-8 UI source");
    let fixture = Fixture::new(&text, "locals.kgr", Environment::Standard);
    fixture.compare(&path);
    assert!(
        fixture.result().clone().into_codegen().is_err(),
        "{}",
        path.display()
    );
}
