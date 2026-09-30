pub(super) const TYPES: &str = r#"
struct Counter<T>{val items:ArrayList<T>,var index:usize}
impl<T> Iterator for Counter<T>{type Item=T;fn next(self)->Option<T>{print("next");if self.index>=self.items.len(){None}else{val item=self.items[self.index];self.index+=1;Some(item)}}}
struct Sequence<T>{val items:ArrayList<T>}
impl<T> Iterable for Sequence<T>{type Item=T;type Iter=Counter<T>;fn iter(self)->Counter<T>{print("iter");Counter{items:self.items,index:0}}}
struct Bag<T>{val items:ArrayList<T>}
impl<T> FromIterator<T> for Bag<T>{fn from_iter<I:Iterable<Item=T>>(source:I)->Self{print("construct");Bag{items:source.iter().collect::<ArrayList<T>>()}}}
struct Total{val value:i32}
impl FromIterator<i32> for Total{fn from_iter<I:Iterable<Item=i32>>(source:I)->Self{print("construct");Total{value:source.iter().fold(0,|sum,item|{print("destination");sum+item})}}}
struct Key{val value:i32}
impl PartialEq for Key{fn eq(self,other:Self)->bool{print("eq");self.value==other.value}}
impl Eq for Key{}
impl Hash for Key{fn hash(self)->i64{print("hash");(self.value%2) as i64}}
fn source<T>(value:T)->T{print("source");value}
fn consume<T,I:Iterator<Item=T>,C:FromIterator<T>>(value:I)->C{value.collect()}
fn divided<T,I:Iterator<Item=T>,C:FromIterator<T>>(value:I,predicate:fn(T)->bool)->(C,C){value.partition(predicate)}
"#;

fn shape(kind: &str) -> (&'static str, &'static str, &'static str) {
    match kind {
        "array" => ("i32", "ArrayList<i32>", "20,22,20"),
        "bag" => ("i32", "Bag<i32>", "20,22,20"),
        "total" => ("i32", "Total", "20,22,20"),
        "set" => (
            "Key",
            "LinkedHashSet<Key>",
            "Key{value:20},Key{value:22},Key{value:20}",
        ),
        _ => (
            "(Key,i32)",
            "LinkedHashMap<Key,i32>",
            "(Key{value:20},0),(Key{value:22},1),(Key{value:20},2)",
        ),
    }
}

fn valid(kind: &str, value: &str, length: usize, total: i32) -> String {
    match kind {
        "bag" => format!("{value}.items.len()=={length}usize"),
        "total" => format!("{value}.value=={total}"),
        _ => format!("{value}.len()=={length}usize"),
    }
}

fn input(route: &str, items: &str) -> String {
    match route {
        "native" => format!("[{items}].iter()"),
        "custom" => format!("Counter{{items:[{items}],index:0}}"),
        _ => format!("[{items}].iter().map(|item|{{print(\"item\");item}})"),
    }
}

pub(super) fn cases() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for kind in ["array", "bag", "total", "set", "map"] {
        let (item, destination, contents) = shape(kind);
        for route in ["native", "custom", "lazy"] {
            let iterator = format!(
                "{}<{item}>",
                if route == "custom" { "Counter" } else { "Iter" }
            );
            for empty in [false, true] {
                for generic in [false, true] {
                    let source = input(route, if empty { "" } else { contents });
                    let call = if generic {
                        format!("consume::<{item},{iterator},{destination}>(source({source}))")
                    } else {
                        format!("source({source}).collect::<{destination}>()")
                    };
                    let length = if empty {
                        0
                    } else if matches!(kind, "set" | "map") {
                        2
                    } else {
                        3
                    };
                    let check = valid(kind, "result", length, if empty { 0 } else { 62 });
                    cases.push((format!("collect_{kind}_{route}_{empty}_{generic}"),format!(r#"{TYPES}
fn main()->i32{{val result:{destination} = {call};std::debug::assert({check},"collect");print("done");42}}
"#)));
                }
            }
        }
        for route in ["sequence", "view"] {
            let source = if route == "sequence" {
                format!("Sequence{{items:[{contents}]}}")
            } else {
                format!("{{val view:List<{item}> =[{contents}];view}}")
            };
            let check = valid(
                kind,
                "result",
                if matches!(kind, "set" | "map") { 2 } else { 3 },
                62,
            );
            cases.push((format!("direct_{kind}_{route}"),format!(r#"{TYPES}
fn main()->i32{{val result:{destination} = <{destination} as FromIterator<{item}>>::from_iter(source({source}));std::debug::assert({check},"from_iter");print("done");42}}
"#)));
        }
        for enumeration in ["option", "result"] {
            let output = if enumeration == "option" {
                format!("Option<{destination}>")
            } else {
                format!("Result<{destination},String>")
            };
            for outcome in ["empty", "success", "failure"] {
                let items = if outcome == "empty" {
                    String::new()
                } else {
                    let parts: Vec<_> = if kind == "map" {
                        vec![
                            "(Key{value:20},0)",
                            "(Key{value:22},1)",
                            "(Key{value:20},2)",
                        ]
                    } else if kind == "set" {
                        vec!["Key{value:20}", "Key{value:22}", "Key{value:20}"]
                    } else {
                        vec!["20", "22", "20"]
                    };
                    parts
                        .into_iter()
                        .enumerate()
                        .map(|(index, value)| {
                            if outcome == "failure" && index == 1 {
                                if enumeration == "option" {
                                    "None".into()
                                } else {
                                    "Err(\"failure\")".into()
                                }
                            } else {
                                format!(
                                    "{}({value})",
                                    if enumeration == "option" {
                                        "Some"
                                    } else {
                                        "Ok"
                                    }
                                )
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                };
                for direct in [false, true] {
                    let element = if enumeration == "option" {
                        format!("Option<{item}>")
                    } else {
                        format!("Result<{item},String>")
                    };
                    let call = if direct {
                        format!(
                            "<{output} as FromIterator<{element}>>::from_iter(source(Sequence{{items:items}}))"
                        )
                    } else {
                        "source(items.iter()).collect()".into()
                    };
                    let check = if outcome == "failure" {
                        format!(
                            "result.is_{}()",
                            if enumeration == "option" {
                                "none"
                            } else {
                                "err"
                            }
                        )
                    } else {
                        let check = valid(
                            kind,
                            "value",
                            if outcome == "empty" {
                                0
                            } else if matches!(kind, "set" | "map") {
                                2
                            } else {
                                3
                            },
                            if outcome == "empty" { 0 } else { 62 },
                        );
                        format!("result.map_or(false,|value|{check})")
                    };
                    cases.push((format!("fallible_{enumeration}_{kind}_{outcome}_{direct}"),format!(r#"{TYPES}
fn main()->i32{{val items:ArrayList<{element}> =[{items}];val result:{output} = {call};std::debug::assert({check},"fallible");print("done");42}}
"#)));
                }
            }
        }
        for route in ["native", "custom", "lazy"] {
            let iterator = format!(
                "{}<{item}>",
                if route == "custom" { "Counter" } else { "Iter" }
            );
            for selection in ["empty", "mixed", "none"] {
                let source = input(route, if selection == "empty" { "" } else { contents });
                let predicate = if selection == "none" {
                    "false"
                } else if kind == "set" {
                    "item.value==20"
                } else if kind == "map" {
                    "item[0].value==20"
                } else {
                    "item==20"
                };
                let (left, right, total_left, total_right) = match selection {
                    "empty" => (0, 0, 0, 0),
                    "none" => (0, if matches!(kind, "set" | "map") { 2 } else { 3 }, 0, 62),
                    _ => (if matches!(kind, "set" | "map") { 1 } else { 2 }, 1, 40, 22),
                };
                let check = format!(
                    "{} && {}",
                    valid(kind, "result[0]", left, total_left),
                    valid(kind, "result[1]", right, total_right)
                );
                cases.push((format!("partition_{kind}_{route}_{selection}"),format!(r#"{TYPES}
fn main()->i32{{val result:({destination},{destination}) = divided::<{item},{iterator},{destination}>(source({source}),|item|{{print("predicate");{predicate}}});std::debug::assert({check},"partition");print("done");42}}
"#)));
            }
        }
    }
    cases
}
