//! Instance 独占的槽堆。槽间只存下标，回收不依赖引用计数或宿主地址。
//! 参考：docs/Lang0/Engineering.md（堆）；docs/Lang0/Evaluator.md（资源）。

use super::value::*;
use std::cell::{Cell as Counter, Ref, RefCell, RefMut};
use std::marker::PhantomData;
use std::rc::Rc;

pub struct Slot<T> {
    index: usize,
    marker: PhantomData<fn() -> T>,
}
impl<T> Copy for Slot<T> {}
impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> PartialEq for Slot<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
impl<T> Eq for Slot<T> {}
impl<T: Stored> Slot<T> {
    pub fn borrow(self, heap: &Heap) -> Ref<'_, T> {
        Ref::map(T::arena(heap).entries.borrow(), |a| {
            a[self.index].as_ref().expect("根可达的槽尚未释放")
        })
    }
    pub fn borrow_mut(self, heap: &Heap) -> RefMut<'_, T> {
        RefMut::map(T::arena(heap).entries.borrow_mut(), |a| {
            a[self.index].as_mut().expect("根可达的槽尚未释放")
        })
    }
}

/// 往 list、dict 里放值也是产出，计入触发量活数据的产出量；见 Evaluator.md「资源」。
impl Slot<Vec<Value>> {
    pub fn push(self, heap: &Heap, v: Value) {
        self.extend(heap, [v])
    }
    pub fn extend(self, heap: &Heap, items: impl IntoIterator<Item = Value>) {
        let mut list = self.borrow_mut(heap);
        let before = list.len();
        list.extend(items);
        heap.produce((list.len() - before) as u64);
    }
    pub fn replace(self, heap: &Heap, items: Vec<Value>) {
        heap.produce(items.len() as u64);
        *self.borrow_mut(heap) = items;
    }
}
impl Slot<Dict> {
    pub fn insert(self, heap: &Heap, k: Key, key: Value, value: Value) {
        heap.produce(1);
        self.borrow_mut(heap).insert(k, key, value);
    }
}

pub struct Arena<T> {
    entries: RefCell<Vec<Option<T>>>,
    free: RefCell<Vec<usize>>,
    marked: RefCell<Vec<bool>>,
}
impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self {
            entries: RefCell::new(Vec::new()),
            free: RefCell::new(Vec::new()),
            marked: RefCell::new(Vec::new()),
        }
    }
}
pub trait Stored: Sized {
    fn arena(heap: &Heap) -> &Arena<Self>;
    fn trace(&self, work: &mut Vec<Root>) -> u64;
}

#[derive(Default)]
pub struct Heap {
    pub produced: Counter<u64>,
    functions: Arena<Function>,
    exceptions: Arena<Exc>,
    receipts: Arena<Receipt>,
    arrivals: Arena<Arrival>,
    derived: Arena<Derived>,
    tools: Arena<Tool>,
    refs: Arena<RefV>,
    methods: Arena<(Value, Rc<str>)>,
    lists: Arena<Vec<Value>>,
    dicts: Arena<Dict>,
    cells: Arena<Option<Value>>,
    iters: Arena<Iter>,
}
macro_rules! stored {
    ($t:ty, $field:ident) => {
        impl Stored for $t {
            fn arena(heap: &Heap) -> &Arena<Self> {
                &heap.$field
            }
            fn trace(&self, work: &mut Vec<Root>) -> u64 {
                Trace::trace(self, work)
            }
        }
    };
}
stored!(Vec<Value>, lists);
stored!(Dict, dicts);
stored!(Option<Value>, cells);
stored!(Iter, iters);
impl Heap {
    pub fn alloc<T: Stored>(&self, value: T) -> Slot<T> {
        self.produce(1);
        let arena = T::arena(self);
        let mut entries = arena.entries.borrow_mut();
        let index = if let Some(i) = arena.free.borrow_mut().pop() {
            entries[i] = Some(value);
            i
        } else {
            let i = entries.len();
            entries.push(Some(value));
            arena.marked.borrow_mut().push(false);
            i
        };
        Slot {
            index,
            marker: PhantomData,
        }
    }
}
stored!(Function, functions);
stored!(Exc, exceptions);
stored!(Receipt, receipts);
stored!(Arrival, arrivals);
stored!(Derived, derived);
stored!(Tool, tools);
stored!(RefV, refs);
stored!((Value, Rc<str>), methods);

/// Rust 的临时值只在一个指令内存活；只在指令之间、所有临时值已回到这些根时回收。
pub enum Root {
    Value(Value),
    Cell(Cell),
}
trait Trace {
    fn trace(&self, work: &mut Vec<Root>) -> u64;
}
fn values(work: &mut Vec<Root>, xs: impl IntoIterator<Item = Value>) {
    work.extend(xs.into_iter().map(Root::Value));
}
fn integer_size(n: &num_bigint::BigInt) -> u64 {
    n.bits().div_ceil(64).max(1)
}
impl Trace for Vec<Value> {
    fn trace(&self, work: &mut Vec<Root>) -> u64 {
        values(work, self.iter().cloned());
        self.len() as u64
    }
}
impl Trace for Dict {
    fn trace(&self, work: &mut Vec<Root>) -> u64 {
        for (k, v) in self.items() {
            values(work, [k.clone(), v.clone()]);
        }
        self.len() as u64
    }
}
impl Trace for Option<Value> {
    fn trace(&self, work: &mut Vec<Root>) -> u64 {
        values(work, self.iter().cloned());
        1
    }
}
impl Trace for Function {
    fn trace(&self, work: &mut Vec<Root>) -> u64 {
        values(work, self.defaults.iter().cloned());
        work.extend(self.cells.iter().copied().map(Root::Cell));
        1
    }
}
impl Trace for Exc {
    fn trace(&self, work: &mut Vec<Root>) -> u64 {
        values(work, self.args.iter().cloned());
        1 + json_size(&self.data) + self.kind.as_ref().map_or(0, |k| k.len() as u64)
    }
}
impl Trace for (Value, Rc<str>) {
    fn trace(&self, work: &mut Vec<Root>) -> u64 {
        values(work, [self.0.clone()]);
        1 + self.1.len() as u64
    }
}
impl Trace for Receipt {
    fn trace(&self, work: &mut Vec<Root>) -> u64 {
        values(
            work,
            [
                self.value.clone(),
                Value::Proxy(self.attach),
                Value::Proxy(self.derived),
            ],
        );
        1 + json_size(&self.dependencies)
    }
}
impl Trace for Arrival {
    fn trace(&self, work: &mut Vec<Root>) -> u64 {
        values(
            work,
            [
                self.port.clone(),
                self.value.clone(),
                Value::Proxy(self.attach),
            ],
        );
        1 + self.sender.len() as u64 + self.via.len() as u64 + json_size(&self.dependencies)
    }
}
impl Trace for Derived {
    fn trace(&self, work: &mut Vec<Root>) -> u64 {
        values(work, [self.revoker.clone()]);
        1 + self.handle.len() as u64
    }
}
impl Trace for Tool {
    fn trace(&self, work: &mut Vec<Root>) -> u64 {
        values(work, [Value::Function(self.f)]);
        1 + self.name.len() as u64
            + json_size(&self.spec)
            + json_size(&self.input)
            + self.output.as_ref().map_or(0, json_size)
    }
}
impl Trace for RefV {
    fn trace(&self, _: &mut Vec<Root>) -> u64 {
        1 + json_size(&self.target)
            + match &self.spec {
                Spec::Inline(s) => json_size(s),
            }
    }
}
impl Trace for Iter {
    fn trace(&self, work: &mut Vec<Root>) -> u64 {
        1 + match self {
            Iter::List(l, _) => {
                values(work, [Value::List(*l)]);
                0
            }
            Iter::Tuple(t, _) => {
                values(work, [Value::Tuple(t.clone())]);
                0
            }
            Iter::Str(s, _) => s.len() as u64,
            Iter::Range(r, n) => {
                values(work, [Value::Range(r.clone())]);
                integer_size(n)
            }
            Iter::Dict { dict, .. } => {
                values(work, [Value::Dict(*dict)]);
                0
            }
            Iter::Proxy(d, _) => {
                values(work, [Value::Proxy(*d)]);
                0
            }
            Iter::Enumerate(i, n) => {
                values(work, [Value::Iter(*i)]);
                integer_size(n)
            }
            Iter::Zip(xs, _) => {
                values(work, xs.iter().map(|i| Value::Iter(*i)));
                0
            }
            Iter::Done => 0,
        }
    }
}
pub(super) fn json_size(json: &serde_json::Value) -> u64 {
    let mut work = vec![json];
    let mut size: u64 = 0;
    while let Some(v) = work.pop() {
        size = size.saturating_add(match v {
            serde_json::Value::String(s) => s.len() as u64,
            serde_json::Value::Array(a) => {
                work.extend(a);
                a.len() as u64
            }
            serde_json::Value::Object(o) => {
                work.extend(o.values());
                o.len() as u64 + o.keys().map(|s| s.len() as u64).sum::<u64>()
            }
            _ => 1,
        });
    }
    size
}
impl<T> Arena<T> {
    fn reset(&self) {
        self.marked.borrow_mut().fill(false);
    }
    fn sweep(&self) {
        let mut entries = self.entries.borrow_mut();
        let marked = self.marked.borrow();
        let mut free = self.free.borrow_mut();
        for (i, entry) in entries.iter_mut().enumerate() {
            if !marked[i] && entry.take_if(|_| true).is_some() {
                free.push(i);
            }
        }
    }
    #[cfg(test)]
    fn occupied(&self) -> usize {
        self.entries.borrow().len() - self.free.borrow().len()
    }
}
impl<T: Stored> Slot<T> {
    fn mark(self, heap: &Heap, work: &mut Vec<Root>) -> u64 {
        let arena = T::arena(heap);
        if std::mem::replace(&mut arena.marked.borrow_mut()[self.index], true) {
            return 0;
        }
        self.borrow(heap).trace(work)
    }
}
impl Heap {
    pub fn produce(&self, n: u64) {
        self.produced.set(self.produced.get().saturating_add(n));
    }
    pub fn collect(&self, mut work: Vec<Root>) -> u64 {
        self.lists.reset();
        self.dicts.reset();
        self.cells.reset();
        self.iters.reset();
        self.functions.reset();
        self.exceptions.reset();
        self.receipts.reset();
        self.arrivals.reset();
        self.derived.reset();
        self.tools.reset();
        self.refs.reset();
        self.methods.reset();
        let mut total: u64 = 0;
        while let Some(root) = work.pop() {
            let size = match root {
                Root::Cell(c) => c.mark(self, &mut work),
                Root::Value(v) => match v {
                    Value::Int(n) => integer_size(&n),
                    Value::Str(s) | Value::Module(s) | Value::Inbox(s) => s.len() as u64,
                    Value::Tuple(t) => {
                        values(&mut work, t.iter().cloned());
                        t.len() as u64
                    }
                    Value::List(s) => s.mark(self, &mut work),
                    Value::Dict(s) | Value::View(_, s) => s.mark(self, &mut work),
                    Value::Proxy(d) => d.mark(self, &mut work),
                    Value::Function(s) => s.mark(self, &mut work),
                    Value::Exc(s) => s.mark(self, &mut work),
                    Value::Method(s) => s.mark(self, &mut work),
                    Value::Delivery(d) => {
                        work.push(Root::Value(d.value.clone()));
                        json_size(&d.dependencies)
                    }
                    Value::Receipt(s) => s.mark(self, &mut work),
                    Value::Arrival(s) => s.mark(self, &mut work),
                    Value::Derived(s) => s.mark(self, &mut work),
                    Value::Tool(s) => s.mark(self, &mut work),
                    Value::Ref(s) => s.mark(self, &mut work),
                    Value::Iter(s) => s.mark(self, &mut work),
                    Value::Range(r) => {
                        integer_size(&r.start) + integer_size(&r.stop) + integer_size(&r.step)
                    }
                    _ => 1,
                },
            };
            total = total.saturating_add(size);
        }
        self.lists.sweep();
        self.dicts.sweep();
        self.cells.sweep();
        self.iters.sweep();
        self.functions.sweep();
        self.exceptions.sweep();
        self.receipts.sweep();
        self.arrivals.sweep();
        self.derived.sweep();
        self.tools.sweep();
        self.refs.sweep();
        self.methods.sweep();
        self.produced.set(0);
        total
    }
    #[cfg(test)]
    pub fn occupied(&self) -> usize {
        self.lists.occupied()
            + self.dicts.occupied()
            + self.cells.occupied()
            + self.iters.occupied()
            + self.functions.occupied()
            + self.exceptions.occupied()
            + self.receipts.occupied()
            + self.arrivals.occupied()
            + self.derived.occupied()
            + self.tools.occupied()
            + self.refs.occupied()
            + self.methods.occupied()
    }
}
