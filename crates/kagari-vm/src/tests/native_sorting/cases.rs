pub(super) const TYPES: &str = r#"
struct Rank<T>{val key:T,val tag:i32,var visits:i32}
impl<T:PartialEq> PartialEq for Rank<T>{fn eq(self,other:Self)->bool{print("eq");self.visits+=1;self.key==other.key}}
impl<T:Eq> Eq for Rank<T>{}
impl<T:PartialOrd> PartialOrd for Rank<T>{fn partial_cmp(self,other:Self)->Option<Ordering>{self.key.partial_cmp(other.key)}}
impl<T:Ord> Ord for Rank<T>{fn cmp(self,other:Self)->Ordering{print("cmp");self.visits+=1;self.key.cmp(other.key)}}
enum Choice<T>{Empty,Item(T)}
fn receiver<T>(items:ArrayList<T>)->ArrayList<T>{print("receiver");items}
fn comparator<T>(p:fn(T,T)->Ordering)->fn(T,T)->Ordering{print("callback");p}
fn extractor<T,K>(p:fn(T)->K)->fn(T)->K{print("callback");p}
fn sort<T:Ord>(items:ArrayList<T>){items.sort();}
fn sort_by<T>(items:ArrayList<T>,p:fn(T,T)->Ordering){items.sort_by(p);}
fn sort_by_key<T,K:Ord>(items:ArrayList<T>,p:fn(T)->K){items.sort_by_key(p);}
fn dedup<T:PartialEq>(items:ArrayList<T>){items.dedup();}
"#;

pub(super) fn cases() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for mode in ["sort", "sort_by", "sort_by_key", "dedup"] {
        for nominal in [false, true] {
            for count in [0, 1, 3, 5] {
                for generic in [false, true] {
                    let keys = [2, 1, 1, 3, 2];
                    let item = if nominal { "Rank<i32>" } else { "i32" };
                    let contents = keys[..count]
                        .iter()
                        .enumerate()
                        .map(|(i, key)| {
                            if nominal {
                                format!("Rank{{key:{key},tag:{i},visits:0}} ")
                            } else {
                                key.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    let callback = if mode == "sort_by" {
                        let (a, b) = if generic { ("b", "a") } else { ("a", "b") };
                        format!(
                            "val p:fn({item},{item})->Ordering =comparator(|a,b|{{print(\"compare\");{a}.cmp({b})}});"
                        )
                    } else if mode == "sort_by_key" {
                        let key = if nominal {
                            if generic { "-item.key" } else { "item.key" }
                        } else if generic {
                            "-item"
                        } else {
                            "item"
                        };
                        let result = if nominal {
                            format!("item.visits+=1;Rank{{key:{key},tag:item.tag,visits:0}}")
                        } else {
                            key.into()
                        };
                        format!(
                            "val p:fn({item})->{item} =extractor(|item|{{print(\"key\");{result}}});"
                        )
                    } else {
                        String::new()
                    };
                    let arguments = if callback.is_empty() {
                        "values"
                    } else {
                        "values,p"
                    };
                    let action = if generic {
                        format!("{mode}({arguments});")
                    } else {
                        format!(
                            "values.{mode}({});",
                            if callback.is_empty() { "" } else { "p" }
                        )
                    };
                    let mut selected = (0..count).collect::<Vec<_>>();
                    if mode == "dedup" {
                        selected.dedup_by_key(|index| keys[*index]);
                    } else if generic && mode != "sort" {
                        selected.sort_by_key(|index| -keys[*index]);
                    } else {
                        selected.sort_by_key(|index| keys[*index]);
                    }
                    let mut checks = format!(
                        "std::debug::assert(values.len()=={}usize,\"length\");",
                        selected.len()
                    );
                    for (slot, index) in selected.iter().enumerate() {
                        checks.push_str(&if nominal{format!("std::debug::assert(values[{slot}usize].key=={} && values[{slot}usize].tag=={index},\"stable slots\");",keys[*index])}else{format!("std::debug::assert(values[{slot}usize]=={},\"order\");",keys[*index])});
                    }
                    if nominal && mode == "sort_by_key" {
                        for index in 0..count {
                            checks.push_str(&format!(
                                "std::debug::assert(seeds[{index}usize].visits==1,\"key once\");"
                            ));
                        }
                    }
                    if nominal && count > 0 {
                        checks.push_str("values[0usize].visits=42;");
                        checks.push_str(&format!(
                            "std::debug::assert(seeds[{}usize].visits==42,\"shared payload\");",
                            selected[0]
                        ));
                    }
                    let source = format!(
                        "{TYPES}\nfn main()->i32{{val seeds:ArrayList<{item}> =[{contents}];val values=receiver(ArrayList::from(seeds));{callback}{action}print(\"committed\");{checks}print(\"done\");42}}"
                    );
                    cases.push((
                        format!(
                            "{mode}_{}_{}_{}",
                            if nominal { "nominal" } else { "scalar" },
                            count,
                            if generic { "generic" } else { "direct" }
                        ),
                        source,
                    ));
                }
            }
        }
    }
    for shape in ["tuple", "option", "choice"] {
        for count in [0, 1, 3] {
            for generic in [false, true] {
                let item = match shape {
                    "tuple" => "(Rank<i32>,i32)",
                    "option" => "Option<Rank<i32>>",
                    _ => "Choice<Rank<i32>>",
                };
                let contents = [2, 2, 1][..count]
                    .iter()
                    .enumerate()
                    .map(|(i, key)| {
                        let rank = format!("Rank{{key:{key},tag:{i},visits:0}}");
                        match shape {
                            "tuple" => format!("({rank},0)"),
                            "option" => format!("Some({rank})"),
                            _ => format!("Choice::Item({rank})"),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                let action = if generic {
                    "dedup(values);"
                } else {
                    "values.dedup();"
                };
                let length = if count == 3 { 2 } else { count };
                let source = format!(
                    "{TYPES}\nfn main()->i32{{val values:ArrayList<{item}> =receiver([{contents}]);{action}print(\"committed\");std::debug::assert(values.len()=={length}usize,\"composed equality\");print(\"done\");42}}"
                );
                cases.push((
                    format!(
                        "dedup_{shape}_{count}_{}",
                        if generic { "generic" } else { "direct" }
                    ),
                    source,
                ));
            }
        }
    }
    cases
}
