//! 顺序求值器的运行时回归；预期不从求值器输出生成。
//! 参考：docs/Evaluator.md（求值、键、文本、资源）；docs/Subset.md（异常的消息）。
use super::*;
struct Empty;
impl Read for Empty {
    fn code_bytes(&self, _: &str) -> Result<Option<Vec<u8>>, croon_eval::host::HostError> {
        Ok(None)
    }
    fn code_json(&self, _: &str) -> Result<Option<Json>, croon_eval::host::HostError> {
        Ok(None)
    }
    fn code_bundle(
        &self,
        _: &str,
    ) -> Result<Option<croon_eval::store::Bundle>, croon_eval::host::HostError> {
        Ok(None)
    }
    fn bytes(&self, _: &str, _: &[String]) -> Result<Option<Vec<u8>>, croon_eval::host::HostError> {
        Ok(None)
    }
    fn json(&self, _: &str, _: &[String]) -> Result<Option<Json>, croon_eval::host::HostError> {
        Ok(None)
    }
    fn bundle(
        &self,
        _: &str,
        _: &[String],
    ) -> Result<Option<croon_eval::store::Bundle>, croon_eval::host::HostError> {
        Ok(None)
    }
}

fn vm(source: &str, input: Json) -> Vm {
    let mut d = json!({"language":"lang0@1","body":{"source":source}});
    let p = crate::modules::load(&d, &Empty).unwrap();
    crate::subset::check(&p).unwrap();
    d["interface"] = crate::infer::interface(&p).unwrap();
    Vm::new(
        p,
        Some(
            json!({"definition":d,"input":{"value":input,"dependencies":[]},"bundle":"definition"}),
        ),
        BTreeMap::new(),
    )
}
fn run(body: &str) -> Json {
    vm(
        &format!(
            "from typing import Any
def main() -> Any:\n{}",
            body.lines()
                .map(|l| format!("    {l}\n"))
                .collect::<String>()
        ),
        json!({}),
    )
    .resume(&BTreeMap::new(), &Empty)
    .unwrap()["done"]
        .clone()
}
fn ok(body: &str, expected: Json) {
    assert_eq!(
        run(body),
        json!({"ok":true,"value":expected,"dependencies":[]})
    );
}

#[test]
fn closures_comprehensions_and_short_circuit() {
    ok(
        "x = 2
def f(a):
    return a + x
xs = [f(i) for i in range(4) if i > 0]
x = 10
return [xs, f(1), False and 1/0, True or 1/0]",
        json!([[3, 4, 5], 11, false, true]),
    );
    ok(
        "return {str(k): v for k in range(3) for v in range(k, k+1)}",
        json!({"0":0,"1":1,"2":2}),
    );
}
#[test]
fn loops_else_unpack_and_comparison_chains() {
    ok(
        "xs = []
for a, b in [(1,2), (3,4)]:
    if a == 1:
        continue
    xs.append(a+b)
else:
    xs.append(9)
a, *b, c = range(5)
return [xs,a,b,c,1 < 2 < 3, 3 < 2 < 1]",
        json!([[7, 9], 0, [1, 2, 3], 4, true, false]),
    );
}
#[test]
fn finally_and_nested_handlers() {
    ok(
        "xs = []
def f():
    try:
        try:
            raise ValueError('x')
        except ValueError as e:
            xs.append(str(e))
            return 7
    finally:
        xs.append('finally')
r = f()
return [r,xs]",
        json!([7, ["x", "finally"]]),
    );
    ok(
        "try:
    try:
        raise KeyError('k')
    except KeyError:
        raise
except LookupError as e:
    return str(e)",
        json!("'k'"),
    );
}
#[test]
fn selection_callbacks_suspend_and_resume_in_order() {
    let source = "from typing import Any\nfrom croon import Port, send\nP=Port({'description':'p','in':{},'attach':{'additionalProperties':False},'out':{'value':{},'attach':{'additionalProperties':False}}})\ndef main(p:P) -> Any:\n    def key(x):\n        return send(p, x).value\n    return sorted([3,1,2],key=key)";
    let mut v = vm(source, json!({}));
    for (i, value) in [3, 1, 2].iter().enumerate() {
        let atom = v.resume(&BTreeMap::new(), &Empty).unwrap();
        assert_eq!(atom["atoms"][0]["send"]["delivery"]["value"], *value);
        v.results.insert(
            i.to_string(),
            json!({"outcome":{"ok":true,"value":value,"dependencies":[]},"attach":{},"derived":{}}),
        );
    }
    assert_eq!(
        v.resume(&BTreeMap::new(), &Empty).unwrap()["done"]["value"],
        json!([1, 2, 3])
    );
}

#[test]
fn inplace_list_preserves_alias_and_iterator_exhaustion() {
    ok(
        "a=[1]\nb=a\na += [2]\na *= 2\nit=enumerate(a,3)\nx=list(it)\ny=list(it)\nreturn [b,str(x),y]",
        json!([[1, 2, 1, 2], "[(3, 1), (4, 2), (5, 1), (6, 2)]", []]),
    );
}

#[test]
fn common_errors_use_frozen_messages() {
    for (body, kind, message) in [
        ("return [].pop()", "IndexError", "pop from empty list"),
        ("return [1].index(2)", "ValueError", "2 is not in list"),
        ("return 'a'.index('z')", "ValueError", "substring not found"),
        ("return {}['k']", "KeyError", "'k'"),
        ("return 1/0", "ZeroDivisionError", "division by zero"),
    ] {
        let r = run(body);
        assert_eq!(r["error"]["kind"], kind);
        assert_eq!(r["error"]["message"], message);
    }
}
#[test]
fn decimal_arithmetic_and_numeric_keys() {
    ok(
        "d = {1: 'a', True: 'b', 1.0:'c'}
return [len(d), d[1], -7//3, -7%3, str(-0.0), int('١٢'), float('1_0.5')]",
        json!([1, "c", -3, 2, "-0.0", 12, 10.5]),
    );
}
#[test]
fn top_level_execution_rejects_failure_and_budget_exhaustion() {
    let source = "x = 1/0
def main() -> None:
    pass";
    let p = crate::modules::load(&json!({"body":{"source":source}}), &Empty).unwrap();
    assert!(
        Vm::new(p, None, BTreeMap::new())
            .check_top(&Empty)
            .unwrap_err()
            .to_string()
            .contains("ZeroDivisionError")
    );
    let p = crate::modules::load(
        &json!({"body":{"source":"while True:\n    pass\ndef main() -> None:\n    pass"}}),
        &Empty,
    )
    .unwrap();
    let mut v = Vm::new(p, None, BTreeMap::new());
    v.meter.max_steps = 10;
    assert!(
        v.check_top(&Empty)
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
}

#[test]
fn tool_checks_input_and_output_and_retains_exception_handler() {
    ok(
        "from croon import tool
def f(x: int) -> int:
    return x
t=tool(f)
try:
    t({'x':'bad'})
except Exception as e:
    return str(e)",
        json!("input/x 违反 type"),
    );
}

#[test]
fn heap_reclaims_cyclic_lists_dicts_and_recursive_closures() {
    let source = "from typing import Any
from croon import Port, send
P=Port({'description':'p','in':{},'attach':{'additionalProperties':False},'out':{'value':{},'attach':{'additionalProperties':False}}})
def main(p:P) -> Any:
    send(p, None).value
    for i in range(60):
        a=[]
        a.append(a)
        d={}
        d['self']=d
        def f():
            return f
        a=None
        d=None
        f=None
    send(p, None).value
    return None";
    let mut v = vm(source, json!({}));
    v.live_floor = 1;
    assert_eq!(
        v.resume(&BTreeMap::new(), &Empty).unwrap()["atoms"][0]["key"],
        "0"
    );
    v.collect_live().unwrap();
    let before = v.heap.occupied();
    let results = BTreeMap::from([(
        "0".into(),
        json!({"outcome":{"ok":true,"value":null},"attach":{},"derived":{}}),
    )]);
    assert_eq!(v.resume(&results, &Empty).unwrap()["atoms"][0]["key"], "1");
    v.collect_live().unwrap();
    assert!(
        v.heap.occupied() <= before + 4,
        "before={before}, after={}",
        v.heap.occupied()
    );
}

#[test]
fn heap_counts_shared_mutable_data_once_and_reclaims_unreachable_data() {
    let heap = Heap::default();
    let Value::List(a) = Value::list(&heap, vec![]) else {
        panic!("list constructor")
    };
    a.borrow_mut(&heap).push(Value::List(a));
    assert_eq!(
        heap.collect(vec![
            Root::Value(Value::List(a)),
            Root::Value(Value::List(a))
        ]),
        1
    );
    assert_eq!(heap.occupied(), 1);
    assert_eq!(heap.collect(vec![]), 0);
    assert_eq!(heap.occupied(), 0);
    let text = Value::str(&heap, "abc");
    assert_eq!(
        heap.collect(vec![Root::Value(text.clone()), Root::Value(text)]),
        6
    );
}

#[test]
fn live_limit_is_deterministic_and_reports_limit_kind() {
    let source = "from typing import Any
def main() -> Any:
    x=['x' * 1000]
    return x";
    let run_once = || {
        let mut v = vm(source, json!({}));
        v.max_live = 100;
        v.live_floor = 16;
        v.resume(&BTreeMap::new(), &Empty).unwrap()
    };
    let first = run_once();
    assert_eq!(first, run_once());
    assert_eq!(first["done"]["error"]["kind"], "limit");
    assert_eq!(
        first["done"]["error"]["message"],
        msg::text("limit", Some("live"), &[])
    );
    let mut v = vm(
        "from typing import Any
from croon import kind
def main() -> Any:
    try:
        x=['x' * 1000]
    except BaseException as e:
        return kind(e)",
        json!({}),
    );
    v.max_live = 100;
    v.live_floor = 16;
    assert_eq!(
        v.resume(&BTreeMap::new(), &Empty).unwrap()["done"]["value"],
        "limit"
    );
}

#[test]
fn construction_baseline_is_subtracted_but_module_growth_is_live() {
    let source = "from typing import Any
x=['x' * 1000]
def main() -> Any:
    for i in range(10):
        s='y' * 300
    return None";
    let mut v = vm(source, json!({}));
    v.max_live = 1200;
    v.live_floor = 16;
    assert_eq!(
        v.resume(&BTreeMap::new(), &Empty).unwrap()["done"]["ok"],
        true
    );
    assert!(v.baseline >= 1000);
    // 最后一次量得的总量超过 L，减去基线后不超过：大模块数据不让 main 一开始就超限。
    assert!(
        v.last_live > v.max_live && v.last_live - v.baseline <= v.max_live,
        "baseline={}, last={}",
        v.baseline,
        v.last_live
    );
    let source = "from typing import Any
x=['x' * 1000]
def main() -> Any:
    for i in range(10):
        x.append('y' * 300)
    return None";
    let mut v = vm(source, json!({}));
    v.max_live = 1200;
    v.live_floor = 16;
    assert_eq!(
        v.resume(&BTreeMap::new(), &Empty).unwrap()["done"]["error"]["kind"],
        "limit"
    );
}

#[test]
fn construction_reads_fail_without_runtime_context() {
    for name in [
        "read_json",
        "read_text",
        "definition",
        "seat",
        "roster",
        "receive",
    ] {
        let arg = if name == "receive" {
            "[]"
        } else if name.starts_with("read_") {
            "'id'"
        } else {
            ""
        };
        let source = format!(
            "from croon import {name}
x={name}({arg})
def main() -> None:
    pass"
        );
        let p = crate::modules::load(&json!({"body":{"source":source}}), &Empty).unwrap();
        let error = Vm::new(p, None, BTreeMap::new())
            .check_top(&Empty)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&msg::text("read_at_top", None, &[("name", name.into())])),
            "{error}"
        );
    }
}

#[test]
fn binding_failures_follow_cpython_order_and_messages() {
    for (body, message) in [
        (
            "def f(a, b):\n    pass\nreturn f(1, 2, 3, c=1)",
            "main.<locals>.f() got an unexpected keyword argument 'c'",
        ),
        (
            "def f(a, b):\n    pass\nreturn f(1, 2, 3, a=1)",
            "main.<locals>.f() got multiple values for argument 'a'",
        ),
        (
            "def f(a, b, c):\n    pass\nreturn f(c=1)",
            "main.<locals>.f() missing 2 required positional arguments: 'a' and 'b'",
        ),
        (
            "def f():\n    pass\nreturn f(**[1])",
            "main.<locals>.f() argument after ** must be a mapping, not list",
        ),
        (
            "def f(a):\n    pass\nreturn f(**{'a': 1}, **{'a': 2})",
            "main.<locals>.f() got multiple values for keyword argument 'a'",
        ),
        (
            "def f(a):\n    pass\nreturn f(**{'a': 1}, a=2)",
            "main.<locals>.f() got multiple values for keyword argument 'a'",
        ),
        (
            "def f(a):\n    pass\nreturn f(a=2, **{'a': 1})",
            "main.<locals>.f() got multiple values for keyword argument 'a'",
        ),
        (
            "return len(obj=1)",
            "len() got some positional-only arguments passed as keyword arguments: 'obj'",
        ),
        (
            "return len()",
            "len() missing 1 required positional argument: 'obj'",
        ),
        (
            "return 'a'.strip(chars='x')",
            "str.strip() got some positional-only arguments passed as keyword arguments: 'chars'",
        ),
        (
            "return range()",
            "range expected at least 1 argument, got 0",
        ),
        (
            "return range(1, 2, 3, 4)",
            "range expected at most 3 arguments, got 4",
        ),
        (
            "return 'a'.startswith('a', 'b')",
            "slice indices must be integers or None or have an __index__ method",
        ),
        (
            "return [1].index(1, 'a')",
            "slice indices must be integers or have an __index__ method",
        ),
    ] {
        let r = run(body);
        assert_eq!(r["error"]["kind"], "TypeError", "{body}");
        assert_eq!(r["error"]["message"], message, "{body}");
    }
}

#[test]
fn call_depth_and_deep_traversal_share_one_budget() {
    let source = "from typing import Any
from croon import kind
def main() -> Any:
    x = []
    for i in range(100):
        x = [x]
    def f(n):
        if n == 0:
            return str(x)
        return f(n - 1)
    shallow = len(str(x))
    try:
        f(60)
    except BaseException as e:
        return [shallow, kind(e), str(e)]";
    let r = vm(source, json!({}))
        .resume(&BTreeMap::new(), &Empty)
        .unwrap();
    assert_eq!(
        r["done"]["value"],
        json!([202, "limit", msg::text("limit", Some("depth"), &[])])
    );
}

#[test]
fn join_of_reference_values_fails_with_ref_text() {
    let source = "from typing import Any
from croon import Port, send
P=Port({'description':'p','in':{},'attach':{'additionalProperties':False},'out':{'value':{},'attach':{'additionalProperties':False}}})
def main(p:P) -> Any:
    try:
        ''.join([p])
    except BaseException as e:
        return str(e)";
    let r = vm(source, json!({}))
        .resume(&BTreeMap::new(), &Empty)
        .unwrap();
    assert_eq!(r["done"]["value"], msg::text("ref_text", None, &[]));
}

#[test]
fn text_output_is_charged_before_allocation() {
    let heap = Heap::default();
    let mut value = Value::str(&heap, "\n");
    for _ in 0..16 {
        value = Value::tuple(&heap, vec![value.clone(), value]);
    }
    let produced = heap.produced.get();
    let mut meter = ops::Meter {
        steps: 0,
        max_steps: 100,
        depth: 128,
    };
    let error = text::metered_str(&heap, &value, &mut meter).unwrap_err();
    assert_eq!(
        text::str_of(&heap, &error, 128).unwrap(),
        msg::text("limit", Some("steps"), &[])
    );
    assert!(meter.steps > meter.max_steps);
    // 失败只产出异常及其消息，未把展开的文本放进求值器的值。
    assert!(heap.produced.get() - produced < 100);
}

#[test]
fn text_measurement_matches_rendered_bytes() {
    let heap = Heap::default();
    let values = [
        Value::str(&heap, "\n\t\\'\"é\u{200b}"),
        Value::tuple(&heap, vec![Value::str(&heap, "\n")]),
        msg::exc(
            &heap,
            "ValueError",
            vec![Value::str(&heap, "é"), Value::None],
            None,
        ),
        msg::key_error(&heap, Value::str(&heap, "\n")),
    ];
    for value in values {
        let mut meter = ops::Meter {
            steps: 0,
            max_steps: 1000,
            depth: 128,
        };
        let rendered = text::metered_str(&heap, &value, &mut meter).unwrap();
        assert_eq!(meter.steps, (rendered.len() as u64).max(1));
    }
    ok(
        "x = ['é', '\\n']\nreturn [str(x), f'{x}', '{}'.format(x)]",
        json!(["['é', '\\n']", "['é', '\\n']", "['é', '\\n']"]),
    );
}

#[test]
fn deeply_nested_tuples_drop_without_recursion() {
    // 两份共享的根分别释放，最后一个引用才拆开其拥有的元素。
    let heap = Heap::default();
    let mut value = Value::None;
    for _ in 0..100_000 {
        value = Value::tuple(&heap, vec![value]);
    }
    let shared = value.clone();
    drop(value);
    let list = Value::list(&heap, vec![shared]);
    heap.collect(vec![heap::Root::Value(list)]);
    heap.collect(vec![]);
    ok(
        "x = ()\nfor i in range(10000):\n    x = (x,)\nx = None\nreturn 1",
        json!(1),
    );
}

#[test]
fn boundary_numbers_use_value_limits() {
    let heap = Heap::default();
    let max = croon_eval::value::Limits::get().number_max;
    for sign in [-1i64, 1] {
        for float in [false, true] {
            for outside in [false, true] {
                let number = (max + u64::from(outside)) as i64 * sign;
                let value = if float {
                    Value::Float(number as f64)
                } else {
                    Value::int(&heap, number)
                };
                let mut meter = ops::Meter {
                    steps: 0,
                    max_steps: 1000,
                    depth: 128,
                };
                let encoded = bridge::encode(&heap, &value, "value", &mut meter);
                if outside {
                    let error = encoded.unwrap_err();
                    assert_eq!(
                        text::str_of(&heap, &error, 128).unwrap(),
                        msg::text(
                            "not_value",
                            None,
                            &[
                                ("where", "value".into()),
                                ("pointer", "".into()),
                                ("reason", msg::value_reason("number", &[])),
                            ]
                        )
                    );
                } else {
                    assert_eq!(encoded.unwrap().as_f64(), Some(number as f64));
                }
            }
        }
    }
}

#[test]
fn cpython_differential() {
    let output = std::process::Command::new("python3.12")
        .args(["-B", "cpython/differential.py"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("差分必须安装 CPython 3.12，不能跳过");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cases: Json = serde_json::from_slice(&output.stdout).unwrap();
    for case in cases.as_array().unwrap() {
        let mut v = vm(case["source"].as_str().unwrap(), json!({}));
        let mut atoms = Vec::new();
        let done = loop {
            let response = v.resume(&BTreeMap::new(), &Empty).unwrap();
            if let Some(done) = response.get("done") {
                break done.clone();
            }
            let batch = response["atoms"].as_array().unwrap();
            assert!(!batch.is_empty(), "{}：没有脚本结果", case["name"]);
            for atom in batch {
                let key = atom["key"].as_str().unwrap();
                let result = &case["results"][key];
                assert!(!result.is_null(), "{}：缺少键 {key}", case["name"]);
                atoms.push(atom.clone());
                v.results.insert(key.to_owned(), result.clone());
            }
        };
        assert_eq!(
            json!({"atoms":atoms,"done":done}),
            case["expected"],
            "{}",
            case["name"]
        );
        println!("{}：结束值与 {} 个 Atom 相符", case["name"], atoms.len());
    }
}

#[test]
fn documented_cpython_differences() {
    ok(
        "def f():\n    pass\nreturn [str(f), str([].append), str(dict.pop), str(enumerate([])), str(zip())]",
        json!([
            "<function main.<locals>.f>",
            "<built-in method append of list object>",
            "<method 'pop' of 'dict' objects>",
            "<enumerate object>",
            "<zip object>"
        ]),
    );
    ok(
        "try:\n    len()\nexcept TypeError as e:\n    return str(e)",
        json!("len() missing 1 required positional argument: 'obj'"),
    );
    for source in [
        "def main() -> None:\n    set()",
        "def main() -> None:\n    frozenset()",
        "def main() -> None:\n    x = {1, 2}",
    ] {
        let p = crate::modules::load(&json!({"body":{"source":source}}), &Empty).unwrap();
        assert!(crate::subset::check(&p).is_err(), "首版不收 set：{source}");
    }
}

#[test]
fn sequence_counts_use_64_bit_boundaries() {
    ok(
        "n = 4294967296
return [len(range(n)), 'aaa'.replace('a', 'b', n), 'a b'.split(' ', n), '' * n, [] * n, len(() * n)]",
        json!([4294967296u64, "bbb", ["a", "b"], "", [], 0]),
    );
    assert_eq!(run("return 'x' * 4294967296")["error"]["kind"], "limit");
}

struct ObservedRead {
    calls: std::cell::RefCell<Vec<Vec<String>>>,
    fault: bool,
}
impl Read for ObservedRead {
    fn code_bytes(&self, _: &str) -> Result<Option<Vec<u8>>, croon_eval::host::HostError> {
        panic!("business read used code closure")
    }
    fn code_json(&self, _: &str) -> Result<Option<Json>, croon_eval::host::HostError> {
        panic!("business read used code closure")
    }
    fn code_bundle(
        &self,
        _: &str,
    ) -> Result<Option<croon_eval::store::Bundle>, croon_eval::host::HostError> {
        panic!("business read used code closure")
    }
    fn bytes(
        &self,
        id: &str,
        consumed: &[String],
    ) -> Result<Option<Vec<u8>>, croon_eval::host::HostError> {
        self.json(id, consumed).map(|v| v.map(|_| b"text".to_vec()))
    }
    fn json(
        &self,
        _: &str,
        consumed: &[String],
    ) -> Result<Option<Json>, croon_eval::host::HostError> {
        self.calls.borrow_mut().push(consumed.to_vec());
        if self.fault {
            return Err(croon_eval::host::HostError("reader stopped".into()));
        }
        Ok((consumed == ["0"]).then(|| json!({"acquired":true})))
    }
    fn bundle(
        &self,
        _: &str,
        _: &[String],
    ) -> Result<Option<croon_eval::store::Bundle>, croon_eval::host::HostError> {
        Ok(None)
    }
}

#[test]
fn business_reads_follow_consumption_even_when_future_results_are_preloaded() {
    let source = "from typing import Any
from croon import Port, send, read_json, read_text, kind
P=Port({'description':'p','in':{},'attach':{'additionalProperties':False}})
def main(p:P) -> Any:
    try:
        read_json('future')
    except Exception as e:
        before=kind(e)
    send(p, None)
    return [before, read_json('future'), read_text('future')]";
    let results = BTreeMap::from([(
        "0".into(),
        json!({"outcome":{"ok":true,"value":null,"dependencies":["root"]},"attach":{},"derived":{},"accepted":true}),
    )]);
    let read = ObservedRead {
        calls: Default::default(),
        fault: false,
    };
    let mut replay = vm(source, json!({}));
    assert_eq!(
        replay.resume(&results, &read).unwrap()["done"]["value"],
        json!(["use",{"acquired":true},"text"])
    );
    assert_eq!(
        *read.calls.borrow(),
        vec![Vec::<String>::new(), vec!["0".into()], vec!["0".into()]]
    );
    let mut live = vm(source, json!({}));
    let live_read = ObservedRead {
        calls: Default::default(),
        fault: false,
    };
    assert_eq!(
        live.resume(&BTreeMap::new(), &live_read).unwrap()["atoms"][0]["key"],
        "0"
    );
    assert_eq!(
        live.resume(&results, &live_read).unwrap(),
        replay.resume(&results, &read).unwrap()
    );
    assert_eq!(*live_read.calls.borrow(), *read.calls.borrow());
}

#[test]
fn send_receipts_and_main_delivery_keep_explicit_dependencies() {
    let source = "from typing import Any
from croon import Port, send, deliver
P=Port({'description':'p','in':{},'attach':{'additionalProperties':False}})
def main(p:P) -> Any:
    r=send(p, {'input':1}, dependencies=['base'])
    d=r.dependencies
    d.clear()
    return deliver([r.value, r.dependencies, list(r.attach), list(r.derived)], r.dependencies)";
    let mut v = vm(source, json!({}));
    assert_eq!(
        v.resume(&BTreeMap::new(), &Empty).unwrap(),
        json!({"atoms":[{"key":"0","send":{"to":{"port":"p"},"delivery":{"value":{"input":1},"dependencies":["base"]},"attach":{}}}]})
    );
    let results = BTreeMap::from([(
        "0".into(),
        json!({"outcome":{"ok":true,"value":2,"dependencies":["product"]},"attach":{},"derived":{},"accepted":true}),
    )]);
    assert_eq!(
        v.resume(&results, &Empty).unwrap(),
        json!({"done":{"ok":true,"value":[2,["product"],[],[]],"dependencies":["product"]}})
    );
    assert_eq!(
        run("from croon import deliver\nreturn deliver(None, [])")["dependencies"],
        json!([])
    );
    assert_eq!(
        run(
            "from croon import deliver, tool\ndef f() -> int:\n    return deliver(1, [])\nreturn tool(f)({})"
        )["error"]["kind"],
        "type"
    );
}

#[test]
fn failure_dependencies_are_available_and_read_faults_escape_body_handlers() {
    let source = "from typing import Any
from croon import Port, send, data
P=Port({'description':'p','in':{},'attach':{'additionalProperties':False}})
def main(p:P) -> Any:
    try:
        send(p, None)
    except Exception as e:
        return data(e)";
    let results = BTreeMap::from([(
        "0".into(),
        json!({"outcome":{"ok":false,"error":{"kind":"use","message":"failed","data":{"status":7}},"dependencies":["partial"]},"attach":{},"derived":{},"accepted":true}),
    )]);
    let mut v = vm(source, json!({}));
    assert_eq!(
        v.resume(&results, &Empty).unwrap()["done"]["value"],
        json!({"error":{"status":7},"dependencies":["partial"]})
    );
    let mut v = vm(
        "from croon import read_json\ndef main() -> None:\n    try:\n        read_json('id')\n    except BaseException:\n        return None",
        json!({}),
    );
    let read = ObservedRead {
        calls: Default::default(),
        fault: true,
    };
    assert_eq!(
        v.resume(&BTreeMap::new(), &read).unwrap_err().0,
        "reader stopped"
    );
    assert!(v.finished.is_none());
}

#[test]
fn definition_reads_the_whole_birth_definition() {
    let source = "from typing import Any\nfrom croon import definition\ndef main() -> Any:\n    d = definition()\n    return [d['language'], 'def main' in d['body']['source']]";
    let mut v = vm(source, json!({}));
    assert_eq!(
        v.resume(&BTreeMap::new(), &Empty).unwrap()["done"]["value"],
        json!(["lang0@1", true])
    );
}

#[test]
fn a_dict_holds_reference_values_and_forwards_them_as_attach() {
    // run_agent 要按目标的 Interface 决定接不接某个 Port：attach 在局部按名字拼起来，值仍是引用值本身。
    let source = "from typing import Any
from croon import Port, send
P = Port({'description':'p','in':{},'attach':{'additionalProperties':False}})
Q = Port({'description':'q','in':{},'attach':{'additionalProperties':False}})
A = Port({'description':'a','in':{},'attach':{'additionalProperties':True}})
def main(p: P, q: Q, a: A) -> Any:
    ports = {}
    want = ['p', 'zzz']
    if 'p' in want:
        ports['p'] = p
    if 'zzz' in want:
        ports['zzz'] = q
    send(a, None, attach=ports)
    return list(ports)";
    let results = BTreeMap::from([(
        "0".into(),
        json!({"outcome":{"ok":true,"value":null,"dependencies":[]},"attach":{},"derived":{},"accepted":true}),
    )]);
    let mut v = vm(source, json!({}));
    let pending = v.run(&Empty).unwrap();
    assert_eq!(
        pending["atoms"][0]["send"]["attach"],
        json!({"p":{"port":"p"},"zzz":{"port":"q"}}),
        "{pending}"
    );
    let done = v.resume(&results, &Empty).unwrap();
    assert_eq!(done["done"]["value"], json!(["p", "zzz"]), "{done}");
}
