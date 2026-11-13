//! 登记 lang0@1 的 WASM 装配与宿主临时保护。
//! 参考：docs/Engineering.md（隔离）；croon:docs/Kernel/Language.md「实现」。

mod blocks;

use croon_eval::host::{CheckError, HostError, Language, Read, Session};
use croon_eval::value::ValueId;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use wasmtime::{
    Caller, Config, Engine, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder,
    TypedFunc,
};

/// 每次调用的宿主保护，不是 lang0@1 的 Surface limit。
#[derive(Clone, Debug)]
pub struct Limits {
    pub fuel: u64,
    pub memory_bytes: usize,
    pub message_bytes: usize,
    pub time: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            fuel: 2_000_000_000,
            memory_bytes: 128 * 1024 * 1024,
            message_bytes: 32 * 1024 * 1024,
            time: Duration::from_secs(30),
        }
    }
}
impl Limits {
    fn validate(&self) -> Result<(), HostError> {
        if self.memory_bytes == 0
            || self.message_bytes == 0
            || self.message_bytes > i32::MAX as usize
            || self.time.is_zero()
        {
            return Err(HostError(
                "宿主 memory、message、time 保护必须为正，message 不超过 i32::MAX".into(),
            ));
        }
        Ok(())
    }
}

/// 克隆只共享已编译模块与宿主额度，不共享 Instance、Store 或读取资格。
#[derive(Clone)]
pub struct WasmLang0 {
    module: Module,
    limits: Arc<RwLock<Limits>>,
}
impl WasmLang0 {
    pub fn load(path: impl AsRef<Path>, limits: Limits) -> Result<Self, HostError> {
        Self::from_bytes(&std::fs::read(path).map_err(host)?, limits)
    }
    pub fn from_bytes(bytes: &[u8], limits: Limits) -> Result<Self, HostError> {
        limits.validate()?;
        let mut config = Config::new();
        config
            .cranelift_nan_canonicalization(true)
            .consume_fuel(true)
            .epoch_interruption(true)
            .max_wasm_stack(16 * 1024 * 1024);
        let engine = Engine::new(&config).map_err(host)?;
        let module = Module::new(&engine, bytes).map_err(host)?;
        Ok(Self {
            module,
            limits: Arc::new(RwLock::new(limits)),
        })
    }
    /// 复用已编译模块，为另一份装配建立独立的保护额度。
    pub fn with_limits(&self, limits: Limits) -> Result<Self, HostError> {
        limits.validate()?;
        Ok(Self {
            module: self.module.clone(),
            limits: Arc::new(RwLock::new(limits)),
        })
    }
    /// 已开的单元在下一次调用使用新额度；故障单元由 Kernel 显式重开。
    pub fn set_limits(&self, limits: Limits) -> Result<(), HostError> {
        limits.validate()?;
        *self
            .limits
            .write()
            .map_err(|_| HostError("宿主额度锁已损坏".into()))? = limits;
        Ok(())
    }
    fn worker(&self, instance: &str) -> Result<Worker, HostError> {
        Worker::new(instance, self.module.clone(), self.limits.clone())
    }
}

fn host(error: impl std::fmt::Display) -> HostError {
    HostError(format!("{error:#}"))
}
fn protocol() -> &'static Json {
    static PROTOCOL: OnceLock<Json> = OnceLock::new();
    PROTOCOL.get_or_init(|| blocks::json("lang0.wasm"))
}
fn check_message(side: &str, value: &Json) -> Result<(), HostError> {
    croon_eval::schema::validate(&protocol()[side], value)
        .map_err(|errors| HostError(format!("guest {side} 违反调用协议：{errors:?}")))
}
fn snapshot(limits: &RwLock<Limits>) -> Result<Limits, HostError> {
    limits
        .read()
        .map(|limits| limits.clone())
        .map_err(|_| HostError("宿主额度锁已损坏".into()))
}

type ReadReply = Result<Option<Vec<u8>>, HostError>;
struct ReadRequest {
    kind: u32,
    id: String,
    consumed_keys: Option<Vec<String>>,
    limit: usize,
    reply: mpsc::Sender<ReadReply>,
}
enum Event {
    Read(ReadRequest),
    Completed(Result<Json, HostError>),
}
struct HostState {
    events: mpsc::Sender<Event>,
    memory: StoreLimits,
    message_limit: usize,
    deadline: Instant,
}
struct Guest {
    store: Store<HostState>,
    memory: Memory,
    alloc: TypedFunc<u32, u32>,
    call: TypedFunc<(u32, u32), u64>,
}

/// 验证后才建立切片或按 guest 长度分配；u32 的加法用宿主 usize 的 checked_add。
fn checked_range(
    size: usize,
    ptr: u32,
    len: u32,
    limit: usize,
) -> Result<std::ops::Range<usize>, HostError> {
    let start = ptr as usize;
    let len = len as usize;
    if len > limit {
        return Err(HostError("guest 消息超出宿主上限".into()));
    }
    let end = start
        .checked_add(len)
        .filter(|end| *end <= size)
        .ok_or_else(|| HostError("guest 内存范围越界".into()))?;
    Ok(start..end)
}

/// 计时线程只在本次调用期间存活；drop 唤醒并 join，不跨调用留下旧 epoch。
struct Deadline {
    cancel: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}
impl Deadline {
    fn start(engine: Engine, time: Duration) -> Result<Self, HostError> {
        let (cancel, cancelled) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("lang0-wasm-deadline".into())
            .spawn(move || {
                if matches!(
                    cancelled.recv_timeout(time),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    engine.increment_epoch();
                }
            })
            .map_err(host)?;
        Ok(Self {
            cancel: Some(cancel),
            thread: Some(thread),
        })
    }
}
impl Drop for Deadline {
    fn drop(&mut self) {
        self.cancel.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Guest {
    fn new(
        module: &Module,
        events: mpsc::Sender<Event>,
        limits: &Limits,
    ) -> Result<Self, HostError> {
        let mut linker = Linker::new(module.engine());
        linker
            .func_wrap(
                "kernel",
                "read",
                |mut caller: Caller<'_, HostState>,
                 kind: u32,
                 ptr: u32,
                 len: u32,
                 out: u32,
                 cap: u32|
                 -> Result<i32, wasmtime::Error> {
                    if kind > 2 {
                        return Err(wasmtime::Error::msg("未知读取种类"));
                    }
                    let memory = caller
                        .get_export("memory")
                        .and_then(|export| export.into_memory())
                        .ok_or_else(|| wasmtime::Error::msg("guest 没有 memory"))?;
                    // ValueId 的文本形状由 Kernel 唯一检查；这里只允许固定长度，再借用 guest 内存。
                    let limit = caller.data().message_limit;
                    let range = checked_range(memory.data_size(&caller), ptr, len, limit)
                        .map_err(|e| wasmtime::Error::msg(e.0))?;
                    if cap != 0 {
                        checked_range(memory.data_size(&caller), out, cap, limit)
                            .map_err(|e| wasmtime::Error::msg(e.0))?;
                    }
                    let request: Json = serde_json::from_slice(&memory.data(&caller)[range])
                        .map_err(|e| wasmtime::Error::msg(e.to_string()))?;
                    let id = request["id"]
                        .as_str()
                        .ok_or_else(|| wasmtime::Error::msg("读取缺少 id"))?
                        .to_owned();
                    let consumed_keys: Option<Vec<String>> =
                        serde_json::from_value(request["consumed_keys"].clone())
                            .map_err(|e| wasmtime::Error::msg(e.to_string()))?;
                    if ValueId::parse(&id).is_none() {
                        return Ok(-1);
                    }
                    let (reply, answer) = mpsc::channel();
                    caller
                        .data()
                        .events
                        .send(Event::Read(ReadRequest {
                            kind,
                            id,
                            consumed_keys,
                            limit,
                            reply,
                        }))
                        .map_err(host)
                        .map_err(|e| wasmtime::Error::msg(e.0))?;
                    let bytes = answer
                        .recv()
                        .map_err(host)
                        .and_then(|result| result)
                        .map_err(|e| wasmtime::Error::msg(e.0))?;
                    let Some(bytes) = bytes else { return Ok(-1) };
                    if bytes.len() > limit || bytes.len() > i32::MAX as usize {
                        return Err(wasmtime::Error::msg("读取结果超出宿主消息上限"));
                    }
                    if cap != 0 {
                        if cap as usize != bytes.len() {
                            return Err(wasmtime::Error::msg("读取容量不符"));
                        }
                        memory
                            .write(&mut caller, out as usize, &bytes)
                            .map_err(host)
                            .map_err(|e| wasmtime::Error::msg(e.0))?;
                    }
                    Ok(bytes.len() as i32)
                },
            )
            .map_err(host)?;
        let memory = StoreLimitsBuilder::new()
            .memory_size(limits.memory_bytes)
            .trap_on_grow_failure(true)
            .build();
        let mut store = Store::new(
            module.engine(),
            HostState {
                events,
                memory,
                message_limit: limits.message_bytes,
                deadline: Instant::now() + limits.time,
            },
        );
        store.limiter(|state| &mut state.memory);
        store.epoch_deadline_callback(|context| {
            if Instant::now() >= context.data().deadline {
                Err(wasmtime::Error::msg("宿主时间保护耗尽"))
            } else {
                Ok(wasmtime::UpdateDeadline::Continue(1))
            }
        });
        store.set_fuel(limits.fuel).map_err(host)?;
        store.set_epoch_deadline(1);
        let _deadline = Deadline::start(module.engine().clone(), limits.time)?;
        let instance = linker.instantiate(&mut store, module).map_err(host)?;
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| HostError("guest 没有 memory".into()))?;
        let alloc = instance.get_typed_func(&mut store, "alloc").map_err(host)?;
        let call = instance.get_typed_func(&mut store, "call").map_err(host)?;
        Ok(Self {
            store,
            memory,
            alloc,
            call,
        })
    }
    fn invoke(&mut self, input: Json, limits: &Limits) -> Result<Json, HostError> {
        if self.memory.data_size(&self.store) > limits.memory_bytes {
            return Err(HostError("guest 已用内存超出调整后的宿主上限".into()));
        }
        self.store.data_mut().deadline = Instant::now() + limits.time;
        self.store.data_mut().message_limit = limits.message_bytes;
        self.store.data_mut().memory = StoreLimitsBuilder::new()
            .memory_size(limits.memory_bytes)
            .trap_on_grow_failure(true)
            .build();
        self.store.set_fuel(limits.fuel).map_err(host)?;
        self.store.set_epoch_deadline(1);
        let _deadline = Deadline::start(self.store.engine().clone(), limits.time)?;
        check_message("in", &input)?;
        let bytes = serde_json::to_vec(&input).map_err(host)?;
        if bytes.len() > limits.message_bytes {
            return Err(HostError("输入超出宿主消息上限".into()));
        }
        let len = u32::try_from(bytes.len()).map_err(host)?;
        let ptr = self.alloc.call(&mut self.store, len).map_err(host)?;
        checked_range(
            self.memory.data_size(&self.store),
            ptr,
            len,
            limits.message_bytes,
        )?;
        self.memory
            .write(&mut self.store, ptr as usize, &bytes)
            .map_err(host)?;
        let packed = self.call.call(&mut self.store, (ptr, len)).map_err(host)?;
        let range = checked_range(
            self.memory.data_size(&self.store),
            packed as u32,
            (packed >> 32) as u32,
            limits.message_bytes,
        )?;
        // 范围与上限均已通过；解析直接借用 guest 切片，不再按输出长度分配宿主 Vec。
        let output = serde_json::from_slice(&self.memory.data(&self.store)[range])
            .map_err(|e| HostError(format!("guest 输出 JSON：{e}")))?;
        check_message("out", &output)?;
        Ok(output)
    }
}

struct Worker {
    commands: Option<mpsc::Sender<Json>>,
    events: mpsc::Receiver<Event>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    fn new(instance: &str, module: Module, limits: Arc<RwLock<Limits>>) -> Result<Self, HostError> {
        let (commands, requests) = mpsc::channel::<Json>();
        let (events_tx, events) = mpsc::channel();
        let thread = thread::Builder::new()
            .name(format!("lang0-wasm-{instance}"))
            .spawn(move || {
                let mut guest: Option<Guest> = None;
                while let Ok(input) = requests.recv() {
                    let result = (|| {
                        let limits = snapshot(&limits)?;
                        if guest.is_none() {
                            guest = Some(Guest::new(&module, events_tx.clone(), &limits)?);
                        }
                        guest
                            .as_mut()
                            .ok_or_else(|| HostError("guest 不存在".into()))?
                            .invoke(input, &limits)
                    })();
                    let failed = result.is_err();
                    if failed {
                        guest.take();
                    }
                    if events_tx.send(Event::Completed(result)).is_err() || failed {
                        break;
                    }
                }
            })
            .map_err(host)?;
        Ok(Self {
            commands: Some(commands),
            events,
            thread: Some(thread),
        })
    }
    fn call(&mut self, input: Json, read: &dyn Read) -> Result<Json, HostError> {
        let commands = self
            .commands
            .as_ref()
            .ok_or_else(|| HostError("WASM 单元已丢弃".into()))?;
        commands.send(input).map_err(host)?;
        loop {
            match self
                .events
                .recv()
                .map_err(|_| HostError("WASM 单元退出或 panic".into()))?
            {
                Event::Read(request) => {
                    let response = (|| {
                        let bytes = match (request.kind, request.consumed_keys.as_deref()) {
                            (0, None) => read.code_bytes(&request.id)?,
                            (0, Some(keys)) => read.bytes(&request.id, keys)?,
                            (1, keys) => {
                                let value = match keys {
                                    None => read.code_json(&request.id)?,
                                    Some(keys) => read.json(&request.id, keys)?,
                                };
                                value
                                    .map(|value| serde_json::to_vec(&value).map_err(host))
                                    .transpose()?
                            }
                            (2, keys) => {
                                let value = match keys {
                                    None => read.code_bundle(&request.id)?,
                                    Some(keys) => read.bundle(&request.id, keys)?,
                                };
                                value.map(|value| serde_json::to_vec(&serde_json::json!({"content":value.content,"dependencies":value.dependencies})).map_err(host)).transpose()?
                            }
                            _ => return Err(HostError("未知读取种类".into())),
                        };
                        if bytes
                            .as_ref()
                            .is_some_and(|bytes| bytes.len() > request.limit)
                        {
                            return Err(HostError("读取结果超出宿主消息上限".into()));
                        }
                        Ok(bytes)
                    })();
                    request.reply.send(response).map_err(host)?;
                }
                Event::Completed(result) => {
                    if result.is_err() {
                        self.close();
                    }
                    return result;
                }
            }
        }
    }
    fn close(&mut self) {
        self.commands.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.close();
    }
}

fn successful(out: &Json, value: bool) -> bool {
    out.as_object().is_some_and(|object| {
        object.len() == if value { 2 } else { 1 }
            && object.get("ok") == Some(&Json::Bool(true))
            && (!value || object.contains_key("value"))
    })
}
fn protocol_error(out: Json) -> HostError {
    HostError(format!("guest 响应不合调用协议：{out}"))
}
impl Language for WasmLang0 {
    fn id(&self) -> &str {
        "lang0@1"
    }
    fn check(&self, definition: &Json, read: &dyn Read) -> Result<(), CheckError> {
        let mut worker = self.worker("check")?;
        let out = worker.call(json!({"op":"check", "definition":definition}), read)?;
        if successful(&out, false) {
            return Ok(());
        }
        if let Some(object) = out.as_object() {
            if object.len() == 2 && object.get("ok") == Some(&Json::Bool(false)) {
                if let Some(message) = object.get("message").and_then(Json::as_str) {
                    return Err(CheckError::Rejected(message.into()));
                }
            }
        }
        Err(CheckError::Host(protocol_error(out)))
    }
    fn open(
        &self,
        instance: &str,
        birth: &Json,
        results: &BTreeMap<String, Json>,
        read: &dyn Read,
    ) -> Result<Box<dyn Session>, HostError> {
        let mut worker = self.worker(instance)?;
        let out = worker.call(
            json!({"op":"open", "instance":instance, "birth":birth, "results":results}),
            read,
        )?;
        if !successful(&out, false) {
            return Err(protocol_error(out));
        }
        Ok(Box::new(WasmSession { worker }))
    }
}
struct WasmSession {
    worker: Worker,
}
impl Session for WasmSession {
    fn resume(
        &mut self,
        results: &BTreeMap<String, Json>,
        read: &dyn Read,
    ) -> Result<Json, HostError> {
        let out = self
            .worker
            .call(json!({"op":"resume", "results":results}), read)?;
        if !successful(&out, true) {
            self.worker.close();
            return Err(protocol_error(out));
        }
        Ok(out["value"].clone())
    }
}
