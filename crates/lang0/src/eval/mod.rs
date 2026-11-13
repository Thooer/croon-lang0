//! 顺序求值器：栈、帧、闭包与待结果的 Atom 都是数据，不借宿主栈挂起。
//! 参考：docs/Lang0/Engineering.md（选择、降）；docs/Lang0/Evaluator.md（求值、键、资源）。
mod bridge;
mod builtin;
mod compile;
mod heap;
mod msg;
mod ops;
#[cfg(test)]
mod tests;
mod text;
mod unicode;
mod value;

use crate::modules::Program;
use compile::{Code, Op, Push};
use croon_eval::host::{Read, Session};
use heap::{Heap, Root, Slot};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};
use text::R;
use value::*;

// B、深度上限、L 与活数据量的下限：试验值，不是冻结值；发布数值在 Evaluator.md 的待定里。
const STEPS: u64 = 32 * 1024 * 1024;
const DEPTH: usize = 128;
const LIVE: u64 = 8 * 1024 * 1024;
const LIVE_FLOOR: u64 = 4096;

struct Frame {
    code: Rc<Code>,
    pc: usize,
    cells: Vec<Cell>,
    stack: Vec<Value>,
    blocks: Vec<(usize, usize)>,
    handlers: Vec<Option<Value>>,
    exception: Option<Value>,
    output: Option<Json>,
}
impl Frame {
    fn new(code: Rc<Code>, cells: Vec<Cell>, output: Option<Json>) -> Self {
        Self {
            code,
            pc: 0,
            cells,
            stack: vec![],
            blocks: vec![],
            handlers: vec![],
            exception: None,
            output,
        }
    }
    fn pop(&mut self) -> Value {
        self.stack.pop().expect("IR 栈下溢")
    }
    fn take(&mut self, n: usize) -> Vec<Value> {
        self.stack.split_off(self.stack.len() - n)
    }
    /// 栈顶往下第 `back` 个（1 是栈顶）：编译器在那里放了 BuildList 造的 list。
    fn list_at(&self, back: usize) -> Slot<Vec<Value>> {
        let Value::List(xs) = self.stack[self.stack.len() - back] else {
            unreachable!("编译器在这里放了 BuildList")
        };
        xs
    }
    /// 同上，BuildDict 造的 dict。
    fn dict_at(&self, back: usize) -> Slot<Dict> {
        let Value::Dict(d) = self.stack[self.stack.len() - back] else {
            unreachable!("编译器在这里放了 BuildDict")
        };
        d
    }
}

enum PendingKind {
    Send(Json),
    Receive,
}
struct Pending {
    key: String,
    atom: Json,
    kind: PendingKind,
    reported: bool,
}

struct Selection {
    owner: usize,
    name: String,
    items: Vec<Value>,
    keys: Vec<Value>,
    key: Value,
    reverse: bool,
    awaiting: bool,
}

pub(crate) struct Vm {
    heap: Heap,
    baseline: u64,
    last_live: u64,
    max_live: u64,
    live_floor: u64,
    program: Program,
    codes: BTreeMap<String, Rc<Code>>,
    globals: BTreeMap<String, BTreeMap<String, Value>>,
    loaded: BTreeSet<String>,
    frames: Vec<Frame>,
    meter: ops::Meter,
    birth: Option<Json>,
    consumed_keys: Vec<String>,
    host_error: Option<croon_eval::host::HostError>,
    /// 模块的顶层已经求值完，基线已经量过。
    constructed: bool,
    finished: Option<Json>,
    final_value: Option<Value>,
    pending: Option<Pending>,
    results: BTreeMap<String, Json>,
    next_key: u64,
    selections: Vec<Selection>,
}

impl Vm {
    pub fn new(program: Program, birth: Option<Json>, results: BTreeMap<String, Json>) -> Self {
        let codes = compile::program(&program);
        let mut vm = Self {
            heap: Heap::default(),
            baseline: 0,
            last_live: 0,
            max_live: LIVE,
            live_floor: LIVE_FLOOR,
            program,
            codes,
            globals: BTreeMap::new(),
            loaded: BTreeSet::new(),
            frames: vec![],
            meter: ops::Meter {
                steps: 0,
                max_steps: STEPS,
                depth: DEPTH,
            },
            birth,
            consumed_keys: vec![],
            host_error: None,
            constructed: false,
            finished: None,
            final_value: None,
            pending: None,
            results,
            next_key: 0,
            selections: vec![],
        };
        vm.load_module("");
        vm
    }
    fn roots(&self) -> Vec<Root> {
        let mut roots: Vec<_> = self
            .globals
            .values()
            .flat_map(|g| g.values().cloned())
            .map(Root::Value)
            .collect();
        for f in &self.frames {
            roots.extend(f.cells.iter().copied().map(Root::Cell));
            roots.extend(f.stack.iter().cloned().map(Root::Value));
            roots.extend(f.handlers.iter().flatten().cloned().map(Root::Value));
            roots.extend(f.exception.iter().cloned().map(Root::Value));
        }
        for s in &self.selections {
            roots.extend(s.items.iter().chain(&s.keys).cloned().map(Root::Value));
            roots.push(Root::Value(s.key.clone()));
        }
        roots.extend(self.final_value.iter().cloned().map(Root::Value));
        roots
    }
    fn collect_live(&mut self) -> R<()> {
        self.last_live = self.heap.collect(self.roots());
        if self.last_live.saturating_sub(self.baseline) > self.max_live {
            Err(msg::limit(&self.heap, "live"))
        } else {
            Ok(())
        }
    }
    fn load_module(&mut self, name: &str) {
        if self.loaded.insert(name.into()) {
            self.globals.entry(name.into()).or_default();
            if let Some(c) = self.codes.get(name) {
                self.frames.push(Frame::new(c.clone(), vec![], None));
            }
        }
    }
    fn frame(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("有帧")
    }
    fn pop(&mut self) -> Value {
        self.frame().pop()
    }
    fn push(&mut self, v: Value) {
        self.frame().stack.push(v)
    }
    fn name(&self, n: usize) -> String {
        self.frames.last().unwrap().code.names[n].clone()
    }
    fn global(&self, module: &str, name: &str) -> R<Value> {
        if let Some(v) = self.globals.get(module).and_then(|g| g.get(name)) {
            return Ok(v.clone());
        }
        if let Some(v) = builtin::lookup(name) {
            return Ok(v);
        }
        Err(msg::err(
            &self.heap,
            "undefined_name",
            None,
            &[("name", name.into())],
        ))
    }
    fn fail(&mut self, e: Value) {
        loop {
            let Some(_) = self.frames.last() else {
                self.finished = Some(bridge::outcome(&self.heap, &e));
                return;
            };
            self.selections.retain(|s| s.owner < self.frames.len());
            let f = self.frames.last_mut().unwrap();
            if let Some((pc, stack)) = f.blocks.pop() {
                f.stack.truncate(stack);
                f.pc = pc;
                f.exception = Some(e);
                return;
            }
            self.frames.pop();
        }
    }
    fn call_function(
        &mut self,
        f: Slot<Function>,
        args: Vec<Value>,
        kw: Vec<(String, Value)>,
        output: Option<Json>,
    ) -> R<()> {
        // 调用层数与深遍历合成一个累计值：帧也占深度，见 Evaluator.md「资源」。
        if self.meter.depth == 0 {
            return Err(msg::limit(&self.heap, "depth"));
        }
        self.meter.charge(&self.heap, 1)?;
        let bound = builtin::bind_function(&self.heap, &f.borrow(&self.heap), args, kw)?;
        let mut cells: Vec<Cell> = (0..f.borrow(&self.heap).code.nlocals)
            .map(|_| self.heap.alloc(None))
            .collect();
        for (i, v) in bound.into_iter().enumerate() {
            *cells[i].borrow_mut(&self.heap) = Some(v)
        }
        cells.extend(f.borrow(&self.heap).cells.iter().cloned());
        self.frames
            .push(Frame::new(f.borrow(&self.heap).code.clone(), cells, output));
        Ok(())
    }
    fn call(
        &mut self,
        f: Value,
        args: Vec<Value>,
        kw: Vec<(String, Value)>,
        read: &dyn Read,
    ) -> R<()> {
        match f {
            Value::Function(f) => self.call_function(f, args, kw, None),
            Value::Tool(t) => {
                let t = t.borrow(&self.heap).clone();
                if args.len() != 1 || !kw.is_empty() {
                    return Err(builtin::arity(&self.heap, &t.name, 1, 1, args.len()));
                }
                let data = bridge::check(&self.heap, &t.input, &args[0], "input", &mut self.meter)?;
                let kwargs = data
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.clone(), bridge::decode(&self.heap, v)))
                    .collect();
                self.call_function(t.f, vec![], kwargs, t.output.clone())
            }
            other => {
                if let Some(v) = self.builtin(other, args, kw, read)? {
                    self.push(v)
                }
                Ok(())
            }
        }
    }
    fn atom(&mut self, mut atom: Json, kind: PendingKind) {
        let key = self.next_key.to_string();
        self.next_key += 1;
        atom["key"] = json!(key);
        self.pending = Some(Pending {
            key,
            atom,
            kind,
            reported: false,
        });
        self.meter.steps = 0;
    }
    fn start_main(&mut self) -> R<()> {
        let b = self.birth.clone().unwrap();
        let iface = &b["definition"]["interface"];
        let mut kw: Vec<_> = b["input"]["value"]
            .as_object()
            .expect("Kernel 已按 Lang0 Signature 检查 input 是对象")
            .iter()
            .map(|(k, v)| (k.clone(), bridge::decode(&self.heap, v)))
            .collect();
        for (k, s) in iface["requires"].as_object().unwrap() {
            kw.push((
                k.clone(),
                Value::Ref(self.heap.alloc(RefV {
                    target: json!({"port":k}),
                    spec: Spec::Inline(s.clone()),
                })),
            ))
        }
        for (k, _) in iface["provides"].as_object().unwrap() {
            kw.push((k.clone(), Value::Inbox(k.as_str().into())))
        }
        let Value::Function(f) = self.global("", "main").expect("构造保证 main 已绑定")
        else {
            panic!("构造保证 main 是唯一的顶层函数");
        };
        self.final_value = None;
        self.meter.steps = 0;
        self.call_function(f, vec![], kw, None)
    }
    pub fn check_top(&mut self, read: &dyn Read) -> Result<(), croon_eval::host::CheckError> {
        self.run(read)?;
        match &self.finished {
            Some(o) if o["ok"] == false => {
                Err(format!("/body: {}: {}", o["error"]["kind"], o["error"]["message"]).into())
            }
            _ => Ok(()),
        }
    }
    fn run(&mut self, read: &dyn Read) -> Result<Json, croon_eval::host::HostError> {
        loop {
            if let Some(error) = self.host_error.take() {
                return Err(error);
            }
            if let Some(o) = &self.finished {
                return Ok(json!({"done":o}));
            }
            self.meter.depth = DEPTH.saturating_sub(self.frames.len());
            if self.heap.produced.get() >= self.last_live.max(self.live_floor)
                && let Err(e) = self.collect_live()
            {
                self.fail(e);
                continue;
            }
            if let Some(p) = self.pending.take() {
                if let Some(result) = self.results.get(&p.key).cloned() {
                    self.consumed_keys.push(p.key.clone());
                    let v = match p.kind {
                        PendingKind::Send(own) => bridge::sent(&self.heap, &result, &own),
                        PendingKind::Receive => Ok(bridge::received(
                            &self.heap,
                            &result,
                            &self.birth.as_ref().unwrap()["definition"]["interface"]["provides"],
                        )),
                    };
                    match v {
                        Ok(v) => self.push(v),
                        Err(e) => self.fail(e),
                    }
                    continue;
                }
                let reported = p.reported;
                let atom = p.atom.clone();
                self.pending = Some(Pending {
                    reported: true,
                    ..p
                });
                return Ok(if reported {
                    json!({"atoms":[]})
                } else {
                    json!({"atoms":[atom]})
                });
            }
            if self.frames.is_empty() {
                if !self.constructed {
                    // 构造结束：量一次，得基线；构造时的顶层与运行受同样的 L。
                    self.constructed = true;
                    if let Err(e) = self.collect_live() {
                        self.fail(e);
                        continue;
                    }
                    self.baseline = self.last_live;
                    if self.birth.is_some()
                        && let Err(e) = self.start_main()
                    {
                        self.fail(e)
                    }
                    continue;
                }
                let v = self.final_value.take().unwrap_or(Value::None);
                let (v, dependencies) = match v {
                    Value::Delivery(d) => (d.value.clone(), d.dependencies.clone()),
                    v => (v, json!([])),
                };
                let out = match bridge::encode(&self.heap, &v, "output", &mut self.meter) {
                    Ok(v) => json!({"ok":true,"value":v,"dependencies":dependencies}),
                    Err(e) => bridge::outcome(&self.heap, &e),
                };
                self.finished = Some(out);
                continue;
            }
            if self
                .selections
                .last()
                .is_some_and(|s| s.owner == self.frames.len())
            {
                if let Err(e) = self.selection_step(read) {
                    self.fail(e)
                }
                continue;
            }
            let op = {
                let f = self.frame();
                let op = f.code.ops[f.pc].clone();
                f.pc += 1;
                op
            };
            if let Err(e) = self.step(op, read) {
                self.fail(e)
            }
        }
    }
    fn selection_step(&mut self, read: &dyn Read) -> R<()> {
        let mut s = self.selections.pop().unwrap();
        if s.awaiting {
            s.keys.push(self.pop());
            s.awaiting = false;
        }
        if s.keys.len() < s.items.len() {
            let arg = s.items[s.keys.len()].clone();
            let key = s.key.clone();
            s.awaiting = true;
            self.selections.push(s);
            self.call(key, vec![arg], vec![], read)?;
            return Ok(());
        }
        let out = self.select_values(&s.name, s.items, s.keys, s.reverse)?;
        self.push(out);
        Ok(())
    }
    fn select_values(
        &mut self,
        name: &str,
        items: Vec<Value>,
        keys: Vec<Value>,
        reverse: bool,
    ) -> R<Value> {
        let mut order: Vec<usize> = (0..items.len()).collect();
        if name == "sorted" {
            // 稳定归并排序；比较期间不调用用户代码。key 的结果已经顺序算完。
            let mut width = 1;
            while width < order.len() {
                let mut next = Vec::with_capacity(order.len());
                for block in order.chunks(width * 2) {
                    let mid = width.min(block.len());
                    let (mut l, mut r) = (0, mid);
                    while l < mid && r < block.len() {
                        self.meter.charge(&self.heap, 1)?;
                        let take_right = if reverse {
                            ops::order(
                                &self.heap,
                                &keys[block[l]],
                                &keys[block[r]],
                                compile::Cmp::Lt,
                                self.meter.depth,
                            )?
                        } else {
                            ops::order(
                                &self.heap,
                                &keys[block[r]],
                                &keys[block[l]],
                                compile::Cmp::Lt,
                                self.meter.depth,
                            )?
                        };
                        if take_right {
                            next.push(block[r]);
                            r += 1
                        } else {
                            next.push(block[l]);
                            l += 1
                        }
                    }
                    next.extend_from_slice(&block[l..mid]);
                    next.extend_from_slice(&block[r..]);
                }
                order = next;
                width = width.saturating_mul(2);
            }
            Ok(Value::list(
                &self.heap,
                order.into_iter().map(|i| items[i].clone()).collect(),
            ))
        } else {
            let mut best = 0;
            for i in 1..items.len() {
                self.meter.charge(&self.heap, 1)?;
                if ops::order(
                    &self.heap,
                    &keys[i],
                    &keys[best],
                    if name == "min" {
                        compile::Cmp::Lt
                    } else {
                        compile::Cmp::Gt
                    },
                    self.meter.depth,
                )? {
                    best = i
                }
            }
            Ok(items[best].clone())
        }
    }
    fn step(&mut self, op: Op, read: &dyn Read) -> R<()> {
        match op {
            Op::Const(i) => {
                let v = self.frame().code.consts[i].clone();
                self.push(v)
            }
            Op::Load(i) => {
                let f = self.frames.last().expect("Load 有帧");
                let v = f.cells[i].borrow(&self.heap).clone();
                let name = f.code.slots[i].clone();
                let free = i >= f.code.nlocals;
                self.push(v.ok_or_else(|| {
                    msg::err(
                        &self.heap,
                        if free {
                            "unbound_free"
                        } else {
                            "unbound_local"
                        },
                        None,
                        &[("name", name)],
                    )
                })?)
            }
            Op::Store(i) => {
                let v = self.pop();
                *self.frame().cells[i].borrow_mut(&self.heap) = Some(v)
            }
            Op::Unbind(i) => *self.frame().cells[i].borrow_mut(&self.heap) = None,
            Op::LoadGlobal(i) => {
                let name = self.name(i);
                let module = self.frame().code.module.clone();
                let v = self.global(&module, &name)?;
                self.push(v)
            }
            Op::StoreGlobal(i) => {
                let name = self.name(i);
                let module = self.frame().code.module.clone();
                let v = self.pop();
                self.globals.get_mut(&module).unwrap().insert(name, v);
            }
            Op::UnbindGlobal(i) => {
                let name = self.name(i);
                let module = self.frame().code.module.clone();
                self.globals.get_mut(&module).unwrap().remove(&name);
            }
            Op::Pop => {
                self.pop();
            }
            Op::Dup => {
                let v = self
                    .frames
                    .last()
                    .expect("IR 有帧")
                    .stack
                    .last()
                    .unwrap()
                    .clone();
                self.push(v)
            }
            Op::Dup2 => {
                let n = self.frame().stack.len();
                let v = self.frame().stack[n - 2..].to_vec();
                self.frame().stack.extend(v)
            }
            Op::Rot2 | Op::Rot3 => {
                let n = if matches!(op, Op::Rot2) { 2 } else { 3 };
                let len = self.frame().stack.len();
                self.frame().stack[len - n..].rotate_right(1)
            }
            Op::BuildList(n) | Op::BuildTuple(n) => {
                self.meter.charge(&self.heap, n as u64)?;
                let v = self.frame().take(n);
                self.push(if matches!(op, Op::BuildList(_)) {
                    Value::list(&self.heap, v)
                } else {
                    Value::tuple(&self.heap, v)
                })
            }
            Op::ListAppend => {
                let v = self.pop();
                self.frame().list_at(1).push(&self.heap, v);
                self.meter.charge(&self.heap, 1)?
            }
            Op::ListExtend { call } => {
                let v = self.pop();
                if ops::iter(&self.heap, &v).is_err() {
                    let message = if call {
                        let frame = self.frames.last().expect("ListExtend 有帧");
                        let f = &frame.stack[frame.stack.len() - 2];
                        let name = builtin::call_name(&self.heap, f);
                        msg::text(
                            "star_not_iterable",
                            None,
                            &[("name", name), ("type", v.type_name(&self.heap))],
                        )
                    } else {
                        msg::text(
                            "star_value_not_iterable",
                            None,
                            &[("type", v.type_name(&self.heap))],
                        )
                    };
                    return Err(msg::exc(
                        &self.heap,
                        "TypeError",
                        vec![Value::str(&self.heap, message)],
                        None,
                    ));
                }
                let mut out = vec![];
                ops::collect(&self.heap, &v, &mut out, &mut self.meter)?;
                self.frame().list_at(1).extend(&self.heap, out)
            }
            Op::ListToTuple => {
                let items = self.frame().list_at(1).borrow(&self.heap).clone();
                self.pop();
                self.push(Value::tuple(&self.heap, items))
            }
            Op::BuildDict => self.push(Value::dict(&self.heap, Dict::default())),
            Op::DictInsert { call } => {
                let v = self.pop();
                let k = self.pop();
                let kk = ops::key(&self.heap, &k, self.meter.depth)?;
                let frame = self.frames.last().expect("IR 有帧");
                if call && frame.dict_at(1).borrow(&self.heap).get(&kk).is_some() {
                    // 调用里的栈是 f、位置参数的 list、关键字的 dict。
                    let name = builtin::call_name(&self.heap, &frame.stack[frame.stack.len() - 3]);
                    let Value::Str(key) = &k else {
                        unreachable!("调用里的关键字参数名是字符串")
                    };
                    return Err(msg::err(
                        &self.heap,
                        "multiple_kw_values",
                        None,
                        &[("name", name), ("key", key.to_string())],
                    ));
                }
                self.frame().dict_at(1).insert(&self.heap, kk, k, v);
                self.meter.charge(&self.heap, 1)?
            }
            Op::DictMerge { call } => {
                let v = self.pop();
                let frame = self.frames.last().expect("IR 有帧");
                let d = frame.dict_at(1);
                // 调用里的栈是 f、位置参数的 list、关键字的 dict。
                let name = call
                    .then(|| builtin::call_name(&self.heap, &frame.stack[frame.stack.len() - 3]));
                self.merge_dict(d, &v, name.as_deref())?
            }
            Op::CompAppend(n) => {
                let v = self.pop();
                self.frame().list_at(n).push(&self.heap, v);
                self.meter.charge(&self.heap, 1)?
            }
            Op::CompInsert(n) => {
                let v = self.pop();
                let k = self.pop();
                let kk = ops::key(&self.heap, &k, self.meter.depth)?;
                self.frame().dict_at(n).insert(&self.heap, kk, k, v);
                self.meter.charge(&self.heap, 1)?
            }
            Op::Binary(operator) | Op::InPlace(operator) => {
                let inplace = matches!(op, Op::InPlace(_));
                let b = self.pop();
                let a = self.pop();
                let v = ops::binary(&self.heap, &a, &b, operator, inplace, &mut self.meter)?;
                self.push(v)
            }
            Op::Unary(op) => {
                let a = self.pop();
                let v = ops::unary(&self.heap, &a, op, &mut self.meter)?;
                self.push(v)
            }
            Op::Compare(op) => {
                let b = self.pop();
                let a = self.pop();
                let v = ops::compare(&self.heap, &a, &b, op, &mut self.meter)?;
                self.push(v)
            }
            Op::GetAttr(i) => {
                let name = self.name(i);
                let a = self.pop();
                let v = self.attr(&a, &name)?;
                self.push(v)
            }
            Op::StoreAttr(i) => {
                let name = self.name(i);
                let a = self.pop();
                self.pop();
                return Err(msg::err(
                    &self.heap,
                    "no_attribute",
                    None,
                    &[("type", a.type_name(&self.heap)), ("name", name)],
                ));
            }
            Op::GetItem => {
                let i = self.pop();
                let a = self.pop();
                let v = ops::getitem(&self.heap, &a, &i, &mut self.meter)?;
                self.push(v)
            }
            Op::StoreItem => {
                let i = self.pop();
                let a = self.pop();
                let v = self.pop();
                ops::setitem(&self.heap, &a, &i, v, &mut self.meter)?
            }
            Op::Call { pos, kw } => {
                let vals = self.frame().take(kw.len());
                let kw = kw.iter().map(|i| self.name(*i)).zip(vals).collect();
                let args = self.frame().take(pos);
                let f = self.pop();
                self.call(f, args, kw, read)?
            }
            Op::CallEx => {
                let kw = self.frame().dict_at(1);
                let args = self.frame().list_at(2);
                self.frame().take(2);
                let f = self.pop();
                let mut pairs = vec![];
                for (k, v) in kw.borrow(&self.heap).items() {
                    let Value::Str(k) = k else {
                        return Err(msg::err(&self.heap, "keywords_not_str", None, &[]));
                    };
                    pairs.push((k.to_string(), v.clone()))
                }
                let args = args.borrow(&self.heap).clone();
                self.call(f, args, pairs, read)?
            }
            Op::MakeFunction { code, defaults } => {
                let defaults = self.frame().take(defaults);
                let f = self.frame();
                let cells = f.code.cellmaps[code].iter().map(|i| f.cells[*i]).collect();
                let code = f.code.children[code].clone();
                self.push(Value::Function(self.heap.alloc(Function {
                    code,
                    defaults,
                    cells,
                })))
            }
            Op::GetIter => {
                let v = self.pop();
                let it = ops::iter(&self.heap, &v)?;
                self.push(Value::Iter(it))
            }
            Op::ForIter(end) => {
                let Value::Iter(it) = *self.frame().stack.last().expect("IR 栈下溢") else {
                    unreachable!("ForIter 之前是 GetIter")
                };
                if let Some(v) = ops::next_any(&self.heap, &it)? {
                    self.push(v)
                } else {
                    self.pop();
                    self.frame().pc = end
                }
            }
            Op::Jump(pc) => self.frame().pc = pc,
            Op::JumpIfFalse(pc) => {
                if !self.pop().truthy(&self.heap) {
                    self.frame().pc = pc
                }
            }
            Op::JumpIfFalseOrPop(pc) | Op::JumpIfTrueOrPop(pc) => {
                let yes = self
                    .frames
                    .last()
                    .expect("条件跳转有帧")
                    .stack
                    .last()
                    .unwrap()
                    .truthy(&self.heap);
                if yes == matches!(op, Op::JumpIfTrueOrPop(_)) {
                    self.frame().pc = pc
                } else {
                    self.pop();
                }
            }
            Op::Return => {
                let v = self.pop();
                let f = self.frames.pop().unwrap();
                if let Some(s) = f.output {
                    bridge::check(&self.heap, &s, &v, "output", &mut self.meter)?;
                }
                if !f.code.is_module {
                    if self.frames.is_empty() {
                        self.final_value = Some(v)
                    } else {
                        self.push(v)
                    }
                }
            }
            Op::SetupExcept(pc) | Op::SetupFinally(pc) => {
                let n = self.frame().stack.len();
                self.frame().blocks.push((pc, n))
            }
            Op::PopBlock => {
                self.frame().blocks.pop();
            }
            Op::EnterHandler => {
                let e = self.frame().exception.clone();
                self.frame().handlers.push(e)
            }
            Op::ExitHandler => {
                self.frame().handlers.pop();
                self.frame().exception = self.frame().handlers.last().cloned().flatten()
            }
            Op::LoadExc => {
                let e = self.frame().exception.clone().expect("处理异常");
                self.push(e)
            }
            Op::CheckMatch => {
                let typ = self.pop();
                let e = self.pop();
                self.push(Value::Bool(builtin::exception_matches(
                    &self.heap, &e, &typ,
                )?))
            }
            Op::Raise => {
                let e = self.pop();
                return Err(match e {
                    Value::Exc(_) => e,
                    Value::Class(Class::Exc(c)) => msg::exc(&self.heap, c, vec![], None),
                    _ => msg::err(&self.heap, "raise_non_exception", None, &[]),
                });
            }
            Op::Reraise | Op::EndFinally => {
                return Err(self
                    .frame()
                    .exception
                    .clone()
                    .unwrap_or_else(|| msg::err(&self.heap, "reraise_none", None, &[])));
            }
            Op::Unpack { n, star } => {
                let v = self.pop();
                let xs = ops::unpack(&self.heap, &v, n, star, &mut self.meter)?;
                self.frame().stack.extend(xs.into_iter().rev())
            }
            Op::FormatValue => {
                let v = self.pop();
                let s = text::metered_str(&self.heap, &v, &mut self.meter)?;
                self.push(Value::str(&self.heap, s))
            }
            Op::BuildString(n) => {
                let xs = self.frame().take(n);
                let mut s = String::new();
                for x in xs {
                    let Value::Str(x) = x else {
                        unreachable!("BuildString 的每段是常量或 FormatValue 的结果")
                    };
                    self.meter.charge(&self.heap, x.len() as u64)?;
                    s.push_str(&x)
                }
                self.push(Value::str(&self.heap, s))
            }
            Op::Import(i) => {
                let imp = self.frame().code.imports[i].clone();
                if let Some(name) = imp.chain.iter().find(|m| !self.loaded.contains(*m)) {
                    let name = name.clone();
                    self.frame().pc -= 1;
                    self.load_module(&name)
                } else {
                    let v = match imp.push {
                        Push::Module(m) => Value::Module(m.into()),
                        Push::Attr(m, n) => self.attr(&Value::Module(m.into()), &n)?,
                    };
                    self.push(v)
                }
            }
            Op::Tick => self.meter.charge(&self.heap, 1)?,
        }
        Ok(())
    }
}
impl Session for Vm {
    fn resume(
        &mut self,
        results: &BTreeMap<String, Json>,
        read: &dyn Read,
    ) -> Result<Json, croon_eval::host::HostError> {
        self.results.extend(results.clone());
        self.run(read)
    }
}
