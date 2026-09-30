pub(super) const OPERATIONS: &[&str] = &[
    "get",
    "contains_key",
    "map_insert",
    "map_remove",
    "contains",
    "set_insert",
    "set_remove",
    "get_or_insert_with",
    "update",
];
const TYPES: &str = r#"
struct Key<T>{val id:T,var visits:i32}
impl<T:PartialEq> PartialEq for Key<T>{fn eq(self,other:Self)->bool{print("eq");self.visits+=1;self.id==other.id}}
impl<T:Eq> Eq for Key<T>{}
impl<T:Eq+Hash> Hash for Key<T>{fn hash(self)->i64{print("hash");self.visits+=1;0i64}}
enum Choice<T>{Empty,Item(T)}
fn receiver<T>(value:T)->T{print("receiver");value}
fn query<T>(value:T)->T{print("query");value}
fn payload(value:i32)->i32{print("payload");value}
fn factory(value:fn()->i32)->fn()->i32{print("callback");value}
fn transform(value:fn(Option<i32>)->i32)->fn(Option<i32>)->i32{print("callback");value}
fn map_get<K:Eq+Hash>(value:Map<K,i32>,key:K)->Option<i32>{value.get(key)}
fn map_contains<K:Eq+Hash>(value:Map<K,i32>,key:K)->bool{value.contains_key(key)}
fn map_insert<K:Eq+Hash>(value:MutableMap<K,i32>,key:K,item:i32){value.insert(key,item);}
fn map_remove<K:Eq+Hash>(value:MutableMap<K,i32>,key:K)->Option<i32>{value.remove(key)}
fn set_contains<K:Eq+Hash>(value:Set<K>,key:K)->bool{value.contains(key)}
fn set_insert<K:Eq+Hash>(value:MutableSet<K>,key:K){value.insert(key);}
fn set_remove<K:Eq+Hash>(value:MutableSet<K>,key:K)->bool{value.remove(key)}
fn map_factory<K:Eq+Hash>(value:MutableMap<K,i32>,key:K,p:fn()->i32)->i32{value.get_or_insert_with(key,p)}
fn map_update<K:Eq+Hash>(value:MutableMap<K,i32>,key:K,p:fn(Option<i32>)->i32)->i32{value.update(key,p)}
"#;
fn key(shape: &str, id: i32) -> String {
    let nominal = format!("Key{{id:{id},visits:0}}");
    match shape {
        "scalar" => id.to_string(),
        "nominal" => nominal,
        "tuple" => format!("({nominal},0)"),
        "option" => format!("Some({nominal})"),
        "choice" => format!("Choice::Item({nominal})"),
        "identity" => format!("[{id}]"),
        _ => panic!(),
    }
}
pub(super) fn cases() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for operation in OPERATIONS {
        for shape in ["scalar", "nominal", "tuple", "option", "choice", "identity"] {
            let item = match shape {
                "scalar" => "i32",
                "nominal" => "Key<i32>",
                "tuple" => "(Key<i32>,i32)",
                "option" => "Option<Key<i32>>",
                "choice" => "Choice<Key<i32>>",
                _ => "ArrayList<i32>",
            };
            for scenario in ["empty", "present", "absent"] {
                for route in ["direct", "generic", "dynamic"] {
                    if route != "direct" && shape != "scalar" && shape != "nominal" {
                        continue;
                    }
                    let set = matches!(*operation, "contains" | "set_insert" | "set_remove");
                    let storage = if set {
                        format!("LinkedHashSet<{item}>")
                    } else {
                        format!("LinkedHashMap<{item},i32>")
                    };
                    let count = if scenario == "empty" { 0 } else { 3 };
                    let inserts = (0..count)
                        .map(|index| {
                            if set {
                                format!("items.insert(keys[{index}usize]);")
                            } else {
                                format!("items.insert(keys[{index}usize],{});", 20 + index)
                            }
                        })
                        .collect::<String>();
                    let contents = (20..23)
                        .map(|id| key(shape, id))
                        .collect::<Vec<_>>()
                        .join(",");
                    let input = if shape == "identity" && scenario == "present" {
                        "keys[2usize]".into()
                    } else {
                        key(shape, if scenario == "present" { 22 } else { 42 })
                    };
                    let method = match *operation {
                        "map_insert" | "set_insert" => "insert",
                        "map_remove" | "set_remove" => "remove",
                        other => other,
                    };
                    let argument = match *operation {
                        "map_insert" => format!("query({input}),payload(42)"),
                        "get_or_insert_with" => {
                            format!("query({input}),factory(||{{print(\"factory\");42}})")
                        }
                        "update" => format!(
                            "query({input}),transform(|previous|{{print(\"transform\");previous.unwrap_or(0)+42}})"
                        ),
                        _ => format!("query({input})"),
                    };
                    let function = match *operation {
                        "get" => "map_get",
                        "contains_key" => "map_contains",
                        "contains" => "set_contains",
                        "get_or_insert_with" => "map_factory",
                        "update" => "map_update",
                        other => other,
                    };
                    let interface = if set {
                        if *operation == "contains" {
                            "Set"
                        } else {
                            "MutableSet"
                        }
                    } else if matches!(*operation, "get" | "contains_key") {
                        "Map"
                    } else {
                        "MutableMap"
                    };
                    let interface = if set {
                        format!("{interface}<{item}>")
                    } else {
                        format!("{interface}<{item},i32>")
                    };
                    let call = match route {
                        "direct" => format!("receiver(items).{method}({argument})"),
                        "generic" => format!("{function}(receiver(items),{argument})"),
                        _ => format!("receiver(view).{method}({argument})"),
                    };
                    let present = scenario == "present";
                    let result = match *operation {
                        "get" | "map_remove" => format!(
                            "val result={call};std::debug::assert(result=={},\"previous\");",
                            if present { "Some(22)" } else { "None" }
                        ),
                        "contains_key" | "contains" | "set_remove" => format!(
                            "val result={call};std::debug::assert(result=={present},\"membership\");"
                        ),
                        "get_or_insert_with" => format!(
                            "val result={call};std::debug::assert(result=={},\"factory result\");",
                            if present { 22 } else { 42 }
                        ),
                        "update" => format!(
                            "val result={call};std::debug::assert(result=={},\"transform result\");",
                            if present { 64 } else { 42 }
                        ),
                        _ => format!("{call};"),
                    };
                    let final_len = if matches!(*operation, "map_remove" | "set_remove") && present
                    {
                        2
                    } else if matches!(
                        *operation,
                        "map_insert" | "set_insert" | "get_or_insert_with" | "update"
                    ) && !present
                    {
                        count + 1
                    } else {
                        count
                    };
                    let declaration = if route == "dynamic" {
                        format!("val view:{interface} =items;")
                    } else {
                        String::new()
                    };
                    let constructor = if set {
                        "LinkedHashSet::new()"
                    } else {
                        "LinkedHashMap::new()"
                    };
                    let source = format!(
                        r#"{TYPES}
fn main()->i32{{val keys:ArrayList<{item}> =[{contents}];val items:{storage} ={constructor};{inserts}{declaration}{result}print("committed");std::debug::assert(items.len()=={final_len}usize,"length");print("done");42}}
"#
                    );
                    cases.push((format!("{operation}_{shape}_{scenario}_{route}"), source));
                }
            }
        }
    }
    cases
}
