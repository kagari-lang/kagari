use crate::engine::KagariEngine;
use kagari_source::source::SourceFile;

#[test]
fn invalid_conversion_implementations_are_diagnostics() {
    for source in [
        "struct X{} impl From<X> for X {fn from(value:X)->X {value}} fn main(){}",
        "struct X{} impl<T> From<X> for T {fn from(value:X)->T {loop{}}} fn main(){}",
        r#"impl From<i32> for String {fn from(value:i32)->String {"x"}} fn main(){}"#,
        "struct X{} impl From<i32> for X {fn from(self,value:i32)->Self {self}} fn main(){}",
        "struct X{} fn main()->X {X::from(1)}",
        "struct X{} impl From<i32> for X {fn from(v:i32)->X {X{}}} fn main(){X{}.from(1);}",
    ] {
        assert!(
            KagariEngine::default()
                .compile_to_artifact(SourceFile::new("bad.kgr", source), Default::default())
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn construction_contracts_reject_invalid_signatures_and_bounds() {
    for source in [
        "struct NoDefault {} fn main() { val value: NoDefault = Default::default(); }",
        "fn main() { val value: (i32, i32) = Default::default(); }",
        "fn main() { val result:i32 = [1,2].collect(); }",
        "fn main(){[1,2].collect();}",
        "use std::collections::HashSet; struct Key{} fn main(){val result:HashSet<Key>=[Key{}].collect();}",
        "use std::collections::HashMap; fn main(){val result:HashMap<i32,i32>=[1,2].collect();}",
        "struct X{} impl Into<i32> for X {fn into(self)->i32 {1}} fn main(){}",
        "struct X{} impl TryInto<i32> for X {type Error=String;fn try_into(self)->Result<i32,String>{Ok(1)}} fn main(){}",
        "use std::str::{FromStr};\nstruct X{} impl FromStr for X {type Err=String;fn from_str(text:i32)->Result<X,String>{Ok(X{})}} fn main(){}",
        "use std::str::{FromStr};\nstruct X{} impl FromStr for X {type Err=String;fn from_str(text:String)->Result<X,String>{Ok(X{})}} fn main(){<X as FromStr<Err=i32>>::from_str(\"42\");}",
        "struct X{} impl TryFrom<i32> for X {type Error=String;fn try_from(value:i32)->X{X{}}} fn main(){}",
        "use std::iter::{Sum};\nstruct X{} impl Sum<i32> for X {fn sum<I:Iterable<Item=String>>(source:I)->X{X{}}} fn main(){}",
        "use std::iter::{Product};\nstruct X{} impl Product<i32> for X {fn product(source:i32)->X{X{}}} fn main(){}",
        "fn main(){val x=Vec<i32>::from_iter(Vec::from([true]));}",
        "fn main()->i32 {i32::sum([true])}",
        "fn main(){val x:Result<i32,String> = i32::try_from(1);}",
    ] {
        let engine = KagariEngine::builder().unwrap().build().unwrap();
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid-foundation.kgr", source),
                    Default::default()
                )
                .is_err(),
            "{source}"
        );
    }
}
