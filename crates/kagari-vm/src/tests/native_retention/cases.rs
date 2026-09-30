pub(super) const TYPES: &str = r#"
struct Cell{var value:i32}
struct Key{val id:i32,var tag:i32}
impl PartialEq for Key{fn eq(self,other:Self)->bool{print("eq");self.id==other.id}}
impl Eq for Key{}
impl Hash for Key{fn hash(self)->i64{print("hash");0i64}}
fn receiver<T>(x:T)->T{print("receiver");x}
fn wrap<T>(p:fn(T)->bool)->fn(T)->bool{print("callback");p}
fn wrap_map<K,V>(p:fn(K,V)->bool)->fn(K,V)->bool{print("callback");p}
fn array_keep<T>(x:ArrayList<T>,p:fn(T)->bool){x.retain(p);}
fn map_keep<K:Eq+Hash,V>(x:LinkedHashMap<K,V>,p:fn(K,V)->bool){x.retain(p);}
fn set_keep<T:Eq+Hash>(x:LinkedHashSet<T>,p:fn(T)->bool){x.retain(p);}
"#;
pub(super) fn cases() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for kind in [
        "array_scalar",
        "array_heap",
        "map_scalar",
        "map_heap",
        "map_custom",
        "set_scalar",
        "set_custom",
    ] {
        for count in [0, 1, 3] {
            for policy in ["all", "none", "even"] {
                for route in ["closure", "generic", "named"] {
                    let heap = kind.ends_with("heap");
                    let custom = kind.ends_with("custom");
                    let map = kind.starts_with("map");
                    let array = kind.starts_with("array");
                    let item = if custom {
                        "Key"
                    } else if heap && array {
                        "Cell"
                    } else {
                        "i32"
                    };
                    let value = if heap || custom { "Cell" } else { "i32" };
                    let element = |i: i32| {
                        let n = 20 + i;
                        let key = if custom {
                            format!("Key{{id:{n},tag:0}}")
                        } else if heap && array {
                            format!("Cell{{value:{n}}}")
                        } else {
                            n.to_string()
                        };
                        if map {
                            let v = if heap || custom {
                                format!("Cell{{value:{n}}}")
                            } else {
                                (100 + i).to_string()
                            };
                            format!("({key},{v})")
                        } else {
                            key
                        }
                    };
                    let seed_type = if map {
                        format!("({item},{value})")
                    } else {
                        item.to_owned()
                    };
                    let contents = (0..count).map(element).collect::<Vec<_>>().join(",");
                    let source_type = if map {
                        format!("LinkedHashMap<{item},{value}>")
                    } else if array {
                        format!("ArrayList<{item}>")
                    } else {
                        format!("LinkedHashSet<{item}>")
                    };
                    let source = if map {
                        "LinkedHashMap::from(seeds)"
                    } else if array {
                        "seeds"
                    } else {
                        "LinkedHashSet::from(seeds)"
                    };
                    let number = if custom {
                        "key.id"
                    } else if heap && array {
                        "key.value-1"
                    } else {
                        "key"
                    };
                    let keep = match policy {
                        "all" => "true".into(),
                        "none" => "false".into(),
                        _ => format!("({number})%2==0"),
                    };
                    let changes = if custom && map {
                        "key.tag+=1;value.value+=1;"
                    } else if custom {
                        "key.tag+=1;"
                    } else if heap && map {
                        "value.value+=1;"
                    } else if heap {
                        "key.value+=1;"
                    } else {
                        ""
                    };
                    let parameters = if map { "key,value" } else { "key" };
                    let signature = if map {
                        format!("{item},{value}")
                    } else {
                        item.to_owned()
                    };
                    let body = format!("print(\"predicate\");{changes}{keep}");
                    let declaration = if route == "named" {
                        let named_params = if map {
                            format!("key:{item},value:{value}")
                        } else {
                            format!("key:{item}")
                        };
                        format!("fn named_keep({named_params})->bool{{{body}}}")
                    } else {
                        "".into()
                    };
                    let predicate = if route == "named" {
                        format!("|{parameters}|named_keep({parameters})")
                    } else {
                        format!("|{parameters}|{{{body}}}")
                    };
                    let wrapper = if map { "wrap_map" } else { "wrap" };
                    let action = if route == "generic" {
                        let helper = if map {
                            "map_keep"
                        } else if array {
                            "array_keep"
                        } else {
                            "set_keep"
                        };
                        format!("{helper}(selected,predicate);")
                    } else {
                        "selected.retain(predicate);".into()
                    };
                    let expected: Vec<_> = (0..count)
                        .filter(|i| policy == "all" || policy == "even" && i % 2 == 0)
                        .collect();
                    let mut checks = format!(
                        "std::debug::assert(selected.len()=={}usize,\"retained length\");",
                        expected.len()
                    );
                    let output = if array { "selected" } else { "observed" };
                    if !array {
                        checks.push_str(&format!(
                            "val observed:ArrayList<{seed_type}> =selected.iter().collect();"
                        ));
                    }
                    for (slot, i) in expected.iter().enumerate() {
                        let key = if map {
                            format!("{output}[{slot}usize][0]")
                        } else {
                            format!("{output}[{slot}usize]")
                        };
                        let number = if custom {
                            format!("{key}.id")
                        } else if heap && array {
                            format!("{key}.value")
                        } else {
                            key.clone()
                        };
                        let n = 20 + i + if heap && array { 1 } else { 0 };
                        checks.push_str(&format!(
                            "std::debug::assert({number}=={n},\"retained order\");"
                        ));
                        if custom {
                            checks.push_str(&format!(
                                "std::debug::assert({key}.tag==1,\"shared key payload\");"
                            ));
                        }
                        if map {
                            let val = if heap || custom {
                                format!("{output}[{slot}usize][1].value")
                            } else {
                                format!("{output}[{slot}usize][1]")
                            };
                            let n = if heap || custom { 21 + i } else { 100 + i };
                            checks.push_str(&format!(
                                "std::debug::assert({val}=={n},\"shared values\");"
                            ));
                        }
                    }
                    if !array && (custom || heap) {
                        for i in 0..count {
                            if custom {
                                let key = if map {
                                    format!("seeds[{i}usize][0]")
                                } else {
                                    format!("seeds[{i}usize]")
                                };
                                checks.push_str(&format!(
                                    "std::debug::assert({key}.tag==1,\"once per original key\");"
                                ));
                            }
                            if map {
                                checks.push_str(&format!("std::debug::assert(seeds[{i}usize][1].value=={},\"completed payload effects\");",21+i));
                            }
                        }
                    }
                    cases.push((format!("{kind}_{count}_{policy}_{route}"),format!("{TYPES}\n{declaration}\nfn main()->i32{{val seeds:ArrayList<{seed_type}> =[{contents}];val data:{source_type} ={source};val selected=receiver(data);val predicate:fn({signature})->bool ={wrapper}({predicate});{action}print(\"committed\");{checks}print(\"done\");42}}")));
                }
            }
        }
    }
    cases
}
