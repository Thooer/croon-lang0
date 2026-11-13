//! WASM 与原生的契约接合：既有 Delegate，以及宿主故障后同一 Instance 的重开。
//! 参考：docs/Lang0/Engineering.md（隔离）；docs/Kernel/Instance.md（执行）。

use croon_eval::host::{CheckError, HostError, Language, Read, Session};
use kernel::host::Config;
use kernel::index::kernel_spec;
use kernel::kernel::{Conn, Kernel};
use lang0_wasm::{Limits, WasmLang0};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

fn module_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("guest/target/wasm32-unknown-unknown/release/lang0_wasm_guest.wasm")
}
fn language() -> WasmLang0 {
    static LANGUAGE: OnceLock<WasmLang0> = OnceLock::new();
    LANGUAGE
        .get_or_init(|| WasmLang0::load(module_path(), Limits::default()).unwrap())
        .clone()
}
struct Empty;
impl Read for Empty {
    fn code_bytes(&self, _: &str) -> Result<Option<Vec<u8>>, HostError> {
        Ok(None)
    }
    fn code_json(&self, _: &str) -> Result<Option<Json>, HostError> {
        Ok(None)
    }
    fn code_bundle(&self, _: &str) -> Result<Option<kernel::store::Bundle>, HostError> {
        Ok(None)
    }
    fn bytes(&self, _: &str, _: &[String]) -> Result<Option<Vec<u8>>, HostError> {
        Ok(None)
    }
    fn json(&self, _: &str, _: &[String]) -> Result<Option<Json>, HostError> {
        Ok(None)
    }
    fn bundle(&self, _: &str, _: &[String]) -> Result<Option<kernel::store::Bundle>, HostError> {
        Ok(None)
    }
}

fn definition(body: &str) -> Json {
    json!({"language":"lang0@1", "body":{"source":body}, "interface":{
        "description":"", "signature":{
            "in":{"type":"object","properties":{},"required":[],"additionalProperties":false},
            "out":{"type":"null"}}, "provides":{}, "requires":{}}})
}
struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Native;
impl Language for Native {
    fn id(&self) -> &str {
        "native@1"
    }
    fn check(&self, d: &Json, read: &dyn Read) -> Result<(), CheckError> {
        lang0::Lang0.check(d, read)
    }
    fn open(
        &self,
        i: &str,
        birth: &Json,
        results: &BTreeMap<String, Json>,
        read: &dyn Read,
    ) -> Result<Box<dyn Session>, HostError> {
        lang0::Lang0.open(i, birth, results, read)
    }
}
fn config(lang: &WasmLang0) -> Config {
    Config {
        languages: vec![Box::new(lang.clone()), Box::new(Native)],
        budgets: kernel::store::Budgets {
            nodes: 10000,
            read_bytes: 32 * 1024 * 1024,
            write_bytes: 32 * 1024 * 1024,
        },
        ports: BTreeMap::new(),
        clock: Box::new(|| "2026-01-01T00:00:00Z".into()),
        fault: None,
    }
}
fn send(key: &str, to: Json, value: Json, attach: Json) -> Json {
    json!({"key":key,"send":{"to":to,"delivery":{"value":value,"dependencies":[]},"attach":attach}})
}
fn submit(k: &mut Kernel, c: &Conn, key: &str, to: Json, value: Json, attach: Json) -> Json {
    k.submit(c, &send(key, to, value, attach)).unwrap().unwrap()
}
fn submit_with_dependencies(
    k: &mut Kernel,
    c: &Conn,
    key: &str,
    to: Json,
    value: Json,
    dependencies: Vec<String>,
    attach: Json,
) -> Json {
    let atom = json!({"key": key, "send": {"to": to, "delivery": {"value": value, "dependencies": dependencies}, "attach": attach}});
    k.submit(c, &atom).unwrap().unwrap()
}
fn spawn(k: &mut Kernel, c: &Conn, key: &str, d: Json) -> (String, Json) {
    let content = k.import(&d).unwrap();
    let bundle = k.import_bundle(content.as_str(), &[]).unwrap();
    let result = submit(
        k,
        c,
        &format!("{key}-pack"),
        json!({"port":"pack"}),
        json!({"content":{"json":d},"dependencies":[]}),
        json!({}),
    );
    assert_eq!(result["outcome"]["ok"], true, "{result}");
    let result = submit_with_dependencies(
        k,
        c,
        &format!("{key}-construct"),
        json!({"port":"construct"}),
        json!({"definition":bundle}),
        vec![bundle.clone()],
        json!({}),
    );
    assert_eq!(result["outcome"]["ok"], true, "{result}");
    let did = result["outcome"]["value"]["id"].clone();
    let result = submit_with_dependencies(
        k,
        c,
        key,
        json!({"port":"instantiate"}),
        json!({"definition":did,"input":{"value":{},"dependencies":[]},"seats":{},"self":{}}),
        vec![did.as_str().unwrap().to_owned()],
        json!({}),
    );
    assert_eq!(result["outcome"]["ok"], true, "{result}");
    let instance = k
        .index()
        .journals
        .iter()
        .find(|(_, j)| j.birth()["definition"] == did)
        .unwrap()
        .0
        .clone();
    (instance, result["attach"].clone())
}

#[test]
fn a_wasm_fault_stays_local_and_reopens_from_recorded_results() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = Dir(std::env::temp_dir().join(format!(
        "croon-wasm-reopen-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    // Keep adjustable protection local to this test; other tests share a different module.
    let lang = language().with_limits(Limits::default()).unwrap();
    let mut k = Kernel::open(&dir.0, config(&lang)).unwrap();
    let message = json!({"description":"message","in":{},"attach":{"additionalProperties":false}});
    let root = json!({"evaluator":"external","interface":{"description":"","signature":{"in":{},"out":{}},
        "provides":{"inbox":message},"requires":{"pack":kernel_spec("pack").0,"construct":kernel_spec("construct").0,
        "instantiate":kernel_spec("instantiate").0,"me":message}}});
    let root_content = k.import(&root).unwrap();
    let root_bundle = k.import_bundle(root_content.as_str(), &[]).unwrap();
    let root = k
        .create_root(
            &root_bundle,
            &json!({"value":null,"dependencies":[]}),
            &json!({"me":"port/inbox"}),
            &json!({"sources":{
                "pack":{"kernel":"pack","spec":kernel_spec("pack").0},
                "construct":{"kernel":"construct","spec":kernel_spec("construct").0},
                "instantiate":{"kernel":"instantiate","spec":kernel_spec("instantiate").0}
            }}),
        )
        .unwrap();
    let conn = k.connect(&root).unwrap();
    let source = "from croon import Inbox, Port, receive\nP = Port(str)\ndef main(inbox: Inbox(P)) -> None:\n    receive(inbox)\n    return None\n";
    let mut d = definition(source);
    d["interface"]["provides"] = json!({"inbox":{"description":"","in":{"type":"string"},"attach":{"additionalProperties":false}}});
    let (target, controls) = spawn(&mut k, &conn, "target", d);
    k.run().unwrap(); // Guest is alive and waiting for receive's result.
    submit(
        &mut k,
        &conn,
        "message",
        json!({"handle":controls["port/inbox"]["handle"]}),
        json!("go"),
        json!({}),
    );
    let mut other_d = definition("def main() -> None:\n    return None\n");
    other_d["language"] = json!("native@1");
    let (other, _) = spawn(&mut k, &conn, "other", other_d);
    lang.set_limits(Limits {
        fuel: 0,
        ..Limits::default()
    })
    .unwrap();
    k.run().unwrap();
    assert!(k.stopped().contains_key(&target));
    let journal = k.index().journal(&target).unwrap();
    assert!(
        journal.keys.contains_key("0"),
        "receive result was committed before the trap"
    );
    assert!(
        journal.ended.is_none(),
        "host trap must not write body failure/end"
    );
    let prefix = journal.records.clone();
    // Correct and the root Entry remain usable while the evaluator is stopped.
    submit(
        &mut k,
        &conn,
        "pause",
        json!({"handle":controls["correct"]["handle"]}),
        json!({"op":"pause"}),
        json!({}),
    );
    submit(
        &mut k,
        &conn,
        "unpause",
        json!({"handle":controls["correct"]["handle"]}),
        json!({"op":"resume"}),
        json!({}),
    );
    lang.set_limits(Limits::default()).unwrap();
    assert!(
        k.index().journal(&other).unwrap().ended.is_some(),
        "other Instance completed in the faulting run"
    );
    assert!(k.stopped().contains_key(&target));
    let generation = k.journal().state(&target).unwrap().unwrap().generation;
    drop(k);
    let mut k = Kernel::open(&dir.0, config(&lang)).unwrap();
    assert!(
        k.stopped().contains_key(&target),
        "stopped marker survives restart"
    );
    k.reopen(&target).unwrap();
    k.run().unwrap();
    let journal = k.index().journal(&target).unwrap();
    assert_eq!(&journal.records[..prefix.len()], prefix.as_slice());
    assert_eq!(
        journal.keys.len(),
        1,
        "recorded receive was not executed twice"
    );
    assert_eq!(
        journal.records.last().unwrap()["what"]["end"],
        json!({"ok":true,"value":null,"dependencies":[]})
    );
    assert!(k.journal().state(&target).unwrap().unwrap().generation > generation);
}

#[test]
fn check_distinguishes_rejection_unreadable_content_and_host_limits() {
    let lang = language();
    assert!(matches!(
        lang.check(
            &definition("print('x')\ndef main() -> None:\n    pass\n"),
            &Empty
        ),
        Err(CheckError::Rejected(_))
    ));
    let mut d = definition("import missing\ndef main() -> None:\n    pass\n");
    d["body"]["modules"] = json!(format!("sha256:{}", "0".repeat(64)));
    assert!(
        matches!(lang.check(&d, &Empty), Err(CheckError::Rejected(_))),
        "unreadable content is None, not a trap"
    );
    let limited = lang
        .with_limits(Limits {
            fuel: 0,
            ..Limits::default()
        })
        .unwrap();
    assert!(matches!(
        limited.check(&definition("def main() -> None:\n    pass\n"), &Empty),
        Err(CheckError::Host(_))
    ));
}

// A minimal binary module exports memory, alloc and a configurable call body.
// It exercises the ABI directly, without a second guest toolchain or WAT dependency.
fn bad_guest(call_body: &[u8]) -> Vec<u8> {
    fn section(module: &mut Vec<u8>, id: u8, bytes: &[u8]) {
        module.push(id);
        module.push(bytes.len() as u8);
        module.extend(bytes);
    }
    let mut m = b"\0asm\x01\0\0\0".to_vec();
    section(
        &mut m,
        1,
        &[2, 0x60, 1, 0x7f, 1, 0x7f, 0x60, 2, 0x7f, 0x7f, 1, 0x7e],
    );
    section(&mut m, 3, &[2, 0, 1]);
    section(&mut m, 5, &[1, 0, 1]);
    section(
        &mut m,
        7,
        b"\x03\x06memory\x02\x00\x05alloc\x00\x00\x04call\x00\x01",
    );
    let mut code = vec![2, 4, 0, 0x41, 0, 0x0b, (call_body.len() + 1) as u8, 0];
    code.extend(call_body);
    section(&mut m, 10, &code);
    m
}
#[test]
fn bridge_rejects_bad_ranges_and_interrupts_unbounded_guest() {
    // i64.const -1 encodes an impossible output pointer and length.
    let invalid =
        WasmLang0::from_bytes(&bad_guest(&[0x42, 0x7f, 0x0b]), Limits::default()).unwrap();
    let result = invalid.check(&definition(""), &Empty);
    assert!(
        matches!(result,Err(CheckError::Host(ref e)) if e.0.contains("消息超出") || e.0.contains("范围越界")),
        "{result:?}"
    );
    // loop { br 0 }; unreachable i64 result. Fuel deliberately exceeds the time budget.
    let looping = WasmLang0::from_bytes(
        &bad_guest(&[0x03, 0x40, 0x0c, 0, 0x0b, 0x42, 0, 0x0b]),
        Limits {
            fuel: u64::MAX,
            time: Duration::from_millis(20),
            ..Limits::default()
        },
    )
    .unwrap();
    let result = looping.check(&definition(""), &Empty);
    assert!(
        matches!(result,Err(CheckError::Host(ref e)) if e.0.contains("时间保护")),
        "{result:?}"
    );
}
