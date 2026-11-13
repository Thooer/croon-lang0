//! builtin、方法与 croon 的运行时绑定。参数形式读 lang0.subset，不复制声明。
//! 参考：docs/Lang0/Subset.md（清单、异常的消息）；docs/Lang0/README.md（Interface、运行时对象、取）。

use super::*;
use num_bigint::BigInt;
use num_traits::{FromPrimitive, Signed, ToPrimitive, Zero};

pub fn lookup(name: &str) -> Option<Value> {
    if let Some(e) = exc_class(name) {
        return Some(Value::Class(Class::Exc(e)));
    }
    const TYPES: &[&str] = &[
        "str",
        "int",
        "float",
        "bool",
        "list",
        "dict",
        "range",
        "enumerate",
        "zip",
    ];
    const FUNCTIONS: &[&str] = &[
        "len",
        "isinstance",
        "min",
        "max",
        "sorted",
        "any",
        "all",
        "sum",
        "abs",
    ];
    if let Some(n) = TYPES.iter().find(|n| **n == name) {
        return Some(Value::Class(Class::Type(n)));
    }
    FUNCTIONS
        .iter()
        .find(|n| **n == name)
        .map(|n| Value::Builtin(n))
}

pub(super) fn call_name(heap: &Heap, f: &Value) -> String {
    match f {
        Value::Function(f) => f.borrow(heap).code.qualname.clone(),
        Value::Builtin(n) => n.to_string(),
        Value::Class(Class::Type(n) | Class::Exc(n)) => n.to_string(),
        Value::Method(m) => format!("{}.{}", m.borrow(heap).0.type_name(heap), m.borrow(heap).1),
        Value::Unbound(t, n) => format!("{t}.{n}"),
        Value::Tool(t) => t.borrow(heap).name.to_string(),
        _ => f.type_name(heap),
    }
}

pub fn arity(heap: &Heap, name: &str, min: usize, max: usize, got: usize) -> Value {
    let n = if got < min { min } else { max };
    let message = msg::text(
        "expected_count",
        None,
        &[
            ("name", name.into()),
            ("n", n.to_string()),
            ("s", if n == 1 { "" } else { "s" }.into()),
            ("got", got.to_string()),
        ],
    );
    msg::exc(
        heap,
        "TypeError",
        vec![Value::str(
            heap,
            if got > max {
                message.replacen("at least", "at most", 1)
            } else {
                message
            },
        )],
        None,
    )
}
fn named_list(names: &[String]) -> String {
    let names: Vec<_> = names.iter().map(|s| format!("'{s}'")).collect();
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        2 => names.join(" and "),
        _ => format!(
            "{}, and {}",
            names[..names.len() - 1].join(", "),
            names.last().unwrap()
        ),
    }
}
/// missing_args 或 missing_kwonly：一次列出这一类缺的全部参数，照 CPython。
fn missing_error(heap: &Heap, id: &str, name: &str, missing: &[String]) -> Value {
    msg::err(
        heap,
        id,
        None,
        &[
            ("name", name.into()),
            ("n", missing.len().to_string()),
            ("s", if missing.len() == 1 { "" } else { "s" }.into()),
            ("names", named_list(missing)),
        ],
    )
}
pub fn bind_function(
    heap: &Heap,
    f: &Function,
    args: Vec<Value>,
    kw: Vec<(String, Value)>,
) -> R<Vec<Value>> {
    let name = &f.code.qualname;
    let params = &f.code.params;
    let mut out = vec![None; params.len()];
    for (i, v) in args.iter().take(params.len()).enumerate() {
        out[i] = Some(v.clone())
    }
    for (k, v) in kw {
        let Some(i) = params.iter().position(|n| n == &k) else {
            return Err(msg::err(
                heap,
                "unexpected_kwarg",
                None,
                &[("name", name.clone()), ("key", k)],
            ));
        };
        if out[i].is_some() {
            return Err(msg::err(
                heap,
                "multiple_values",
                None,
                &[("name", name.clone()), ("key", k)],
            ));
        }
        out[i] = Some(v);
    }
    let required = params.len() - f.defaults.len();
    if args.len() > params.len() {
        return Err(msg::err(
            heap,
            "too_many_args",
            None,
            &[
                ("name", name.clone()),
                (
                    "n",
                    if required == params.len() {
                        params.len().to_string()
                    } else {
                        format!("from {required} to {}", params.len())
                    },
                ),
                ("s", if params.len() == 1 { "" } else { "s" }.into()),
                (
                    "given",
                    format!(
                        "{} {}",
                        args.len(),
                        if args.len() == 1 { "was" } else { "were" }
                    ),
                ),
            ],
        ));
    }
    let missing: Vec<_> = (0..required)
        .filter(|i| out[*i].is_none())
        .map(|i| params[i].clone())
        .collect();
    if !missing.is_empty() {
        return Err(missing_error(heap, "missing_args", name, &missing));
    }
    for (i, v) in f.defaults.iter().enumerate() {
        if out[required + i].is_none() {
            out[required + i] = Some(v.clone())
        }
    }
    Ok(out.into_iter().map(Option::unwrap).collect())
}

pub fn exception_matches(heap: &Heap, e: &Value, typ: &Value) -> R<bool> {
    let Value::Exc(e) = e else { return Ok(false) };
    match typ {
        Value::Class(Class::Exc(c)) => Ok(is_subclass(e.borrow(heap).class, c)),
        Value::Tuple(ts) => {
            for t in ts.iter() {
                if exception_matches(heap, &Value::Exc(*e), t)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Err(msg::err(heap, "catch_non_exception", None, &[])),
    }
}

fn integer(heap: &Heap, v: &Value) -> R<BigInt> {
    v.as_int()
        .ok_or_else(|| msg::err(heap, "not_integer", None, &[("type", v.type_name(heap))]))
}
fn string(heap: &Heap, v: &Value) -> R<String> {
    if let Value::Str(s) = v {
        Ok(s.to_string())
    } else {
        Err(msg::err(
            heap,
            "str_argument",
            Some("index"),
            &[("type", v.type_name(heap))],
        ))
    }
}
/// 方法的 start、end 截到 `[0, len]`；`receiver` 选 bad_slice_index 的措辞，是 list 或 str。
fn position(heap: &Heap, n: &Value, len: usize, receiver: &str) -> R<usize> {
    let n = n
        .as_int()
        .ok_or_else(|| msg::err(heap, "bad_slice_index", Some(receiver), &[]))?;
    let len = BigInt::from(len);
    Ok(if n < Zero::zero() {
        (n + &len).max(Zero::zero())
    } else {
        n.min(len)
    }
    .to_usize()
    .expect("截到 [0, len] 的数放得进 usize"))
}

impl Vm {
    pub(super) fn attr(&self, v: &Value, name: &str) -> R<Value> {
        let data = match v {
            Value::Module(m) => {
                if let Some(v) = self.globals.get(m.as_ref()).and_then(|g| g.get(name)) {
                    return Ok(v.clone());
                }
                let child = format!("{m}.{name}");
                if self.loaded.contains(&child) {
                    return Ok(Value::Module(child.into()));
                }
                if m.as_ref() == "croon" {
                    if let Some(c) = exc_class(name) {
                        return Ok(Value::Class(Class::Exc(c)));
                    }
                    const NAMES: &[&str] = &[
                        "receive",
                        "tool",
                        "send",
                        "deliver",
                        "read_json",
                        "read_text",
                        "definition",
                        "seat",
                        "roster",
                        "kind",
                        "data",
                        "Port",
                        "Schema",
                        "Inbox",
                    ];
                    if let Some(n) = NAMES.iter().find(|n| **n == name) {
                        return Ok(Value::Builtin(n));
                    }
                }
                if m.as_ref() == "typing" {
                    const NAMES: &[&str] = &[
                        "TypedDict",
                        "NotRequired",
                        "Literal",
                        "Any",
                        "Annotated",
                        "Callable",
                    ];
                    if let Some(n) = NAMES.iter().find(|n| **n == name) {
                        return Ok(Value::Form(Rc::new(Form::Typing(n))));
                    }
                }
                None
            }
            Value::Receipt(r) => match name {
                "value" => Some(r.borrow(&self.heap).value.clone()),
                "dependencies" => Some(bridge::decode(
                    &self.heap,
                    &r.borrow(&self.heap).dependencies,
                )),
                "attach" => Some(Value::Proxy(r.borrow(&self.heap).attach)),
                "derived" => Some(Value::Proxy(r.borrow(&self.heap).derived)),
                _ => None,
            },
            Value::Arrival(a) => match name {
                "port" => Some(a.borrow(&self.heap).port.clone()),
                "sender" => Some(Value::Str(a.borrow(&self.heap).sender.clone())),
                "via" => Some(Value::Str(a.borrow(&self.heap).via.clone())),
                "value" => Some(a.borrow(&self.heap).value.clone()),
                "dependencies" => Some(bridge::decode(
                    &self.heap,
                    &a.borrow(&self.heap).dependencies,
                )),
                "attach" => Some(Value::Proxy(a.borrow(&self.heap).attach)),
                _ => None,
            },
            Value::Derived(d) => match name {
                "handle" => Some(Value::Str(d.borrow(&self.heap).handle.clone())),
                "revoker" => Some(d.borrow(&self.heap).revoker.clone()),
                _ => None,
            },
            Value::Tool(t) => match name {
                "name" => Some(Value::Str(t.borrow(&self.heap).name.clone())),
                "spec" => Some(bridge::decode(&self.heap, &t.borrow(&self.heap).spec)),
                _ => None,
            },
            Value::Class(Class::Type(t))
                if self.program.subset["methods"][*t].get(name).is_some() =>
            {
                return Ok(Value::Unbound(t, name.into()));
            }
            _ => None,
        };
        if let Some(v) = data {
            return Ok(v);
        }
        let typ = v.type_name(&self.heap);
        if self.program.subset["methods"][&typ].get(name).is_some()
            || matches!(v, Value::Proxy(_)) && name == "get"
        {
            return Ok(Value::Method(self.heap.alloc((v.clone(), name.into()))));
        }
        Err(msg::err(
            &self.heap,
            "no_attribute",
            None,
            &[("type", typ), ("name", name.into())],
        ))
    }

    /// 把映射 `v` 并进 `d`。`call` 是调用里 `f(**v)` 时被调用者的名字：
    /// 这时不是映射报 star_star_not_mapping，键重复报 multiple_kw_values。
    pub(super) fn merge_dict(&mut self, d: Slot<Dict>, v: &Value, call: Option<&str>) -> R<()> {
        let items: Vec<_> = match v {
            Value::Dict(v) | Value::Proxy(v) => v.borrow(&self.heap).items().cloned().collect(),
            _ => {
                let typ = v.type_name(&self.heap);
                return Err(match call {
                    Some(name) => msg::err(
                        &self.heap,
                        "star_star_not_mapping",
                        None,
                        &[("name", name.into()), ("type", typ)],
                    ),
                    None => msg::err(&self.heap, "not_mapping", None, &[("type", typ)]),
                });
            }
        };
        self.meter.charge(&self.heap, items.len() as u64)?;
        for (k, v) in items {
            let key = ops::key(&self.heap, &k, self.meter.depth)?;
            if let Some(name) = call
                && d.borrow(&self.heap).get(&key).is_some()
            {
                let Value::Str(k) = &k else {
                    return Err(msg::err(&self.heap, "keywords_not_str", None, &[]));
                };
                return Err(msg::err(
                    &self.heap,
                    "multiple_kw_values",
                    None,
                    &[("name", name.into()), ("key", k.to_string())],
                ));
            }
            d.insert(&self.heap, key, k, v)
        }
        Ok(())
    }

    fn iterable(&mut self, v: &Value) -> R<Vec<Value>> {
        let mut out = vec![];
        ops::collect(&self.heap, v, &mut out, &mut self.meter)?;
        Ok(out)
    }
    fn dict_update(&mut self, d: Slot<Dict>, v: &Value) -> R<()> {
        if matches!(v, Value::Dict(_) | Value::Proxy(_)) {
            return self.merge_dict(d, v, None);
        }
        let entries = self.iterable(v)?;
        for (i, v) in entries.iter().enumerate() {
            let xs = self.iterable(v).map_err(|_| {
                msg::err(&self.heap, "dict_sequence", None, &[("i", i.to_string())])
            })?;
            if xs.len() != 2 {
                return Err(msg::err(
                    &self.heap,
                    "dict_sequence_length",
                    None,
                    &[("i", i.to_string()), ("n", xs.len().to_string())],
                ));
            }
            d.insert(
                &self.heap,
                ops::key(&self.heap, &xs[0], self.meter.depth)?,
                xs[0].clone(),
                xs[1].clone(),
            );
        }
        Ok(())
    }

    pub(super) fn builtin(
        &mut self,
        f: Value,
        args: Vec<Value>,
        kw: Vec<(String, Value)>,
        read: &dyn Read,
    ) -> R<Option<Value>> {
        let name = match &f {
            Value::Builtin(n) => *n,
            Value::Class(Class::Type(n)) => *n,
            Value::Class(Class::Exc(c)) => {
                if !kw.is_empty() {
                    return Err(msg::err(
                        &self.heap,
                        "unexpected_kwarg",
                        None,
                        &[("name", c.to_string()), ("key", kw[0].0.clone())],
                    ));
                }
                return Ok(Some(msg::exc(&self.heap, c, args, None)));
            }
            Value::Method(m) => {
                let m = m.borrow(&self.heap).clone();
                return self.method(&m.0, &m.1, args, kw).map(Some);
            }
            Value::Unbound(typ, n) => {
                if args.is_empty() {
                    return Err(arity(&self.heap, n, 1, usize::MAX, 0));
                }
                if args[0].type_name(&self.heap) != *typ {
                    return Err(msg::err(
                        &self.heap,
                        "descriptor_receiver",
                        None,
                        &[
                            ("name", n.to_string()),
                            ("owner", typ.to_string()),
                            ("type", args[0].type_name(&self.heap)),
                        ],
                    ));
                }
                return self.method(&args[0], n, args[1..].to_vec(), kw).map(Some);
            }
            _ => {
                return Err(msg::err(
                    &self.heap,
                    "not_callable",
                    None,
                    &[("type", f.type_name(&self.heap))],
                ));
            }
        };
        self.meter.charge(&self.heap, 1)?;
        if matches!(name, "send" | "deliver" | "read_json" | "read_text") {
            if self.birth.is_none() {
                return Err(msg::err(
                    &self.heap,
                    "read_at_top",
                    None,
                    &[("name", name.into())],
                ));
            }
            let a = self.bind_builtin(name, None, args, kw)?;
            if name.starts_with("read_") {
                let Value::Str(id) = &a["id"] else {
                    return Err(self.delivery_argument(name, "ID 用字符串"));
                };
                let fetched = if name == "read_json" {
                    read.json(id, &self.consumed_keys)
                } else {
                    read.bytes(id, &self.consumed_keys).map(|bytes| {
                        bytes
                            .and_then(|bytes| String::from_utf8(bytes).ok())
                            .map(Json::String)
                    })
                };
                return match fetched {
                    Ok(Some(value)) => {
                        self.meter
                            .charge(&self.heap, super::heap::json_size(&value))?;
                        Ok(Some(bridge::decode(&self.heap, &value)))
                    }
                    Ok(None) => Err(msg::err(
                        &self.heap,
                        "content_read",
                        None,
                        &[("name", name.into()), ("id", id.to_string())],
                    )),
                    Err(error) => {
                        self.host_error = Some(error);
                        Ok(None)
                    }
                };
            }
            let dependencies =
                bridge::encode(&self.heap, &a["dependencies"], "value", &mut self.meter)?;
            if !dependencies
                .as_array()
                .is_some_and(|ids| ids.iter().all(Json::is_string))
            {
                return Err(self.delivery_argument(name, "dependencies 用 Bundle ID 的 list"));
            }
            if name == "deliver" {
                return Ok(Some(Value::Delivery(Rc::new(Delivery {
                    value: a["value"].clone(),
                    dependencies,
                }))));
            }
            let Value::Ref(r) = &a["ref"] else {
                return Err(self.delivery_argument(name, "目标用引用值"));
            };
            let Value::Dict(attachments) = &a["attach"] else {
                return Err(self.delivery_argument(name, "attach 用名字到引用值的 dict"));
            };
            let value = bridge::encode(&self.heap, &a["value"], "value", &mut self.meter)?;
            let mut attach = serde_json::Map::new();
            for (key, value) in attachments.borrow(&self.heap).items() {
                let (Value::Str(key), Value::Ref(reference)) = (key, value) else {
                    return Err(msg::err(&self.heap, "croon_argument", Some("attach"), &[]));
                };
                attach.insert(key.to_string(), reference.borrow(&self.heap).target.clone());
            }
            let reference = r.borrow(&self.heap).clone();
            let Spec::Inline(own) = reference.spec;
            self.atom(json!({"send":{"to":reference.target,"delivery":{"value":value,"dependencies":dependencies},"attach":attach}}), PendingKind::Send(own));
            return Ok(None);
        }
        if name == "receive" {
            if self.birth.is_none() {
                return Err(msg::err(
                    &self.heap,
                    "read_at_top",
                    None,
                    &[("name", name.into())],
                ));
            }
            if args.len() != 1 {
                return Err(arity(&self.heap, name, 1, 1, args.len()));
            }
            let ports = match &args[0] {
                Value::List(xs) => xs.borrow(&self.heap).clone(),
                x => vec![x.clone()],
            };
            let mut names = vec![];
            for p in ports {
                let Value::Inbox(n) = p else {
                    return Err(msg::err(&self.heap, "croon_argument", Some("receive"), &[]));
                };
                names.push(n.to_string())
            }
            let mut receive = json!({"ports":names});
            for (k, v) in kw {
                match k.as_str() {
                    "via" | "sender" => {
                        let vs = match v {
                            Value::List(_) => v,
                            x => Value::list(&self.heap, vec![x]),
                        };
                        receive[&k] = bridge::encode(&self.heap, &vs, "value", &mut self.meter)?
                    }
                    "timeout" => {
                        receive[&k] = bridge::encode(&self.heap, &v, "value", &mut self.meter)?
                    }
                    _ => {
                        return Err(msg::err(
                            &self.heap,
                            "unexpected_kwarg",
                            None,
                            &[("name", name.into()), ("key", k)],
                        ));
                    }
                }
            }
            self.atom(json!({"receive":receive}), PendingKind::Receive);
            return Ok(None);
        }
        if matches!(name, "definition" | "seat" | "roster") {
            if !args.is_empty() || !kw.is_empty() {
                return Err(arity(&self.heap, name, 0, 0, args.len()));
            }
            let Some(b) = &self.birth else {
                return Err(msg::err(
                    &self.heap,
                    "read_at_top",
                    None,
                    &[("name", name.into())],
                ));
            };
            return Ok(Some(bridge::decode(&self.heap, &b[name])));
        }
        if name == "tool" {
            if args.len() != 1 || !kw.is_empty() {
                return Err(arity(&self.heap, name, 1, 1, args.len()));
            }
            let Value::Function(f) = &args[0] else {
                return Err(msg::err(&self.heap, "croon_argument", Some("tool"), &[]));
            };
            let code = f.borrow(&self.heap).code.clone();
            let def = code.def.as_ref().expect("tool 的函数有 AST");
            let (spec, output) =
                crate::infer::tool(&self.program, &f.borrow(&self.heap).code.module, def).map_err(
                    |m| {
                        msg::exc(
                            &self.heap,
                            "TypeError",
                            vec![Value::str(&self.heap, m)],
                            None,
                        )
                    },
                )?;
            let t = Tool {
                name: f.borrow(&self.heap).code.name.as_str().into(),
                input: spec["input"].clone(),
                output: Some(output),
                spec,
                f: *f,
            };
            return Ok(Some(Value::Tool(self.heap.alloc(t))));
        }
        if name == "kind" || name == "data" {
            if args.len() != 1 || !kw.is_empty() {
                return Err(arity(&self.heap, name, 1, 1, args.len()));
            }
            let Value::Exc(e) = &args[0] else {
                return Err(msg::err(
                    &self.heap,
                    "croon_argument",
                    Some("exception"),
                    &[("name", name.into())],
                ));
            };
            return Ok(Some(if name == "data" {
                bridge::decode(&self.heap, &e.borrow(&self.heap).data)
            } else {
                Value::str(&self.heap, e.borrow(&self.heap).kind())
            }));
        }
        let a = self.bind_builtin(name, None, args, kw)?;
        let get = |key: &str| {
            a.get(key)
                .cloned()
                .unwrap_or_else(|| panic!("调用形式绑上了 {key}"))
        };
        let v = match name {
            "len" => {
                let v = get("obj");
                Value::int(
                    &self.heap,
                    match &v {
                        Value::Str(s) => s.chars().count(),
                        Value::List(x) => x.borrow(&self.heap).len(),
                        Value::Tuple(x) => x.len(),
                        Value::Dict(x) => x.borrow(&self.heap).len(),
                        Value::Proxy(x) => x.borrow(&self.heap).len(),
                        Value::View(_, x) => x.borrow(&self.heap).len(),
                        Value::Range(r) => {
                            let n = r
                                .len()
                                .to_i64()
                                .ok_or_else(|| msg::err(&self.heap, "ssize_t", None, &[]))?;
                            return Ok(Some(Value::int(&self.heap, n)));
                        }
                        _ => {
                            return Err(msg::err(
                                &self.heap,
                                "no_len",
                                None,
                                &[("type", v.type_name(&self.heap))],
                            ));
                        }
                    },
                )
            }
            "str" => Value::str(
                &self.heap,
                text::metered_str(&self.heap, &get("object"), &mut self.meter)?,
            ),
            "bool" => Value::Bool(get("x").truthy(&self.heap)),
            "int" => {
                let x = get("x");
                Value::int(
                    &self.heap,
                    match &x {
                        Value::Bool(_) | Value::Int(_) => x.as_int().unwrap(),
                        Value::Str(s) => text::parse_int(&self.heap, s)?,
                        Value::Float(x) => BigInt::from_f64(*x).ok_or_else(|| {
                            if x.is_nan() {
                                msg::err(&self.heap, "float_nan_to_int", None, &[])
                            } else {
                                msg::err(&self.heap, "float_to_int", None, &[])
                            }
                        })?,
                        _ => {
                            return Err(msg::err(
                                &self.heap,
                                "int_argument",
                                None,
                                &[("type", x.type_name(&self.heap))],
                            ));
                        }
                    },
                )
            }
            "float" => {
                let x = get("x");
                Value::Float(match &x {
                    Value::Bool(_) | Value::Int(_) => ops::float_of(&self.heap, &num(&x).unwrap())?,
                    Value::Float(x) => *x,
                    Value::Str(s) => text::parse_float(&self.heap, s)?,
                    _ => {
                        return Err(msg::err(
                            &self.heap,
                            "float_argument",
                            None,
                            &[("type", x.type_name(&self.heap))],
                        ));
                    }
                })
            }
            "list" => {
                let xs = self.iterable(&get("iterable"))?;
                Value::list(&self.heap, xs)
            }
            "dict" => {
                let d = self.heap.alloc(Dict::default());
                self.dict_update(d, &get("mapping_or_iterable"))?;
                self.merge_dict(d, &get("kwargs"), None)?;
                Value::Dict(d)
            }
            "range" => {
                let start = a
                    .get("start")
                    .map(|v| integer(&self.heap, v))
                    .transpose()?
                    .unwrap_or_default();
                let stop = integer(&self.heap, &get("stop"))?;
                let step = a
                    .get("step")
                    .map(|v| integer(&self.heap, v))
                    .transpose()?
                    .unwrap_or_else(|| 1.into());
                if step.is_zero() {
                    return Err(msg::err(&self.heap, "range_step_zero", None, &[]));
                }
                Value::Range(Rc::new(RangeV { start, stop, step }))
            }
            "enumerate" => {
                let it = ops::iter(&self.heap, &get("iterable"))?;
                Value::Iter(
                    self.heap
                        .alloc(Iter::Enumerate(it, integer(&self.heap, &get("start"))?)),
                )
            }
            "zip" => {
                let Value::Tuple(xs) = get("iterables") else {
                    unreachable!("zip 的 *iterables 绑成 tuple")
                };
                let mut its = vec![];
                for x in xs.iter() {
                    its.push(ops::iter(&self.heap, x)?)
                }
                Value::Iter(
                    self.heap
                        .alloc(Iter::Zip(its, get("strict").truthy(&self.heap))),
                )
            }
            "isinstance" => Value::Bool(instance_of(
                &self.heap,
                &get("obj"),
                &get("class_or_tuple"),
            )?),
            "abs" => {
                let v = get("x");
                match num(&v) {
                    Some(Num::Int(i)) => Value::int(&self.heap, i.abs()),
                    Some(Num::Float(x)) => Value::Float(x.abs()),
                    None => {
                        return Err(msg::err(
                            &self.heap,
                            "bad_abs",
                            None,
                            &[("type", v.type_name(&self.heap))],
                        ));
                    }
                }
            }
            "any" | "all" => {
                let it = ops::iter(&self.heap, &get("iterable"))?;
                let want = name == "any";
                let mut answer = !want;
                while let Some(v) = ops::next_any(&self.heap, &it)? {
                    self.meter.charge(&self.heap, 1)?;
                    if v.truthy(&self.heap) == want {
                        answer = want;
                        break;
                    }
                }
                Value::Bool(answer)
            }
            "sum" => {
                let mut total = get("start");
                if matches!(total, Value::Str(_)) {
                    return Err(msg::err(&self.heap, "sum_strings", None, &[]));
                }
                let it = ops::iter(&self.heap, &get("iterable"))?;
                while let Some(v) = ops::next_any(&self.heap, &it)? {
                    total = ops::binary(
                        &self.heap,
                        &total,
                        &v,
                        compile::Bin::Add,
                        false,
                        &mut self.meter,
                    )?
                }
                total
            }
            "sorted" | "min" | "max" => {
                let xs = if a.contains_key("iterable") {
                    self.iterable(&get("iterable"))?
                } else {
                    let mut xs = vec![get("arg1"), get("arg2")];
                    if let Value::Tuple(rest) = get("args") {
                        xs.extend(rest.iter().cloned())
                    }
                    xs
                };
                if xs.is_empty() {
                    if name == "sorted" {
                        Value::list(&self.heap, vec![])
                    } else {
                        a.get("default").cloned().ok_or_else(|| {
                            msg::err(&self.heap, "empty_min_max", None, &[("name", name.into())])
                        })?
                    }
                } else if matches!(get("key"), Value::None) {
                    let keys = xs.clone();
                    // 只有 sorted 的形式有 reverse。
                    self.select_values(
                        name,
                        xs,
                        keys,
                        a.get("reverse").is_some_and(|r| r.truthy(&self.heap)),
                    )?
                } else {
                    self.selections.push(Selection {
                        owner: self.frames.len(),
                        name: name.into(),
                        items: xs,
                        keys: vec![],
                        key: get("key"),
                        reverse: a.get("reverse").is_some_and(|r| r.truthy(&self.heap)),
                        awaiting: false,
                    });
                    return Ok(None);
                }
            }
            _ => {
                return Err(msg::err(
                    &self.heap,
                    "declaration_only",
                    None,
                    &[("name", name.into())],
                ));
            }
        };
        Ok(Some(v))
    }

    fn delivery_argument(&self, name: &str, reason: &str) -> Value {
        msg::err(
            &self.heap,
            "delivery_argument",
            None,
            &[("name", name.into()), ("reason", reason.into())],
        )
    }

    fn bind_builtin(
        &self,
        name: &str,
        typ: Option<&str>,
        args: Vec<Value>,
        kw: Vec<(String, Value)>,
    ) -> R<BTreeMap<String, Value>> {
        let forms = if typ.is_none() && self.program.subset["croon_calls"].get(name).is_some() {
            &self.program.subset["croon_calls"][name]
        } else if let Some(t) = typ {
            &self.program.subset["methods"][t][name]
        } else {
            &self.program.subset["builtins"][name]
        };
        let label = typ
            .map(|t| format!("{t}.{name}"))
            .unwrap_or_else(|| name.into());
        let forms: Vec<_> = forms
            .as_array()
            .expect("声明有调用形式")
            .iter()
            .map(|f| FormArgs::parse(&self.heap, f.as_str().expect("调用形式是字符串")))
            .collect();
        let mut first = None;
        for form in &forms {
            match form.bind(&self.heap, &label, &args, &kw) {
                Ok(v) => return Ok(v),
                Err(e) if first.is_none() && (forms.len() == 1 || form.count_fits(args.len())) => {
                    first = Some(e)
                }
                Err(_) => {}
            }
        }
        Err(first.unwrap_or_else(|| {
            let min = forms.iter().map(|f| f.min).min().expect("至少一种形式");
            let max = forms.iter().map(FormArgs::max).max().expect("至少一种形式");
            arity(&self.heap, &label, min, max, args.len())
        }))
    }

    fn method(
        &mut self,
        obj: &Value,
        name: &str,
        args: Vec<Value>,
        kw: Vec<(String, Value)>,
    ) -> R<Value> {
        let typ = if matches!(obj, Value::Proxy(_)) {
            "dict".into()
        } else {
            obj.type_name(&self.heap)
        };
        let a = self.bind_builtin(name, Some(&typ), args, kw)?;
        let get = |k: &str| {
            a.get(k)
                .cloned()
                .unwrap_or_else(|| panic!("调用形式绑上了 {k}"))
        };
        self.meter.charge(&self.heap, 1)?;
        match obj {
            Value::Dict(d) => match name {
                "get" | "setdefault" | "pop" => {
                    let key = get("key");
                    let kk = ops::key(&self.heap, &key, self.meter.depth)?;
                    let present = d.borrow(&self.heap).get(&kk).cloned();
                    if name == "pop" {
                        return d
                            .borrow_mut(&self.heap)
                            .remove(&kk)
                            .or_else(|| a.get("default").cloned())
                            .ok_or_else(|| msg::key_error(&self.heap, key));
                    }
                    if let Some(v) = present {
                        return Ok(v);
                    }
                    let default = get("default");
                    if name == "setdefault" {
                        d.insert(&self.heap, kk, key, default.clone())
                    }
                    Ok(default)
                }
                "keys" | "values" | "items" => Ok(Value::View(
                    match name {
                        "keys" => View::Keys,
                        "values" => View::Values,
                        _ => View::Items,
                    },
                    *d,
                )),
                "clear" => {
                    d.borrow_mut(&self.heap).clear();
                    Ok(Value::None)
                }
                "update" => {
                    self.dict_update(*d, &get("other"))?;
                    self.merge_dict(*d, &get("kwargs"), None)?;
                    Ok(Value::None)
                }
                _ => unreachable!("bind_builtin 只绑得上 lang0.subset 里 dict 的方法"),
            },
            Value::Proxy(d) => {
                let k = ops::key(&self.heap, &get("key"), self.meter.depth)?;
                Ok(d.borrow(&self.heap)
                    .get(&k)
                    .cloned()
                    .unwrap_or_else(|| get("default")))
            }
            Value::List(xs) => match name {
                "append" => {
                    xs.push(&self.heap, get("object"));
                    Ok(Value::None)
                }
                "extend" => {
                    let more = self.iterable(&get("iterable"))?;
                    xs.extend(&self.heap, more);
                    Ok(Value::None)
                }
                "clear" => {
                    xs.borrow_mut(&self.heap).clear();
                    Ok(Value::None)
                }
                "pop" => {
                    let i = integer(&self.heap, &get("index"))?
                        .to_i64()
                        .ok_or_else(|| msg::err(&self.heap, "ssize_t", None, &[]))?;
                    let n = xs.borrow(&self.heap).len() as i64;
                    if n == 0 {
                        return Err(msg::err(&self.heap, "pop_empty", None, &[]));
                    }
                    let i = if i < 0 { n + i } else { i };
                    if i < 0 || i >= n {
                        return Err(msg::err(&self.heap, "pop_out_of_range", None, &[]));
                    }
                    Ok(xs.borrow_mut(&self.heap).remove(i as usize))
                }
                "index" => {
                    let items = xs.borrow(&self.heap).clone();
                    self.seq_index(&items, &get("value"), &get("start"), &get("stop"), "list")
                }
                _ => unreachable!("bind_builtin 只绑得上 lang0.subset 里 list 的方法"),
            },
            Value::Tuple(xs) => {
                self.seq_index(xs, &get("value"), &get("start"), &get("stop"), "tuple")
            }
            Value::Str(s) => self.string_method(s, name, a),
            _ => unreachable!("lang0.subset 只给 dict、list、tuple、str 声明方法"),
        }
    }
    fn seq_index(
        &mut self,
        xs: &[Value],
        value: &Value,
        start: &Value,
        stop: &Value,
        typ: &str,
    ) -> R<Value> {
        let start = position(&self.heap, start, xs.len(), "list")?;
        let stop = position(&self.heap, stop, xs.len(), "list")?;
        for (i, v) in xs.iter().enumerate().take(stop).skip(start) {
            self.meter.charge(&self.heap, 1)?;
            if ops::same(&self.heap, v, value) || ops::eq(&self.heap, v, value, self.meter.depth)? {
                return Ok(Value::int(&self.heap, i));
            }
        }
        Err(msg::err(
            &self.heap,
            "index_not_found",
            Some(typ),
            &[("repr", text::repr(&self.heap, value, self.meter.depth)?)],
        ))
    }
    fn string_method(&mut self, s: &str, name: &str, a: BTreeMap<String, Value>) -> R<Value> {
        let get = |k: &str| {
            a.get(k)
                .cloned()
                .unwrap_or_else(|| panic!("调用形式绑上了 {k}"))
        };
        self.meter.charge(&self.heap, s.len() as u64)?;
        match name {
            "join" => {
                let iterable = get("iterable");
                if ops::iter(&self.heap, &iterable).is_err() {
                    return Err(msg::err(&self.heap, "join_not_iterable", None, &[]));
                }
                let values = self.iterable(&iterable)?;
                let mut out = String::new();
                for (i, v) in values.iter().enumerate() {
                    let Value::Str(v) = v else {
                        if text::carries_ref(&self.heap, v) {
                            return Err(msg::err(&self.heap, "ref_text", None, &[]));
                        }
                        return Err(msg::err(
                            &self.heap,
                            "join_item",
                            None,
                            &[("i", i.to_string()), ("type", v.type_name(&self.heap))],
                        ));
                    };
                    self.meter.charge(&self.heap, (v.len() + s.len()) as u64)?;
                    if i > 0 {
                        out.push_str(s)
                    }
                    out.push_str(v)
                }
                Ok(Value::str(&self.heap, out))
            }
            "strip" => {
                let chars = get("chars");
                let trim = |c: char| match &chars {
                    Value::None => text::is_space(c),
                    Value::Str(s) => s.contains(c),
                    _ => false,
                };
                if !matches!(chars, Value::None | Value::Str(_)) {
                    return Err(msg::err(&self.heap, "str_argument", Some("strip"), &[]));
                }
                Ok(Value::str(&self.heap, s.trim_matches(trim)))
            }
            "replace" => {
                let old = string(&self.heap, &get("old"))?;
                let new = string(&self.heap, &get("new"))?;
                let n = integer(&self.heap, &get("count"))?
                    .to_i64()
                    .ok_or_else(|| msg::err(&self.heap, "ssize_t", None, &[]))?;
                let n = if n < 0 {
                    usize::MAX
                } else {
                    usize::try_from(n).unwrap_or(usize::MAX)
                };
                let count = if old.is_empty() {
                    s.chars().count() + 1
                } else {
                    s.matches(&old).count()
                }
                .min(n);
                self.meter.charge(
                    &self.heap,
                    s.len().saturating_add(count.saturating_mul(new.len())) as u64,
                )?;
                Ok(Value::str(&self.heap, s.replacen(&old, &new, n)))
            }
            "split" => {
                let sep = get("sep");
                let max = integer(&self.heap, &get("maxsplit"))?
                    .to_i64()
                    .ok_or_else(|| msg::err(&self.heap, "ssize_t", None, &[]))?;
                let max = if max < 0 {
                    usize::MAX
                } else {
                    usize::try_from(max).unwrap_or(usize::MAX)
                };
                let parts: Vec<String> = match sep {
                    Value::Str(sep) => {
                        if sep.is_empty() {
                            return Err(msg::err(&self.heap, "empty_separator", None, &[]));
                        }
                        s.splitn(max.saturating_add(1), sep.as_ref())
                            .map(str::to_owned)
                            .collect()
                    }
                    Value::None => {
                        let mut rest = s.trim_start_matches(text::is_space);
                        let mut parts = vec![];
                        while !rest.is_empty() {
                            if parts.len() == max {
                                parts.push(rest.into());
                                break;
                            }
                            let end = rest.find(text::is_space).unwrap_or(rest.len());
                            parts.push(rest[..end].into());
                            rest = rest[end..].trim_start_matches(text::is_space);
                        }
                        parts
                    }
                    other => {
                        return Err(msg::err(
                            &self.heap,
                            "str_argument",
                            Some("split"),
                            &[("type", other.type_name(&self.heap))],
                        ));
                    }
                };
                Ok(Value::list(
                    &self.heap,
                    parts
                        .into_iter()
                        .map(|s| Value::str(&self.heap, s))
                        .collect(),
                ))
            }
            "index" | "startswith" | "endswith" => {
                let chars: Vec<char> = s.chars().collect();
                let start = if matches!(get("start"), Value::None) {
                    0
                } else {
                    position(&self.heap, &get("start"), chars.len(), "str")?
                };
                let end = if matches!(get("end"), Value::None) {
                    chars.len()
                } else {
                    position(&self.heap, &get("end"), chars.len(), "str")?
                };
                let slice: String = chars[start.min(end)..end].iter().collect();
                if name == "index" {
                    let sub = string(&self.heap, &get("sub"))?;
                    let idx = slice
                        .find(&sub)
                        .ok_or_else(|| msg::err(&self.heap, "index_not_found", Some("str"), &[]))?;
                    Ok(Value::int(&self.heap, start + slice[..idx].chars().count()))
                } else {
                    let needle = get(if name == "startswith" {
                        "prefix"
                    } else {
                        "suffix"
                    });
                    let ns = match needle {
                        Value::Tuple(xs) => xs.to_vec(),
                        x => vec![x],
                    };
                    for n in ns {
                        let n = string(&self.heap, &n)?;
                        if if name == "startswith" {
                            slice.starts_with(&n)
                        } else {
                            slice.ends_with(&n)
                        } {
                            return Ok(Value::Bool(true));
                        }
                    }
                    Ok(Value::Bool(false))
                }
            }
            "format" => {
                let Value::Tuple(args) = get("args") else {
                    unreachable!("format 的 *args 绑成 tuple")
                };
                let Value::Dict(kwargs) = get("kwargs") else {
                    unreachable!("format 的 **kwargs 绑成 dict")
                };
                let mut out = String::new();
                let mut chars = s.chars().peekable();
                let mut index = 0;
                while let Some(c) = chars.next() {
                    if c == '{' || c == '}' {
                        if chars.peek() == Some(&c) {
                            chars.next();
                            out.push(c);
                            continue;
                        }
                        if c == '}' {
                            return Err(msg::err(&self.heap, "format_brace", Some("close"), &[]));
                        }
                        let mut field = String::new();
                        loop {
                            match chars.next() {
                                Some('}') => break,
                                Some(c) => field.push(c),
                                None => {
                                    return Err(msg::err(
                                        &self.heap,
                                        "format_brace",
                                        Some(if field.is_empty() { "open" } else { "end" }),
                                        &[],
                                    ));
                                }
                            }
                        }
                        let v = if field.is_empty() {
                            let i = index;
                            index += 1;
                            args.get(i).cloned().ok_or_else(|| {
                                msg::err(&self.heap, "format_index", None, &[("i", i.to_string())])
                            })?
                        } else {
                            if !crate::parser::is_identifier(&field) {
                                return Err(msg::err(
                                    &self.heap,
                                    "format_field",
                                    None,
                                    &[("field", field)],
                                ));
                            }
                            kwargs
                                .borrow(&self.heap)
                                .get(&Key::Str(field.as_str().into()))
                                .cloned()
                                .ok_or_else(|| {
                                    msg::key_error(&self.heap, Value::str(&self.heap, field))
                                })?
                        };
                        let part = text::metered_str(&self.heap, &v, &mut self.meter)?;
                        out.push_str(&part)
                    } else {
                        out.push(c)
                    }
                }
                Ok(Value::str(&self.heap, out))
            }
            _ => unreachable!("bind_builtin 只绑得上 lang0.subset 里 str 的方法"),
        }
    }
}

fn instance_of(heap: &Heap, v: &Value, t: &Value) -> R<bool> {
    match t {
        Value::Class(Class::Type(t)) => {
            Ok(v.type_name(heap) == *t || *t == "int" && matches!(v, Value::Bool(_)))
        }
        Value::Class(Class::Exc(_)) => exception_matches(heap, v, t),
        Value::Tuple(ts) => {
            for t in ts.iter() {
                if instance_of(heap, v, t)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Err(msg::err(heap, "isinstance_arg", None, &[])),
    }
}

struct Param {
    name: String,
    default: Option<Value>,
    posonly: bool,
    kwonly: bool,
}
struct FormArgs {
    params: Vec<Param>,
    varargs: Option<String>,
    kwargs: Option<String>,
    min: usize,
}
impl FormArgs {
    fn parse(heap: &Heap, s: &str) -> Self {
        let mut out = Self {
            params: vec![],
            varargs: None,
            kwargs: None,
            min: 0,
        };
        let mut kwonly = false;
        for p in s
            .trim_start_matches('(')
            .trim_end_matches(')')
            .split(',')
            .map(str::trim)
        {
            if p.is_empty() {
                continue;
            }
            if p == "/" {
                for p in &mut out.params {
                    p.posonly = true
                }
                continue;
            }
            if let Some(n) = p.strip_prefix("**") {
                out.kwargs = Some(n.into());
                continue;
            }
            if let Some(n) = p.strip_prefix('*') {
                kwonly = true;
                if !n.is_empty() {
                    out.varargs = Some(n.into())
                }
                continue;
            }
            let (name, default) = p
                .split_once('=')
                .map(|(n, d)| (n, Some(default_value(heap, d))))
                .unwrap_or((p, None));
            if default.is_none() && !kwonly {
                out.min += 1
            }
            out.params.push(Param {
                name: name.into(),
                default,
                posonly: false,
                kwonly,
            });
        }
        out
    }
    /// 位置参数最多几个；有 `*args` 时不限。
    fn max(&self) -> usize {
        match self.varargs {
            Some(_) => usize::MAX,
            None => self.params.iter().take_while(|p| !p.kwonly).count(),
        }
    }
    /// 位置参数的个数合得上这一种形式，见 Subset.md「形状之外的规则」。
    fn count_fits(&self, got: usize) -> bool {
        self.min <= got && got <= self.max()
    }
    /// 按 CPython 的顺序：关键字参数，位置参数过多，缺位置参数，缺仅关键字参数。
    fn bind(
        &self,
        heap: &Heap,
        name: &str,
        args: &[Value],
        kw: &[(String, Value)],
    ) -> R<BTreeMap<String, Value>> {
        let positional = self.params.iter().take_while(|p| !p.kwonly).count();
        let mut out = BTreeMap::new();
        for (p, v) in self.params.iter().take(positional).zip(args) {
            out.insert(p.name.clone(), v.clone());
        }
        if let Some(n) = &self.varargs {
            out.insert(
                n.clone(),
                Value::tuple(heap, args.iter().skip(positional).cloned().collect()),
            );
        }
        let mut extra = Dict::default();
        for (k, v) in kw {
            if self.params.iter().any(|p| p.name == *k && !p.posonly) {
                if out.contains_key(k) {
                    return Err(msg::err(
                        heap,
                        "multiple_values",
                        None,
                        &[("name", name.into()), ("key", k.clone())],
                    ));
                }
                out.insert(k.clone(), v.clone());
            } else if self.kwargs.is_some() {
                extra.insert(
                    Key::Str(k.as_str().into()),
                    Value::str(heap, k.as_str()),
                    v.clone(),
                );
            } else {
                let posonly: Vec<_> = kw
                    .iter()
                    .filter(|(k, _)| self.params.iter().any(|p| p.posonly && p.name == *k))
                    .map(|(k, _)| k.as_str())
                    .collect();
                return Err(if posonly.is_empty() {
                    msg::err(
                        heap,
                        "unexpected_kwarg",
                        None,
                        &[("name", name.into()), ("key", k.clone())],
                    )
                } else {
                    msg::err(
                        heap,
                        "positional_as_kw",
                        None,
                        &[("name", name.into()), ("names", posonly.join(", "))],
                    )
                });
            }
        }
        if args.len() > self.max() {
            return Err(arity(heap, name, self.min, self.max(), args.len()));
        }
        if let Some(n) = &self.kwargs {
            out.insert(n.clone(), Value::dict(heap, extra));
        }
        for kwonly in [false, true] {
            let mut missing = vec![];
            for p in self.params.iter().filter(|p| p.kwonly == kwonly) {
                if out.contains_key(&p.name) {
                    continue;
                }
                match &p.default {
                    Some(v) => {
                        out.insert(p.name.clone(), v.clone());
                    }
                    None => missing.push(p.name.clone()),
                }
            }
            if !missing.is_empty() {
                let id = if kwonly {
                    "missing_kwonly"
                } else {
                    "missing_args"
                };
                return Err(missing_error(heap, id, name, &missing));
            }
        }
        Ok(out)
    }
}
fn default_value(heap: &Heap, s: &str) -> Value {
    match s {
        "None" => Value::None,
        "True" => Value::Bool(true),
        "False" => Value::Bool(false),
        "()" => Value::tuple(heap, vec![]),
        "[]" => Value::list(heap, vec![]),
        "{}" => Value::dict(heap, Dict::default()),
        "''" => Value::str(heap, ""),
        s => {
            if s.contains('.') {
                Value::Float(s.parse().unwrap())
            } else {
                Value::int(heap, s.parse::<BigInt>().unwrap())
            }
        }
    }
}
