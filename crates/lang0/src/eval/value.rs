//! 求值器的值：数据与运行时对象。值的语义照 Python 3.12，与它不同的地方列在 docs/Lang0/README.md
//! 的「拦什么」与 Subset.md 的「形状之外的规则」。
//! 参考：docs/Lang0/README.md（值与类型、Port 与交出、运行时对象）；docs/Lang0/Subset.md（形状之外的规则）。

use super::heap::{Heap, Slot};
use num_bigint::BigInt;
use num_traits::{FromPrimitive, ToPrimitive, Zero};
use serde_json::Value as Json;
use std::collections::HashMap;
use std::rc::Rc;

use super::compile::Code;

pub type Cell = Slot<Option<Value>>;

#[derive(Clone)]
pub enum Value {
    None,
    Bool(bool),
    Int(Rc<BigInt>),
    Float(f64),
    Str(Rc<str>),
    List(Slot<Vec<Value>>),
    Tuple(Rc<Tuple>),
    Dict(Slot<Dict>),
    Range(Rc<RangeV>),
    /// dict 的 keys、values、items。
    View(View, Slot<Dict>),
    /// 运行时对象的 `.attach`、`.derived`：只读的映射。
    Proxy(Slot<Dict>),
    /// for 用的迭代器，以及 enumerate、zip 的对象。
    Iter(Slot<Iter>),
    Function(Slot<Function>),
    Builtin(&'static str),
    /// 绑到值上的方法。
    Method(Slot<(Value, Rc<str>)>),
    /// 从类上取的方法，如 `dict.pop`。
    Unbound(&'static str, Rc<str>),
    Class(Class),
    Exc(Slot<Exc>),
    Module(Rc<str>),
    Ref(Slot<RefV>),
    Inbox(Rc<str>),
    Receipt(Slot<Receipt>),
    Delivery(Rc<Delivery>),
    Arrival(Slot<Arrival>),
    Derived(Slot<Derived>),
    Tool(Slot<Tool>),
    /// 类型的写法在运行时的值：TypedDict、Port、Schema、typing 的名字。只用来写标注与 Port。
    Form(Rc<Form>),
}

/// tuple 只拥有不可变的元素；最后一个引用释放时把子 tuple 的元素移进工作栈。
pub struct Tuple(Vec<Value>);
impl std::ops::Deref for Tuple {
    type Target = [Value];
    fn deref(&self) -> &[Value] {
        &self.0
    }
}
impl Drop for Tuple {
    fn drop(&mut self) {
        let mut work = std::mem::take(&mut self.0);
        while let Some(value) = work.pop() {
            if let Value::Tuple(tuple) = value {
                if let Ok(mut tuple) = Rc::try_unwrap(tuple) {
                    work.append(&mut tuple.0);
                }
            }
        }
    }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<Lang0 value>")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Keys,
    Values,
    Items,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    /// str、int、float、bool、list、dict、range、enumerate、zip。
    Type(&'static str),
    Exc(&'static str),
}

pub struct RangeV {
    pub start: BigInt,
    pub stop: BigInt,
    pub step: BigInt,
}

impl RangeV {
    pub fn len(&self) -> BigInt {
        let (lo, hi, step) = if self.step > BigInt::zero() {
            (&self.start, &self.stop, self.step.clone())
        } else {
            (&self.stop, &self.start, -self.step.clone())
        };
        if lo >= hi {
            return BigInt::zero();
        }
        (hi - lo - 1) / step + 1
    }
    pub fn at(&self, i: &BigInt) -> BigInt {
        &self.start + i * &self.step
    }
}

#[derive(Clone)]
pub struct Function {
    pub code: Rc<Code>,
    pub defaults: Vec<Value>,
    pub cells: Vec<Cell>,
}

/// 一个引用值：目标是 Required port 的名字或 Handle，类型是自己这一侧写明的 Port spec，
/// 自己这一侧写的是 any 时用 Kernel 给出的 Port spec 的 ID。
#[derive(Clone)]
pub struct RefV {
    pub target: Json,
    pub spec: Spec,
}

#[derive(Clone)]
pub enum Spec {
    Inline(Json),
}

#[derive(Clone)]
pub struct Receipt {
    pub value: Value,
    pub dependencies: Json,
    pub attach: Slot<Dict>,
    pub derived: Slot<Dict>,
}

#[derive(Clone)]
pub struct Arrival {
    pub port: Value,
    pub sender: Rc<str>,
    pub via: Rc<str>,
    pub value: Value,
    pub dependencies: Json,
    pub attach: Slot<Dict>,
}

#[derive(Clone)]
pub struct Delivery {
    pub value: Value,
    pub dependencies: Json,
}

#[derive(Clone)]
pub struct Derived {
    pub handle: Rc<str>,
    pub revoker: Value,
}

#[derive(Clone)]
pub struct Tool {
    pub name: Rc<str>,
    pub spec: Json,
    pub input: Json,
    pub output: Option<Json>,
    pub f: Slot<Function>,
}

pub enum Form {
    Port,
    Schema,
    Typing(&'static str),
}

#[derive(Clone)]
pub struct Exc {
    pub class: &'static str,
    pub args: Vec<Value>,
    /// croon.Failure 的 kind 与 data。
    pub kind: Option<String>,
    pub data: Json,
}

/// 异常类的父类；类名照 Python。
pub fn exc_parent(class: &str) -> Option<&'static str> {
    Some(match class {
        "BaseException" => return None,
        "Exception" | "Limit" => "BaseException",
        "ArithmeticError" | "LookupError" | "ValueError" | "TypeError" | "RuntimeError"
        | "OSError" | "AttributeError" | "NameError" | "Failure" => "Exception",
        "ZeroDivisionError" | "OverflowError" => "ArithmeticError",
        "KeyError" | "IndexError" => "LookupError",
        "UnboundLocalError" => "NameError",
        _ => return None,
    })
}

/// 清单里的异常类与 croon 的两个。
pub fn exc_class(name: &str) -> Option<&'static str> {
    const ALL: &[&str] = &[
        "BaseException",
        "Exception",
        "ArithmeticError",
        "LookupError",
        "KeyError",
        "IndexError",
        "ValueError",
        "TypeError",
        "RuntimeError",
        "OSError",
        "ZeroDivisionError",
        "OverflowError",
        "AttributeError",
        "NameError",
        "UnboundLocalError",
        "Failure",
        "Limit",
    ];
    ALL.iter().copied().find(|c| *c == name)
}

pub fn is_subclass(class: &str, of: &str) -> bool {
    let mut c = Some(class);
    while let Some(x) = c {
        if x == of {
            return true;
        }
        c = exc_parent(x);
    }
    false
}

/// 内部的迭代器。
pub enum Iter {
    List(Slot<Vec<Value>>, usize),
    Tuple(Rc<Tuple>, usize),
    Str(Rc<str>, usize),
    Range(Rc<RangeV>, BigInt),
    Dict {
        dict: Slot<Dict>,
        view: View,
        pos: usize,
        len: usize,
        version: u64,
    },
    Proxy(Slot<Dict>, usize),
    Enumerate(Slot<Iter>, BigInt),
    Zip(Vec<Slot<Iter>>, bool),
    Done,
}

impl Iter {
    pub fn type_name(&self) -> &'static str {
        match self {
            Iter::Enumerate(..) => "enumerate",
            Iter::Zip(..) => "zip",
            _ => "iterator",
        }
    }
}

// ---- dict ----

/// dict 的键：Python 里相等、hash 相同的值是同一个键，如 1、1.0 与 True。
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Key {
    None,
    Int(BigInt),
    Float(u64),
    Str(Rc<str>),
    Tuple(Vec<Key>),
}

#[derive(Default, Clone)]
pub struct Dict {
    entries: Vec<Option<(Value, Value)>>,
    index: HashMap<Key, usize>,
    len: usize,
    /// 键的集合变了就加一：迭代中查改动用。
    pub version: u64,
}

impl Dict {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn get(&self, k: &Key) -> Option<&Value> {
        self.index
            .get(k)
            .map(|&i| &self.entries[i].as_ref().unwrap().1)
    }
    pub fn insert(&mut self, k: Key, key: Value, value: Value) {
        match self.index.get(&k) {
            Some(&i) => self.entries[i].as_mut().unwrap().1 = value,
            None => {
                self.index.insert(k, self.entries.len());
                self.entries.push(Some((key, value)));
                self.len += 1;
                self.version += 1;
            }
        }
    }
    pub fn remove(&mut self, k: &Key) -> Option<Value> {
        let i = self.index.remove(k)?;
        self.len -= 1;
        self.version += 1;
        self.entries[i].take().map(|(_, v)| v)
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
        self.len = 0;
        self.version += 1;
    }
    pub fn items(&self) -> impl Iterator<Item = &(Value, Value)> {
        self.entries.iter().flatten()
    }
    /// 第 `pos` 个槽位起的下一项：(它的位置, 键, 值)。
    pub fn next_from(&self, pos: usize) -> Option<(usize, Value, Value)> {
        (pos..self.entries.len()).find_map(|i| {
            self.entries[i]
                .as_ref()
                .map(|(k, v)| (i, k.clone(), v.clone()))
        })
    }
    /// 字符串键的映射；运行时对象用它。
    pub fn of_strs(items: impl IntoIterator<Item = (String, Value)>) -> Dict {
        let mut d = Dict::default();
        for (k, v) in items {
            let s: Rc<str> = k.into();
            d.insert(Key::Str(s.clone()), Value::Str(s), v);
        }
        d
    }
}

// ---- 值的通用操作 ----

impl Value {
    pub fn int(heap: &Heap, n: impl Into<BigInt>) -> Value {
        {
            let n = n.into();
            heap.produce(n.bits().div_ceil(64).max(1));
            Value::Int(Rc::new(n))
        }
    }
    pub fn str(heap: &Heap, s: impl Into<Rc<str>>) -> Value {
        {
            let s = s.into();
            heap.produce(s.len() as u64);
            Value::Str(s)
        }
    }
    pub fn list(heap: &Heap, v: Vec<Value>) -> Value {
        heap.produce(v.len() as u64);
        Value::List(heap.alloc(v))
    }
    pub fn tuple(heap: &Heap, v: Vec<Value>) -> Value {
        {
            heap.produce(v.len() as u64);
            Value::Tuple(Rc::new(Tuple(v)))
        }
    }
    pub fn dict(heap: &Heap, d: Dict) -> Value {
        heap.produce(d.len() as u64);
        Value::Dict(heap.alloc(d))
    }

    /// 类型名，见 Subset.md 的 type_names。
    pub fn type_name(&self, heap: &Heap) -> String {
        match self {
            Value::None => "NoneType",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "str",
            Value::List(_) => "list",
            Value::Tuple(_) => "tuple",
            Value::Dict(_) => "dict",
            Value::Range(_) => "range",
            Value::View(View::Keys, _) => "dict_keys",
            Value::View(View::Values, _) => "dict_values",
            Value::View(View::Items, _) => "dict_items",
            Value::Proxy(_) => "mappingproxy",
            Value::Iter(i) => i.borrow(heap).type_name(),
            Value::Function(_) => "function",
            Value::Builtin(_) | Value::Method(_) => "builtin_function_or_method",
            Value::Unbound(..) => "method_descriptor",
            Value::Class(_) => "type",
            Value::Exc(e) => return e.borrow(heap).class.into(),
            Value::Module(_) => "module",
            Value::Ref(_) | Value::Inbox(_) => "Ref",
            Value::Receipt(_) => "Receipt",
            Value::Delivery(_) => "Delivery",
            Value::Arrival(_) => "Arrival",
            Value::Derived(_) => "Derived",
            Value::Tool(_) => "Tool",
            Value::Form(f) => match **f {
                Form::Port => "Port",
                _ => "type",
            },
        }
        .into()
    }

    pub fn truthy(&self, heap: &Heap) -> bool {
        match self {
            Value::None => false,
            Value::Bool(b) => *b,
            Value::Int(n) => !n.is_zero(),
            Value::Float(x) => *x != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::List(l) => !l.borrow(heap).is_empty(),
            Value::Tuple(t) => !t.is_empty(),
            Value::Dict(d) => d.borrow(heap).len() > 0,
            Value::Range(r) => !r.len().is_zero(),
            Value::View(_, d) => d.borrow(heap).len() > 0,
            Value::Proxy(d) => d.borrow(heap).len() > 0,
            _ => true,
        }
    }

    /// int 与 bool 的整数值。
    pub fn as_int(&self) -> Option<BigInt> {
        match self {
            Value::Bool(b) => Some(BigInt::from(*b as u8)),
            Value::Int(n) => Some((**n).clone()),
            _ => None,
        }
    }
}

/// 数的比较：int 与 float 之间按精确的值比，照 Python。
pub enum Num {
    Int(BigInt),
    Float(f64),
}

pub fn num(v: &Value) -> Option<Num> {
    match v {
        Value::Bool(b) => Some(Num::Int(BigInt::from(*b as u8))),
        Value::Int(n) => Some(Num::Int((**n).clone())),
        Value::Float(x) => Some(Num::Float(*x)),
        _ => None,
    }
}

/// 精确地比较两个数；有 NaN 时为 None。
pub fn num_cmp(a: &Num, b: &Num) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    match (a, b) {
        (Num::Int(x), Num::Int(y)) => Some(x.cmp(y)),
        (Num::Float(x), Num::Float(y)) => x.partial_cmp(y),
        (Num::Int(x), Num::Float(y)) => int_float_cmp(x, *y),
        (Num::Float(x), Num::Int(y)) => int_float_cmp(y, *x).map(Ordering::reverse),
    }
}

fn int_float_cmp(x: &BigInt, y: f64) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    if y.is_nan() {
        return None;
    }
    if y.is_infinite() {
        return Some(if y > 0.0 {
            Ordering::Less
        } else {
            Ordering::Greater
        });
    }
    let floor = y.floor();
    let f = BigInt::from_f64(floor).expect("有限");
    Some(match x.cmp(&f) {
        Ordering::Equal if y > floor => Ordering::Less,
        o => o,
    })
}

/// float 能表示为 int 时的 int。
pub fn float_int(x: f64) -> Option<BigInt> {
    (x.is_finite() && x.fract() == 0.0).then(|| BigInt::from_f64(x).unwrap())
}

pub fn to_f64(n: &BigInt) -> Option<f64> {
    n.to_f64().filter(|x| x.is_finite())
}

impl Exc {
    pub fn kind(&self) -> &str {
        match &self.kind {
            Some(kind) => kind,
            None if self.class == "Limit" => "limit",
            None => self.class,
        }
    }
}
