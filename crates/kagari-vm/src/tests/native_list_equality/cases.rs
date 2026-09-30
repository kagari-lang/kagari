pub(super) const TYPES: &str = r#"
struct Sequence<T> {val items:ArrayList<T>}
impl<T> Iterable for Sequence<T> {type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{print("iter");self.items.iter()}}
impl<T> Index<usize> for Sequence<T> {type Output=T;fn index(self,index:usize)->T{self.items[index]}}
impl<T> List<T> for Sequence<T> {fn len(self)->usize{print("len");self.items.len()}fn is_empty(self)->bool{self.items.is_empty()}fn get(self,index:usize)->Option<T>{print("get");self.items.get(index)}}
struct Key<T> {val value:T}
impl<T:PartialEq> PartialEq for Key<T> {fn eq(self,other:Self)->bool {print("eq");self.value==other.value}}
enum Choice<T> {Empty,Pair(i32,T),Other(T)}
"#;
fn item(kind: &str, value: i32) -> String {
    match kind {
        "scalar" => value.to_string(),
        "leaf" => format!("Key{{value:{value}}}"),
        "tuple" => format!("(Key{{value:{value}}},{value})"),
        "option" => {
            if value == 10 {
                "None".into()
            } else {
                format!("Some(Key{{value:{value}}})")
            }
        }
        "enum" => {
            if value == 10 {
                "Choice::Empty".into()
            } else if value == 40 {
                format!("Choice::Other(Key{{value:{value}}})")
            } else {
                format!("Choice::Pair({value},Key{{value:{value}}})")
            }
        }
        _ => format!("({},{})", item("option", value), item("enum", value)),
    }
}
pub(super) fn cases() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for kind in ["scalar", "leaf", "tuple", "option", "enum", "nested"] {
        let ty = match kind {
            "scalar" => "i32",
            "leaf" => "Key<i32>",
            "tuple" => "(Key<i32>,i32)",
            "option" => "Option<Key<i32>>",
            "enum" => "Choice<Key<i32>>",
            _ => "(Option<Key<i32>>,Choice<Key<i32>>)",
        };
        for source_kind in ["array", "custom", "dynamic", "view"] {
            for method in ["contains", "starts_with", "ends_with"] {
                for scenario in 0..if method == "contains" { 3 } else { 5 } {
                    let values = if scenario == 0 && method == "contains" || scenario == 3 {
                        vec![]
                    } else if scenario == 4 {
                        vec![10]
                    } else {
                        vec![10, 20, 30]
                    };
                    let needle = if method == "contains" {
                        vec![if scenario == 2 { 40 } else { 20 }]
                    } else {
                        match scenario {
                            0 => vec![],
                            1 => {
                                if method == "starts_with" {
                                    vec![10, 20]
                                } else {
                                    vec![20, 30]
                                }
                            }
                            2 => {
                                if method == "starts_with" {
                                    vec![10, 40]
                                } else {
                                    vec![20, 40]
                                }
                            }
                            3 => vec![10],
                            _ => vec![10, 20],
                        }
                    };
                    let values = values
                        .iter()
                        .map(|value| item(kind, *value))
                        .collect::<Vec<_>>()
                        .join(",");
                    let right = if method == "contains" {
                        item(kind, needle[0])
                    } else {
                        "needle".into()
                    };
                    let needle = needle
                        .iter()
                        .map(|value| item(kind, *value))
                        .collect::<Vec<_>>()
                        .join(",");
                    let argument = if matches!(source_kind, "custom" | "dynamic") {
                        "Sequence{items:values}"
                    } else {
                        "values"
                    };
                    let constraint = if matches!(source_kind, "custom" | "array") {
                        format!("fn consume<L:List<{ty}>>(source:L)")
                    } else {
                        format!("fn consume(source:List<{ty}>)")
                    };
                    let needle_statement = if method == "contains" {
                        String::new()
                    } else {
                        format!(
                            "val other:ArrayList<{ty}> =[{needle}];val needle:List<{ty}> ={};",
                            if matches!(source_kind, "custom" | "dynamic") {
                                "Sequence{items:other}"
                            } else {
                                "other"
                            }
                        )
                    };
                    let expected = scenario == 1 || (method != "contains" && scenario == 0);
                    let source = format!(
                        "{TYPES}\n{constraint}{{{needle_statement}val result=source.{method}({right});std::debug::assert_eq(result,{expected},\"selected\");print(\"done\");}}fn main()->i32{{val values:ArrayList<{ty}> =[{values}];consume({argument});42}}"
                    );
                    cases.push((format!("{kind}_{source_kind}_{method}_{scenario}"), source));
                }
            }
        }
    }
    cases
}
