//! 无 WASI 的 Lang0 guest；Read 借用只在一次导出调用里使用。
//! 参考：docs/Lang0/Engineering.md（隔离）；docs/Kernel/Language.md（插口）。

use croon_eval::host::{CheckError, HostError, Language, Read, Session};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;

#[link(wasm_import_module = "kernel")]
unsafe extern "C" {
    fn read(kind: u32, ptr: *const u8, len: u32, out: *mut u8, cap: u32) -> i32;
}

struct Reader;
impl Reader {
    fn read(&self, kind: u32, id: &str, consumed: Option<&[String]>) -> Result<Option<Vec<u8>>, HostError> {
        let request = serde_json::to_vec(&json!({"id":id,"consumed_keys":consumed})).map_err(|e| HostError(e.to_string()))?;
        let n = unsafe {
            read(
                kind,
                request.as_ptr(),
                request.len().try_into().expect("读取请求长度"),
                std::ptr::null_mut(),
                0,
            )
        };
        if n == -1 {
            return Ok(None);
        }
        assert!(n >= 0, "读取桥返回了未知状态");
        let mut bytes = vec![0; n as usize];
        let got = unsafe {
            read(
                kind,
                request.as_ptr(),
                request.len() as u32,
                bytes.as_mut_ptr(),
                n as u32,
            )
        };
        assert_eq!(got, n, "同一次调用里的读取结果改变");
        Ok(Some(bytes))
    }
}
impl Read for Reader {
    fn code_bytes(&self, id: &str) -> Result<Option<Vec<u8>>, HostError> { self.read(0, id, None) }
    fn code_json(&self, id: &str) -> Result<Option<Value>, HostError> {
        self.read(1, id, None)?.map(|bytes| serde_json::from_slice(&bytes).map_err(|e| HostError(e.to_string()))).transpose()
    }
    fn code_bundle(&self, id: &str) -> Result<Option<croon_eval::store::Bundle>, HostError> {
        self.read(2, id, None)?.map(decode_bundle).transpose()
    }
    fn bytes(&self, id: &str, consumed: &[String]) -> Result<Option<Vec<u8>>, HostError> { self.read(0, id, Some(consumed)) }
    fn json(&self, id: &str, consumed: &[String]) -> Result<Option<Value>, HostError> {
        self.read(1, id, Some(consumed))?.map(|bytes| serde_json::from_slice(&bytes).map_err(|e| HostError(e.to_string()))).transpose()
    }
    fn bundle(&self, id: &str, consumed: &[String]) -> Result<Option<croon_eval::store::Bundle>, HostError> {
        self.read(2, id, Some(consumed))?.map(decode_bundle).transpose()
    }
}

fn decode_bundle(bytes: Vec<u8>) -> Result<croon_eval::store::Bundle, HostError> {
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| HostError(e.to_string()))?;
    Ok(croon_eval::store::Bundle {
        content: value["content"].as_str().ok_or_else(|| HostError("Bundle 缺少 content".into()))?.into(),
        dependencies: serde_json::from_value(value["dependencies"].clone()).map_err(|e| HostError(e.to_string()))?,
    })
}

thread_local! {
    static SESSION: RefCell<Option<Box<dyn Session>>> = const { RefCell::new(None) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: u32) -> *mut u8 {
    Box::into_raw(vec![0u8; len as usize].into_boxed_slice()) as *mut u8
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn call(ptr: *mut u8, len: u32) -> u64 {
    let bytes = unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len as usize)) };
    let response = match serde_json::from_slice(&bytes) {
        Ok(input) => invoke(input).unwrap_or_else(|error| json!({"error":error})),
        Err(error) => json!({"error":format!("输入 JSON：{error}")}),
    };
    OUTPUT.with(|slot| {
        let mut bytes = slot.borrow_mut();
        *bytes = match serde_json::to_vec(&response) {
            Ok(bytes) => bytes,
            Err(error) => format!("{{\"error\":\"输出 JSON：{error}\"}}").into_bytes(),
        };
        ((bytes.len() as u64) << 32) | bytes.as_ptr() as u64
    })
}

fn invoke(input: Value) -> Result<Value, String> {
    let op = input.get("op").and_then(Value::as_str).ok_or("缺少 op")?;
    if op == "check" {
        let definition = input.get("definition").ok_or("缺少 definition")?;
        return match lang0::Lang0.check(definition, &Reader) {
            Ok(()) => Ok(json!({"ok":true})),
            Err(CheckError::Rejected(message)) => Ok(json!({"ok":false,"message":message})),
            Err(CheckError::Host(error)) => Err(error.to_string()),
        };
    }
    let results: BTreeMap<String, Value> =
        serde_json::from_value(input.get("results").cloned().ok_or("缺少 results")?)
            .map_err(|error| format!("results：{error}"))?;
    match op {
        "open" => {
            let instance = input
                .get("instance")
                .and_then(Value::as_str)
                .ok_or("缺少 instance")?;
            let birth = input.get("birth").ok_or("缺少 birth")?;
            let session = lang0::Lang0
                .open(instance, birth, &results, &Reader)
                .map_err(|error| error.to_string())?;
            SESSION.with(|slot| *slot.borrow_mut() = Some(session));
            Ok(json!({"ok":true}))
        }
        "resume" => SESSION.with(|slot| {
            let mut slot = slot.borrow_mut();
            let session = slot.as_mut().ok_or("会话不存在")?;
            let value = session
                .resume(&results, &Reader)
                .map_err(|error| error.to_string())?;
            Ok(json!({"ok":true,"value":value}))
        }),
        _ => Err("未知操作".into()),
    }
}
