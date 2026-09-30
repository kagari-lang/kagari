pub(super) const OPERATIONS: &[&str] = &[
    "union",
    "intersection",
    "difference",
    "symmetric_difference",
    "is_subset",
    "is_superset",
    "is_disjoint",
];
pub(super) const TYPES: &str = r#"
struct Key{val id:i32,var visits:i32}
impl PartialEq for Key{fn eq(self,other:Self)->bool{print("eq");self.visits+=1;self.id==other.id}}
impl Eq for Key{}impl Hash for Key{fn hash(self)->i64{print("hash");self.visits+=1;0i64}}
struct Policy<T:PartialEq>{val items:ArrayList<T>,val side:String}
impl<T:PartialEq> Iterable for Policy<T>{type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{print(if self.side=="l"{"l-iter"}else{"r-iter"});self.items.iter().inspect(|item|print(if self.side=="l"{"l-next"}else{"r-next"}))}}
impl<T:PartialEq> Set<T> for Policy<T>{fn len(self)->usize{self.items.len()}fn is_empty(self)->bool{self.items.is_empty()}fn contains(self,value:T)->bool{print(if self.side=="l"{"l-contains"}else{"r-contains"});self.items.contains(value)}}
fn left<T>(source:T)->T{print("left");source}fn right<T>(source:T)->T{print("right");source}
fn union<T:Eq+Hash,S:Set<T>>(a:S,b:Set<T>)->LinkedHashSet<T>{a.union(b)}
fn intersection<T:Eq+Hash,S:Set<T>>(a:S,b:Set<T>)->LinkedHashSet<T>{a.intersection(b)}
fn difference<T:Eq+Hash,S:Set<T>>(a:S,b:Set<T>)->LinkedHashSet<T>{a.difference(b)}
fn symmetric_difference<T:Eq+Hash,S:Set<T>>(a:S,b:Set<T>)->LinkedHashSet<T>{a.symmetric_difference(b)}
fn is_subset<T,S:Set<T>>(a:S,b:Set<T>)->bool{a.is_subset(b)}
fn is_superset<T,S:Set<T>>(a:S,b:Set<T>)->bool{a.is_superset(b)}
fn is_disjoint<T,S:Set<T>>(a:S,b:Set<T>)->bool{a.is_disjoint(b)}
"#;
pub(super) fn cases() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for operation in OPERATIONS {
        for shape in ["scalar", "nominal", "float"] {
            if shape == "float" && !operation.starts_with("is_") {
                continue;
            }
            let item = match shape {
                "scalar" => "i32",
                "nominal" => "Key",
                _ => "f64",
            };
            let key = |i| match shape {
                "scalar" => format!("{i}"),
                "nominal" => format!("Key{{id:{i},visits:0}}"),
                _ => format!("{i}.0"),
            };
            for route in ["native", "proxy", "generic", "dynamic"] {
                if shape == "float" && route == "native" {
                    continue;
                }
                for scenario in ["empty", "all", "overlap", "disjoint", "same"] {
                    let a: Vec<_> = if scenario == "empty" {
                        vec![]
                    } else {
                        vec![20, 21]
                    };
                    let b = match scenario {
                        "empty" => vec![22],
                        "all" => vec![20, 21, 22],
                        "disjoint" => vec![22, 23],
                        "same" => a.clone(),
                        _ => vec![21, 22],
                    };
                    let left_values = a.iter().map(|i| key(*i)).collect::<Vec<_>>().join(",");
                    let right_values = b.iter().map(|i| key(*i)).collect::<Vec<_>>().join(",");
                    let source = if route == "native" {
                        format!("val source:LinkedHashSet<{item}> =LinkedHashSet::from(items);")
                    } else {
                        "val source=Policy{items:items,side:\"l\"};".to_owned()
                    };
                    let other = if scenario == "same" {
                        format!("val other:Set<{item}> =source;")
                    } else {
                        format!(
                            "val other:Set<{item}> =Policy{{items:[{right_values}],side:\"r\"}};"
                        )
                    };
                    let view = if route == "dynamic" {
                        format!("val view:Set<{item}> =source;")
                    } else {
                        String::new()
                    };
                    let call = match route {
                        "generic" => format!("{operation}(left(source),right(other))"),
                        "dynamic" => format!("left(view).{operation}(right(other))"),
                        _ => format!("left(source).{operation}(right(other))"),
                    };
                    let expected = match *operation {
                        "union" => {
                            let mut values = a.clone();
                            values.extend(b.iter().filter(|i| !a.contains(i)).copied());
                            values
                        }
                        "intersection" => a.iter().filter(|i| b.contains(i)).copied().collect(),
                        "difference" => a.iter().filter(|i| !b.contains(i)).copied().collect(),
                        "symmetric_difference" => a
                            .iter()
                            .filter(|i| !b.contains(i))
                            .chain(b.iter().filter(|i| !a.contains(i)))
                            .copied()
                            .collect(),
                        _ => vec![],
                    };
                    let check = if operation.starts_with("is_") {
                        let result = match *operation {
                            "is_subset" => a.iter().all(|i| b.contains(i)),
                            "is_superset" => b.iter().all(|i| a.contains(i)),
                            _ => a.iter().all(|i| !b.contains(i)),
                        };
                        format!(
                            "val result={call};print(\"result\");std::debug::assert(result=={result},\"relation\");"
                        )
                    } else {
                        let assertions = expected
                            .iter()
                            .enumerate()
                            .map(|(i, value)| {
                                format!(
                                    "std::debug::assert({}=={value},\"order\");",
                                    if shape == "nominal" {
                                        format!("entries[{i}usize].id")
                                    } else {
                                        format!("entries[{i}usize]")
                                    }
                                )
                            })
                            .collect::<String>();
                        format!(
                            "val result:LinkedHashSet<{item}> ={call};print(\"result\");std::debug::assert(result.len()=={}usize,\"length\");val entries:ArrayList<{item}> =ArrayList::from(result.to_array());{assertions}",
                            expected.len()
                        )
                    };
                    out.push((format!("{operation}_{shape}_{route}_{scenario}"),format!(r#"{TYPES}
fn main()->i32{{val items:ArrayList<{item}> =[{left_values}];{source}{other}{view}{check}print("done");42}}
"#)));
                }
            }
        }
    }
    out
}
