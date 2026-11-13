//! 契约边界：Value 与求值器值、引用值与 Handle 的转换。
//! 参考：docs/Kernel/Language.md（契约边界上的转换）；docs/Lang0/README.md（值与类型、运行时对象）。
use super::heap::{Heap, Slot};

use super::{
    msg,
    ops::Meter,
    text::{self, R},
    value::*,
};
use num_traits::ToPrimitive;
use serde_json::{Value as Json, json};

pub fn decode(heap: &Heap, v: &Json) -> Value {
    match v {
        Json::Null => Value::None,
        Json::Bool(v) => Value::Bool(*v),
        Json::Number(n) => {
            // Value 的规范文本决定 int/float，不能沿用 serde_json 原来保留的 2.0。
            let s = croon_eval::value::canonical(v).expect("Kernel 给的 Value");
            if !s.contains(['.', 'e', 'E']) {
                Value::int(heap, s.parse::<num_bigint::BigInt>().unwrap())
            } else {
                Value::Float(n.as_f64().unwrap())
            }
        }
        Json::String(s) => Value::str(heap, s.as_str()),
        Json::Array(xs) => Value::list(heap, xs.iter().map(|v| decode(heap, v)).collect()),
        Json::Object(xs) => Value::dict(
            heap,
            Dict::of_strs(xs.iter().map(|(k, v)| (k.clone(), decode(heap, v)))),
        ),
    }
}

fn invalid(heap: &Heap, where_: &str, pointer: &str, reason: String) -> Value {
    msg::err(
        heap,
        "not_value",
        None,
        &[
            ("where", where_.into()),
            ("pointer", pointer.into()),
            ("reason", reason),
        ],
    )
}

pub fn encode(heap: &Heap, v: &Value, where_: &str, m: &mut Meter) -> R<Json> {
    fn go(heap: &Heap, v: &Value, w: &str, p: &str, depth: usize, m: &mut Meter) -> R<Json> {
        if depth == 0 {
            return Err(msg::limit(heap, "depth"));
        }
        m.charge(heap, 1)?;
        Ok(match v {
            Value::None => Json::Null,
            Value::Bool(b) => json!(b),
            Value::Int(i) => {
                let n = i
                    .to_i64()
                    .filter(|n| n.unsigned_abs() <= croon_eval::value::Limits::get().number_max)
                    .ok_or_else(|| invalid(heap, w, p, msg::value_reason("number", &[])))?;
                json!(n)
            }
            Value::Float(x) => {
                if !x.is_finite() || x.abs() > croon_eval::value::Limits::get().number_max as f64 {
                    return Err(invalid(heap, w, p, msg::value_reason("number", &[])));
                }
                json!(x)
            }
            Value::Str(s) => {
                m.charge(heap, s.len() as u64)?;
                json!(s.as_ref())
            }
            Value::List(xs) => Json::Array(
                xs.borrow(heap)
                    .iter()
                    .enumerate()
                    .map(|(i, v)| go(heap, v, w, &format!("{p}/{i}"), depth - 1, m))
                    .collect::<R<_>>()?,
            ),
            Value::Dict(xs) => {
                let mut out = serde_json::Map::new();
                for (k, v) in xs.borrow(heap).items() {
                    let Value::Str(k) = k else {
                        return Err(invalid(heap, w, p, msg::value_reason("key", &[])));
                    };
                    out.insert(
                        k.to_string(),
                        go(
                            heap,
                            v,
                            w,
                            &format!("{p}/{}", k.replace('~', "~0").replace('/', "~1")),
                            depth - 1,
                            m,
                        )?,
                    );
                }
                Json::Object(out)
            }
            other => {
                return Err(invalid(
                    heap,
                    w,
                    p,
                    msg::value_reason(
                        match other {
                            Value::Tuple(_) => "tuple",
                            Value::Ref(_) | Value::Inbox(_) => "引用值",
                            Value::Receipt(_) => "回执",
                            Value::Arrival(_) => "到达",
                            Value::Derived(_) => "派生出的那一份",
                            _ => "其余",
                        },
                        &[("type", other.type_name(heap))],
                    ),
                ));
            }
        })
    }
    let out = go(heap, v, where_, "", m.depth, m)?;
    croon_eval::value::canonical(&out).map_err(|e| {
        use croon_eval::value::ValueError;
        let (pointer, limit) = match e {
            ValueError::TooDeep { path, .. } => (path, "depth"),
            ValueError::JsonTooLarge { .. } => (String::new(), "json_bytes"),
            ValueError::NumberTooLarge { .. } => panic!("encode 已检查数的范围"),
            ValueError::BytesTooLarge { .. } => panic!("canonical 只编码 JSON"),
        };
        invalid(
            heap,
            where_,
            &pointer,
            msg::value_reason("limits", &[("limit", limit.into())]),
        )
    })?;
    Ok(out)
}

pub fn check(heap: &Heap, schema: &Json, v: &Value, where_: &str, m: &mut Meter) -> R<Json> {
    let json = encode(heap, v, where_, m)?;
    let report = croon_eval::schema::validate(schema, &json);
    if let Err(errors) = report {
        let e = &errors[0];
        return Err(msg::err(
            heap,
            "schema_violation",
            None,
            &[
                ("where", where_.into()),
                ("pointer", e.path.clone()),
                ("rule", e.rule.into()),
            ],
        ));
    }
    Ok(json)
}

pub fn outcome(heap: &Heap, error: &Value) -> Json {
    let Value::Exc(e) = error else {
        panic!("求值器错误不是异常")
    };
    let e = e.borrow(heap).clone();
    // 失败交出时帧已退完，深度回到满额。
    let message = match text::exc_str(heap, e.class, &e.args, super::DEPTH) {
        Ok(message) => message,
        Err(error) => return outcome(heap, &error),
    };
    json!({"ok":false,"error":{"kind":e.kind(),"message":message,"data":e.data},"dependencies":[]})
}

pub fn incoming(heap: &Heap, v: &Json, declared: Option<&Json>) -> Value {
    Value::Ref(heap.alloc(RefV {
        target: json!({"handle":v["handle"]}),
        spec: match declared {
            Some(s) if s.is_object() => Spec::Inline(s.clone()),
            _ => Spec::Inline(v["spec"].clone()),
        },
    }))
}

pub fn attachments(heap: &Heap, v: &Json, declaration: Option<&Json>) -> Slot<Dict> {
    heap.alloc(Dict::of_strs(
        v.as_object()
            .expect("契约的 attach 是对象")
            .iter()
            .map(|(name, v)| {
                let own = declaration.and_then(|d| {
                    d["properties"]
                        .get(name)
                        .or_else(|| d.get("additionalProperties"))
                });
                (name.clone(), incoming(heap, v, own))
            }),
    ))
}

pub fn received(heap: &Heap, result: &Json, declared: &Json) -> Value {
    let a = &result["arrival"];
    if a.is_null() {
        return Value::None;
    }
    let port = a["port"].as_str().unwrap();
    Value::Arrival(heap.alloc(Arrival {
        port: Value::Inbox(port.into()),
        sender: a["sender"].as_str().unwrap().into(),
        via: a["via"].as_str().unwrap().into(),
        value: decode(heap, &a["delivery"]["value"]),
        dependencies: a["delivery"]["dependencies"].clone(),
        attach: attachments(heap, &a["attach"], Some(&declared[port]["attach"])),
    }))
}

pub fn sent(heap: &Heap, result: &Json, own: &Json) -> R<Value> {
    let o = &result["outcome"];
    if o["ok"] == false {
        let e = &o["error"];
        return Err(Value::Exc(heap.alloc(Exc {
            class: "Failure",
            args: vec![Value::str(heap, e["message"].as_str().unwrap())],
            kind: Some(e["kind"].as_str().unwrap().into()),
            data: json!({"error":e["data"],"dependencies":o["dependencies"]}),
        })));
    }
    let value = decode(heap, &o["value"]);
    let derived = Dict::of_strs(
        result["derived"]
            .as_object()
            .expect("契约的 derived 是对象")
            .iter()
            .map(|(k, d)| {
                (
                    k.clone(),
                    Value::Derived(heap.alloc(Derived {
                        handle: d["handle"].as_str().unwrap().into(),
                        revoker: incoming(heap, &d["revoker"], None),
                    })),
                )
            }),
    );
    Ok(Value::Receipt(heap.alloc(Receipt {
        value,
        dependencies: o["dependencies"].clone(),
        attach: attachments(heap, &result["attach"], Some(&own["out"]["attach"])),
        derived: heap.alloc(derived),
    })))
}
