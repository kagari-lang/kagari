use kagari_common::source_database::{SourceDatabase, SourceLayer};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;

fn compile(text: &str) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("foundation.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![]);
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&mir).unwrap();
    kagari_bytecode::program::verify_program(&bytecode).unwrap();
}

#[test]
fn scalar_language_program_compiles_without_optional_modules() {
    compile("fn main() -> i32 { if 1 + 2 == 3 { 42 } else { 0 } }");
}

#[test]
fn default_list_native_calls_compile_without_optional_modules() {
    compile("fn main() -> i32 { val values = [1, 2]; values.push(3); values[0] }");
}

#[test]
fn collection_interface_iteration_compiles_without_optional_modules() {
    compile(
        "fn main() -> i32 { val values: List<i32> = [1, 2]; var total = 0; for item in values { total = total + item; } total }",
    );
}

#[test]
fn foundational_static_methods_and_generic_bounds_compile() {
    compile(
        r#"
struct Count { val value: i32 }
impl From<i32> for Count { fn from(value:i32)->Self { Count {value} } }
impl TryFrom<i32> for Count {
    type Error = String;
    fn try_from(value:i32)->Result<Self,String> { if value < 0 {Err("negative")} else {Ok(Count{value})} }
}
impl FromStr for Count {
    type Err = String;
    fn from_str(text:String)->Result<Self,String> { if text == "42" {Ok(Count{value:42})} else {Err(text)} }
}
impl Sum<i32> for Count {
    fn sum<I:Iterable<Item=i32>>(source:I)->Self {
        var value=0; for item in source {value=value+item;} Count{value}
    }
}
impl Product<i32> for Count {
    fn product<I:Iterable<Item=i32>>(source:I)->Self {
        var value=1; for item in source {value=value*item;} Count{value}
    }
}
impl FromIterator<i32> for Count {
    fn from_iter<I:Iterable<Item=i32>>(source:I)->Self { Self::sum(source) }
}
fn collect<T:FromIterator<i32>,I:Iterable<Item=i32>>(source:I)->T {T::from_iter(source)}
fn main()->i32 {
    val a:Count=collect([10,11]);
    val b=Count::product([3,7]);
    val c=Count::from(1);
    val d=Count::try_from(c.value);
    val e=<Count as FromStr>::from_str("42");
    match e {Ok(n)=>n.value,Err(_)=>a.value+b.value}
}
"#,
    );
}
