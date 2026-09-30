pub(super) fn cases() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (scalar, valid, value) in [
        ("i8", "-128", "-128i8"),
        ("i16", "-32768", "-32768i16"),
        ("i32", "42", "42"),
        ("i64", "42", "42i64"),
        ("isize", "42", "42isize"),
        ("u8", "255", "255u8"),
        ("u16", "65535", "65535u16"),
        ("u32", "42", "42u32"),
        ("u64", "42", "42u64"),
        ("usize", "42", "42usize"),
        ("f32", "1.5", "1.5f32"),
        ("f64", "1.5", "1.5f64"),
        ("bool", "true", "true"),
    ] {
        for (index, text) in [valid, "", "bad", " 42"].into_iter().enumerate() {
            for generic in [false, true] {
                let call = if generic {
                    format!("parsed::<{scalar}>(source({text:?}))")
                } else {
                    format!("source({text:?}).parse::<{scalar}>()")
                };
                let check = if index == 0 {
                    format!("result==Ok({value})")
                } else {
                    "result.is_err()".into()
                };
                out.push((
                    format!("parse_{scalar}_{index}_{generic}"),
                    format!(
                        r#"
fn source(text:String)->String{{print("source");text}}
fn parsed<T:FromStr>(text:String)->Result<T,<T as FromStr>::Err>{{text.parse::<T>()}}
fn main()->i32{{val result={call};std::debug::assert({check},"parsed");print("done");42}}
"#
                    ),
                ));
            }
        }
    }
    for success in [false, true] {
        for generic in [false, true] {
            let text = if success { "42" } else { "bad" };
            let call = if generic {
                format!("parsed::<Wrapped<i32>>(source({text:?}))")
            } else {
                format!("String::parse::<Wrapped<i32>>(source({text:?}))")
            };
            out.push((format!("parse_custom_{success}_{generic}"),format!(r#"
struct Wrapped<T>{{val value:T}}
impl<T:FromStr> FromStr for Wrapped<T>{{type Err=<T as FromStr>::Err;fn from_str(text:String)->Result<Self,Self::Err>{{print("parse");text.parse::<T>().map(|value|{{print("wrap");Wrapped{{value:value}}}})}}}}
fn source(text:String)->String{{print("source");text}}
fn parsed<T:FromStr>(text:String)->Result<T,<T as FromStr>::Err>{{text.parse::<T>()}}
fn main()->i32{{val result={call};std::debug::assert(result.is_ok()=={success},"custom");print("done");42}}
"#)));
        }
    }
    for kind in [
        "scalar",
        "identity",
        "nominal",
        "option",
        "tuple",
        "enumeration",
        "dynamic",
    ] {
        for equal in [false, true] {
            for generic in [false, true] {
                let value = if equal { 42 } else { 43 };
                let (ty, first, second): (&str, String, String) = match kind {
                    "scalar" => ("i32", "42".into(), value.to_string()),
                    "identity" => (
                        "ArrayList<i32>",
                        "storage".into(),
                        if equal {
                            "storage".into()
                        } else {
                            "[42]".into()
                        },
                    ),
                    "nominal" => (
                        "Leaf<i32>",
                        "Leaf{value:42,visits:0}".into(),
                        format!("Leaf{{value:{value},visits:0}}"),
                    ),
                    "option" => (
                        "Option<Leaf<i32>>",
                        "Some(Leaf{value:42,visits:0})".into(),
                        format!("Some(Leaf{{value:{value},visits:0}})"),
                    ),
                    "tuple" => (
                        "(Leaf<i32>,i32)",
                        "(Leaf{value:42,visits:0},7)".into(),
                        format!("(Leaf{{value:{value},visits:0}},7)"),
                    ),
                    "enumeration" => (
                        "Wrap<Leaf<i32>>",
                        "Wrap::One(Leaf{value:42,visits:0})".into(),
                        format!("Wrap::One(Leaf{{value:{value},visits:0}})"),
                    ),
                    _ => (
                        "List<i32>",
                        "storage".into(),
                        if equal {
                            "storage".into()
                        } else {
                            "[42]".into()
                        },
                    ),
                };
                let call = if generic {
                    "checked(left(a),right(b),message())"
                } else {
                    "std::debug::assert_eq(left(a),right(b),message())"
                };
                out.push((format!("assert_{kind}_{equal}_{generic}"),format!(r#"
struct Leaf<T>{{val value:T,var visits:i32}}
impl<T:PartialEq> PartialEq for Leaf<T>{{fn eq(self,other:Self)->bool{{print("eq");self.visits+=1;self.value==other.value}}}}
enum Wrap<T>{{One(T),Empty}}
fn left<T>(value:T)->T{{print("left");value}}fn right<T>(value:T)->T{{print("right");value}}
fn message()->String{{print("message");"native assertion"}}
fn checked<T:PartialEq>(a:T,b:T,message:String){{std::debug::assert_eq(a,b,message);}}
fn main()->i32{{val storage=[42];val a:{ty} = {first};val b:{ty} = {second};{call};print("done");42}}
"#)));
            }
        }
    }
    out
}
