//! 值上的运算：相等、比较、hash、下标、in、迭代、算术。语义照 Python 3.12，报错用 lang0.messages。
//! 每个运算按读入与产出的数据量计步，见 docs/Evaluator.md 的「资源」。
//! 参考：docs/Subset.md（形状之外的规则、异常的消息）；docs/Evaluator.md（资源）。
use super::heap::{Heap, Slot};

use std::cmp::Ordering;
use std::rc::Rc;

use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::compile::{Bin, Cmp, Un};
use super::msg;
use super::text::R;
use super::value::*;

/// 计步与深度。
pub struct Meter {
    pub steps: u64,
    pub max_steps: u64,
    /// 深遍历还能往下走的层数。
    pub depth: usize,
}

impl Meter {
    pub fn charge(&mut self, heap: &Heap, n: u64) -> R<()> {
        self.steps = self.steps.saturating_add(n.max(1));
        if self.steps > self.max_steps {
            Err(msg::limit(heap, "steps"))
        } else {
            Ok(())
        }
    }
    /// 产出之前先看够不够：先算好大小、计入步数再分配。
    pub fn reserve(&mut self, heap: &Heap, n: &BigInt) -> R<()> {
        match n.to_u64() {
            Some(n) => self.charge(heap, n),
            None => Err(msg::limit(heap, "steps")),
        }
    }
}

fn tn(heap: &Heap, v: &Value) -> String {
    v.type_name(heap)
}

// ---- hash 与相等 ----

/// hash 是深遍历：nested tuple 每进一层用掉一层 `depth`。
pub fn key(heap: &Heap, v: &Value, depth: usize) -> R<Key> {
    if depth == 0 {
        return Err(msg::limit(heap, "depth"));
    }
    Ok(match v {
        Value::None => Key::None,
        Value::Bool(b) => Key::Int(BigInt::from(*b as u8)),
        Value::Int(n) => Key::Int((**n).clone()),
        Value::Float(x) => match float_int(*x) {
            Some(n) => Key::Int(n),
            None => Key::Float(x.to_bits()),
        },
        Value::Str(s) => Key::Str(s.clone()),
        Value::Tuple(t) => Key::Tuple(
            t.iter()
                .map(|v| key(heap, v, depth - 1))
                .collect::<R<_>>()?,
        ),
        other => {
            return Err(msg::err(
                heap,
                "unhashable",
                None,
                &[("type", tn(heap, other))],
            ));
        }
    })
}

pub fn eq(heap: &Heap, a: &Value, b: &Value, depth: usize) -> R<bool> {
    if depth == 0 {
        return Err(msg::limit(heap, "depth"));
    }
    let d = depth - 1;
    if let (Some(x), Some(y)) = (num(a), num(b)) {
        return Ok(num_cmp(&x, &y) == Some(Ordering::Equal));
    }
    Ok(match (a, b) {
        (Value::None, Value::None) => true,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::List(x), Value::List(y)) => {
            x == y || seq_eq(heap, &x.borrow(heap), &y.borrow(heap), d)?
        }
        (Value::Tuple(x), Value::Tuple(y)) => Rc::ptr_eq(x, y) || seq_eq(heap, x, y, d)?,
        (Value::Dict(x), Value::Dict(y)) => {
            x == y || dict_eq(heap, &x.borrow(heap), &y.borrow(heap), d)?
        }
        (Value::Proxy(x), Value::Proxy(y)) => {
            x == y || dict_eq(heap, &x.borrow(heap), &y.borrow(heap), d)?
        }
        (Value::Range(x), Value::Range(y)) => {
            let (lx, ly) = (x.len(), y.len());
            lx == ly && (lx.is_zero() || (x.start == y.start && (lx.is_one() || x.step == y.step)))
        }
        (Value::View(vx, x), Value::View(vy, y)) => vx == vy && x == y,
        (Value::Function(x), Value::Function(y)) => x == y,
        (Value::Builtin(x), Value::Builtin(y)) => x == y,
        (Value::Class(x), Value::Class(y)) => x == y,
        (Value::Module(x), Value::Module(y)) => x == y,
        (Value::Inbox(x), Value::Inbox(y)) => x == y,
        (Value::Ref(x), Value::Ref(y)) => x == y,
        (Value::Exc(x), Value::Exc(y)) => x == y,
        (Value::Tool(x), Value::Tool(y)) => x == y,
        (Value::Iter(x), Value::Iter(y)) => x == y,
        _ => false,
    })
}

fn seq_eq(heap: &Heap, x: &[Value], y: &[Value], d: usize) -> R<bool> {
    if x.len() != y.len() {
        return Ok(false);
    }
    for (a, b) in x.iter().zip(y) {
        if !same(heap, a, b) && !eq(heap, a, b, d)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn dict_eq(heap: &Heap, x: &Dict, y: &Dict, d: usize) -> R<bool> {
    if x.len() != y.len() {
        return Ok(false);
    }
    for (k, v) in x.items() {
        match y.get(&key(heap, k, d)?) {
            Some(w) if same(heap, v, w) || eq(heap, v, w, d)? => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

/// 同一个对象：容器按身份；Python 的 `in` 与容器的相等先看身份。
pub fn same(_heap: &Heap, a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::List(x), Value::List(y)) => x == y,
        (Value::Dict(x), Value::Dict(y)) => x == y,
        (Value::Tuple(x), Value::Tuple(y)) => Rc::ptr_eq(x, y),
        (Value::Str(x), Value::Str(y)) => Rc::ptr_eq(x, y),
        (Value::Int(x), Value::Int(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// `<`、`<=`、`>`、`>=`。
pub fn order(heap: &Heap, a: &Value, b: &Value, op: Cmp, depth: usize) -> R<bool> {
    if depth == 0 {
        return Err(msg::limit(heap, "depth"));
    }
    let want = |o: Ordering| holds(op, o);
    if let (Some(x), Some(y)) = (num(a), num(b)) {
        return Ok(num_cmp(&x, &y).is_some_and(want));
    }
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => Ok(want(x.cmp(y))),
        (Value::List(x), Value::List(y)) => seq_order(
            heap,
            &x.borrow(heap).clone(),
            &y.borrow(heap).clone(),
            op,
            depth - 1,
        ),
        (Value::Tuple(x), Value::Tuple(y)) => seq_order(heap, x, y, op, depth - 1),
        _ => Err(unorderable(heap, a, b, op)),
    }
}

fn seq_order(heap: &Heap, x: &[Value], y: &[Value], op: Cmp, d: usize) -> R<bool> {
    for (a, b) in x.iter().zip(y) {
        if !(same(heap, a, b) || eq(heap, a, b, d)?) {
            return order(heap, a, b, op, d);
        }
    }
    Ok(holds(op, x.len().cmp(&y.len())))
}

/// 一个大小关系满不满足 `op`；只由 order 与 seq_order 调用，它们只收这四个运算。
fn holds(op: Cmp, o: Ordering) -> bool {
    match op {
        Cmp::Lt => o == Ordering::Less,
        Cmp::LtE => o != Ordering::Greater,
        Cmp::Gt => o == Ordering::Greater,
        Cmp::GtE => o != Ordering::Less,
        _ => unreachable!("order 只收 <、<=、>、>="),
    }
}

fn cmp_symbol(op: Cmp) -> &'static str {
    match op {
        Cmp::Lt => "<",
        Cmp::LtE => "<=",
        Cmp::Gt => ">",
        Cmp::GtE => ">=",
        _ => unreachable!("order 只收 <、<=、>、>="),
    }
}

fn unorderable(heap: &Heap, a: &Value, b: &Value, op: Cmp) -> Value {
    msg::err(
        heap,
        "unorderable",
        None,
        &[
            ("op", cmp_symbol(op).into()),
            ("left", tn(heap, a)),
            ("right", tn(heap, b)),
        ],
    )
}

fn singleton(v: &Value) -> bool {
    matches!(v, Value::None | Value::Bool(_))
}

pub fn compare(heap: &Heap, a: &Value, b: &Value, op: Cmp, m: &mut Meter) -> R<Value> {
    m.charge(heap, 1)?;
    let d = m.depth;
    Ok(Value::Bool(match op {
        Cmp::Eq => eq(heap, a, b, d)?,
        Cmp::NotEq => !eq(heap, a, b, d)?,
        Cmp::Lt | Cmp::LtE | Cmp::Gt | Cmp::GtE => order(heap, a, b, op, d)?,
        Cmp::In => contains(heap, b, a, m)?,
        Cmp::NotIn => !contains(heap, b, a, m)?,
        Cmp::Is | Cmp::IsNot => {
            let r = if singleton(a) || singleton(b) {
                match (a, b) {
                    (Value::None, Value::None) => true,
                    (Value::Bool(x), Value::Bool(y)) => x == y,
                    _ => false,
                }
            } else if matches!(
                (a, b),
                (
                    Value::List(_) | Value::Dict(_),
                    Value::List(_) | Value::Dict(_)
                )
            ) {
                same(heap, a, b)
            } else {
                return Err(msg::err(
                    heap,
                    "identity_of_value",
                    None,
                    &[("left", tn(heap, a)), ("right", tn(heap, b))],
                ));
            };
            if op == Cmp::Is { r } else { !r }
        }
    }))
}

pub fn contains(heap: &Heap, container: &Value, item: &Value, m: &mut Meter) -> R<bool> {
    let d = m.depth;
    let scan = |items: &[Value], m: &mut Meter| -> R<bool> {
        m.charge(heap, items.len() as u64)?;
        for x in items {
            if same(heap, x, item) || eq(heap, x, item, d)? {
                return Ok(true);
            }
        }
        Ok(false)
    };
    match container {
        Value::Str(s) => match item {
            Value::Str(sub) => {
                m.charge(heap, (s.len() + sub.len()) as u64)?;
                Ok(s.contains(&**sub))
            }
            other => Err(msg::err(
                heap,
                "in_string",
                None,
                &[("type", tn(heap, other))],
            )),
        },
        Value::List(l) => {
            let items = l.borrow(heap).clone();
            scan(&items, m)
        }
        Value::Tuple(t) => scan(t, m),
        Value::Dict(dict) | Value::View(View::Keys, dict) => {
            Ok(dict.borrow(heap).get(&key(heap, item, d)?).is_some())
        }
        Value::Proxy(dict) => Ok(dict.borrow(heap).get(&key(heap, item, d)?).is_some()),
        Value::View(View::Values, dict) => {
            let items: Vec<Value> = dict.borrow(heap).items().map(|(_, v)| v.clone()).collect();
            scan(&items, m)
        }
        Value::View(View::Items, dict) => {
            let Value::Tuple(t) = item else {
                return Ok(false);
            };
            if t.len() != 2 {
                return Ok(false);
            }
            let dict = dict.borrow(heap);
            match dict.get(&key(heap, &t[0], d)?) {
                Some(v) => Ok(same(heap, v, &t[1]) || eq(heap, v, &t[1], d)?),
                None => Ok(false),
            }
        }
        Value::Range(r) => {
            let Some(n) = num(item) else { return Ok(false) };
            let n = match n {
                Num::Int(n) => n,
                Num::Float(x) => match float_int(x) {
                    Some(n) => n,
                    None => return Ok(false),
                },
            };
            let len = r.len();
            if len.is_zero() {
                return Ok(false);
            }
            let off = &n - &r.start;
            let (q, rem) = off.div_mod_floor(&r.step);
            Ok(rem.is_zero() && !q.is_negative() && q < len)
        }
        Value::Iter(_) => {
            let mut items = vec![];
            collect(heap, container, &mut items, m)?;
            scan(&items, m)
        }
        other => Err(msg::err(
            heap,
            "in_not_iterable",
            None,
            &[("type", tn(heap, other))],
        )),
    }
}

// ---- 下标 ----

/// 64 位下标：超出报 index_size。
pub fn index_of(heap: &Heap, i: &Value, seq: &str) -> R<i64> {
    let Some(n) = i.as_int() else {
        return Err(msg::err(
            heap,
            "bad_index",
            Some(seq),
            &[("type", tn(heap, i))],
        ));
    };
    n.to_i64()
        .ok_or_else(|| msg::err(heap, "index_size", None, &[]))
}

fn norm(i: i64, len: usize) -> Option<usize> {
    let len = len as i64;
    let j = if i < 0 { i + len } else { i };
    (0..len).contains(&j).then_some(j as usize)
}

pub fn getitem(heap: &Heap, obj: &Value, i: &Value, m: &mut Meter) -> R<Value> {
    m.charge(heap, 1)?;
    match obj {
        Value::List(l) => {
            let n = index_of(heap, i, "list")?;
            let l = l.borrow(heap);
            norm(n, l.len())
                .map(|j| l[j].clone())
                .ok_or_else(|| msg::err(heap, "index_out_of_range", Some("list"), &[]))
        }
        Value::Tuple(t) => {
            let n = index_of(heap, i, "tuple")?;
            norm(n, t.len())
                .map(|j| t[j].clone())
                .ok_or_else(|| msg::err(heap, "index_out_of_range", Some("tuple"), &[]))
        }
        Value::Str(s) => {
            let n = index_of(heap, i, "str")?;
            let count = s.chars().count();
            m.charge(heap, count as u64)?;
            norm(n, count)
                .map(|j| Value::str(heap, s.chars().nth(j).unwrap().to_string()))
                .ok_or_else(|| msg::err(heap, "index_out_of_range", Some("str"), &[]))
        }
        Value::Range(r) => {
            let Some(n) = i.as_int() else {
                return Err(msg::err(
                    heap,
                    "bad_index",
                    Some("range"),
                    &[("type", tn(heap, i))],
                ));
            };
            let len = r.len();
            let j = if n.is_negative() { &n + &len } else { n };
            if j.is_negative() || j >= len {
                return Err(msg::err(heap, "index_out_of_range", Some("range"), &[]));
            }
            Ok(Value::int(heap, r.at(&j)))
        }
        Value::Dict(d) => d
            .borrow(heap)
            .get(&key(heap, i, m.depth)?)
            .cloned()
            .ok_or_else(|| msg::key_error(heap, i.clone())),
        Value::Proxy(d) => d
            .borrow(heap)
            .get(&key(heap, i, m.depth)?)
            .cloned()
            .ok_or_else(|| msg::key_error(heap, i.clone())),
        Value::Class(Class::Type("list" | "dict")) | Value::Form(_) => {
            Ok(Value::Form(Rc::new(Form::Typing("GenericAlias"))))
        }
        other => Err(msg::err(
            heap,
            "not_subscriptable",
            None,
            &[("type", tn(heap, other))],
        )),
    }
}

pub fn setitem(heap: &Heap, obj: &Value, i: &Value, v: Value, m: &mut Meter) -> R<()> {
    m.charge(heap, 1)?;
    match obj {
        Value::List(l) => {
            let n = index_of(heap, i, "list")?;
            let mut l = l.borrow_mut(heap);
            let len = l.len();
            let j = norm(n, len).ok_or_else(|| msg::err(heap, "assign_out_of_range", None, &[]))?;
            l[j] = v;
            Ok(())
        }
        Value::Dict(d) => {
            let k = key(heap, i, m.depth)?;
            d.insert(heap, k, i.clone(), v);
            Ok(())
        }
        other => Err(msg::err(
            heap,
            "no_item_assignment",
            None,
            &[("type", tn(heap, other))],
        )),
    }
}

// ---- 迭代 ----

pub fn iter(heap: &Heap, v: &Value) -> R<Slot<Iter>> {
    let it = match v {
        Value::List(l) => Iter::List(*l, 0),
        Value::Tuple(t) => Iter::Tuple(t.clone(), 0),
        Value::Str(s) => Iter::Str(s.clone(), 0),
        Value::Range(r) => Iter::Range(r.clone(), BigInt::zero()),
        Value::Dict(d) => dict_iter(heap, d, View::Keys),
        Value::View(view, d) => dict_iter(heap, d, *view),
        Value::Proxy(d) => Iter::Proxy(*d, 0),
        Value::Iter(it) => return Ok(*it),
        other => {
            return Err(msg::err(
                heap,
                "not_iterable",
                None,
                &[("type", tn(heap, other))],
            ));
        }
    };
    Ok(heap.alloc(it))
}

fn dict_iter(heap: &Heap, d: &Slot<Dict>, view: View) -> Iter {
    let b = d.borrow(heap);
    Iter::Dict {
        dict: *d,
        view,
        pos: 0,
        len: b.len(),
        version: b.version,
    }
}

/// 不组合别的迭代器的那几种的下一项；None 是迭代完了。只由 next_any 调用。
fn next_leaf(heap: &Heap, it: &Slot<Iter>) -> R<Option<Value>> {
    let mut it = it.borrow_mut(heap);
    let out = match &mut *it {
        Iter::List(l, i) => {
            let x = l.borrow(heap).get(*i).cloned();
            *i += 1;
            x
        }
        Iter::Tuple(t, i) => {
            let x = t.get(*i).cloned();
            *i += 1;
            x
        }
        Iter::Str(s, i) => {
            let x = s[*i..].chars().next();
            if let Some(c) = x {
                *i += c.len_utf8();
            }
            x.map(|c| Value::str(heap, c.to_string()))
        }
        Iter::Range(r, i) => {
            if *i < r.len() {
                let x = r.at(i);
                *i += 1;
                Some(Value::int(heap, x))
            } else {
                None
            }
        }
        Iter::Dict {
            dict,
            view,
            pos,
            len,
            version,
        } => {
            let d = dict.borrow(heap);
            if d.len() != *len {
                *len = usize::MAX;
                return Err(msg::err(heap, "dict_changed", Some("size"), &[]));
            }
            if d.version != *version {
                return Err(msg::err(heap, "dict_changed", Some("keys"), &[]));
            }
            match d.next_from(*pos) {
                Some((at, k, v)) => {
                    *pos = at + 1;
                    Some(match view {
                        View::Keys => k,
                        View::Values => v,
                        View::Items => Value::tuple(heap, vec![k, v]),
                    })
                }
                None => None,
            }
        }
        Iter::Proxy(d, pos) => match d.borrow(heap).next_from(*pos) {
            Some((at, k, _)) => {
                *pos = at + 1;
                Some(k)
            }
            None => None,
        },
        Iter::Enumerate(..) | Iter::Zip(..) => unreachable!("next_any 先分出了组合迭代器"),
        Iter::Done => None,
    };
    if out.is_none() {
        *it = Iter::Done;
    }
    Ok(out)
}

/// 取出全部元素。
pub fn collect(heap: &Heap, v: &Value, out: &mut Vec<Value>, m: &mut Meter) -> R<()> {
    match v {
        Value::List(l) => {
            m.charge(heap, l.borrow(heap).len() as u64)?;
            out.extend(l.borrow(heap).iter().cloned());
        }
        Value::Tuple(t) => {
            m.charge(heap, t.len() as u64)?;
            out.extend(t.iter().cloned());
        }
        _ => {
            let it = iter(heap, v)?;
            while let Some(x) = next_any(heap, &it)? {
                m.charge(heap, 1)?;
                out.push(x);
            }
        }
    }
    Ok(())
}

/// enumerate 与 zip 也走这里。
pub fn next_any(heap: &Heap, it: &Slot<Iter>) -> R<Option<Value>> {
    let kind = match &*it.borrow(heap) {
        Iter::Enumerate(inner, n) => Some((Some(*inner), n.clone(), vec![], false)),
        Iter::Zip(inners, strict) => Some((None, BigInt::zero(), inners.clone(), *strict)),
        _ => None,
    };
    match kind {
        None => next_leaf(heap, it),
        Some((Some(inner), n, _, _)) => match next_any(heap, &inner)? {
            Some(x) => {
                if let Iter::Enumerate(_, k) = &mut *it.borrow_mut(heap) {
                    *k += 1;
                }
                Ok(Some(Value::tuple(heap, vec![Value::int(heap, n), x])))
            }
            None => Ok(None),
        },
        Some((None, _, inners, strict)) => {
            if inners.is_empty() {
                return Ok(None);
            }
            let mut items = vec![];
            for (i, inner) in inners.iter().enumerate() {
                match next_any(heap, inner)? {
                    Some(x) => items.push(x),
                    None => {
                        if strict {
                            if i > 0 {
                                return Err(zip_strict(heap, i + 1, "shorter"));
                            }
                            for (j, rest) in inners.iter().enumerate().skip(1) {
                                if next_any(heap, rest)?.is_some() {
                                    return Err(zip_strict(heap, j + 1, "longer"));
                                }
                            }
                        }
                        *it.borrow_mut(heap) = Iter::Zip(vec![], false);
                        return Ok(None);
                    }
                }
            }
            Ok(Some(Value::tuple(heap, items)))
        }
    }
}

fn zip_strict(heap: &Heap, i: usize, longer: &str) -> Value {
    let (s, range) = if i == 2 {
        ("", "1".to_string())
    } else {
        ("s", format!("1-{}", i - 1))
    };
    msg::err(
        heap,
        "zip_strict",
        None,
        &[
            ("i", i.to_string()),
            ("longer", longer.into()),
            ("s", s.into()),
            ("range", range),
        ],
    )
}

// ---- 算术 ----

fn bin_symbol(op: Bin, inplace: bool) -> String {
    let s = match op {
        Bin::Add => "+",
        Bin::Sub => "-",
        Bin::Mult => "*",
        Bin::Div => "/",
        Bin::FloorDiv => "//",
        Bin::Mod => "%",
        Bin::BitOr => "|",
    };
    if inplace { format!("{s}=") } else { s.into() }
}

fn unsupported(heap: &Heap, a: &Value, b: &Value, op: Bin, inplace: bool) -> Value {
    msg::err(
        heap,
        "unsupported_operand",
        None,
        &[
            ("op", bin_symbol(op, inplace)),
            ("left", tn(heap, a)),
            ("right", tn(heap, b)),
        ],
    )
}

fn to_float(heap: &Heap, n: &BigInt) -> R<f64> {
    to_f64(n).ok_or_else(|| msg::err(heap, "int_to_float", None, &[]))
}

pub fn float_of(heap: &Heap, v: &Num) -> R<f64> {
    match v {
        Num::Int(n) => to_float(heap, n),
        Num::Float(x) => Ok(*x),
    }
}

/// int / int，照 Python 正确舍入。
fn int_div(heap: &Heap, a: &BigInt, b: &BigInt) -> R<f64> {
    let limit = BigInt::one() << 53;
    if a.abs() <= limit && b.abs() <= limit {
        // 绝对值不超过 2**53 的 int 都能精确地转成 f64。
        return Ok(a.to_f64().expect("不超过 2**53") / b.to_f64().expect("不超过 2**53"));
    }
    let neg = (a.sign() == Sign::Minus) != (b.sign() == Sign::Minus);
    let (a, b) = (a.abs(), b.abs());
    // 取 55 位以上的商，余数并成粘位，再交给 to_f64 按就近偶数舍入。
    let shift = (b.bits() as i64 - a.bits() as i64 + 55).max(0);
    let (q, r) = (&a << shift).div_rem(&b);
    let q = if r.is_zero() {
        q
    } else {
        (q << 1) | BigInt::one()
    };
    let extra = if r.is_zero() { 0 } else { 1 };
    // num-bigint 的 to_f64 不会失败：过大时得 inf，由下面的 is_infinite 报 int_div_too_large。
    let x = q.to_f64().expect("BigInt::to_f64 总有值");
    let mut e = -(shift + extra);
    let mut y = x;
    while e < 0 {
        let step = e.max(-1000);
        y *= 2f64.powi(step as i32);
        e -= step;
    }
    if y.is_infinite() {
        return Err(msg::err(heap, "int_div_too_large", None, &[]));
    }
    Ok(if neg { -y } else { y })
}

fn float_floordiv(a: f64, b: f64) -> (f64, f64) {
    let m = a % b;
    let mut div = (a - m) / b;
    let mut m = m;
    if m != 0.0 {
        if (b < 0.0) != (m < 0.0) {
            m += b;
            div -= 1.0;
        }
    } else {
        m = 0f64.copysign(b);
    }
    let floordiv = if div != 0.0 {
        let f = div.floor();
        if div - f > 0.5 { f + 1.0 } else { f }
    } else {
        0f64.copysign(a / b)
    };
    (floordiv, m)
}

/// 重复的次数：`unit` 是一份的大小，产出先算好大小、计入步数。
fn repeat_count(heap: &Heap, n: &Value, unit: usize, m: &mut Meter) -> R<usize> {
    let count = n
        .as_int()
        .ok_or_else(|| msg::err(heap, "mul_non_int", None, &[("type", n.type_name(heap))]))?;
    let count = count
        .to_i64()
        .ok_or_else(|| msg::err_as(heap, "OverflowError", "index_size", None, &[]))?
        .max(0);
    m.reserve(heap, &(BigInt::from(unit) * count))?;
    // 空序列无需重复；非空产出已由步数上限约束。
    Ok(if unit == 0 {
        0
    } else {
        usize::try_from(count).expect("产出已计步")
    })
}

fn repeat_items(items: &[Value], count: usize) -> Vec<Value> {
    (0..count).flat_map(|_| items.iter().cloned()).collect()
}

/// 序列乘整数；调用方已分出 is_seq 的一边。
fn repeat(heap: &Heap, seq: &Value, n: &Value, m: &mut Meter) -> R<Value> {
    Ok(match seq {
        Value::Str(s) => Value::str(heap, s.repeat(repeat_count(heap, n, s.len(), m)?)),
        Value::List(l) => {
            let items = l.borrow(heap).clone();
            Value::list(
                heap,
                repeat_items(&items, repeat_count(heap, n, items.len(), m)?),
            )
        }
        Value::Tuple(t) => Value::tuple(heap, repeat_items(t, repeat_count(heap, n, t.len(), m)?)),
        _ => unreachable!("repeat 只收 is_seq 的值"),
    })
}

fn is_seq(v: &Value) -> bool {
    matches!(v, Value::Str(_) | Value::List(_) | Value::Tuple(_))
}

pub fn binary(
    heap: &Heap,
    a: &Value,
    b: &Value,
    op: Bin,
    inplace: bool,
    m: &mut Meter,
) -> R<Value> {
    m.charge(heap, 1)?;
    if inplace
        && op == Bin::Add
        && let Value::List(l) = a
    {
        let mut items = vec![];
        collect(heap, b, &mut items, m)?;
        l.extend(heap, items);
        return Ok(a.clone());
    }
    if inplace
        && op == Bin::Mult
        && let Value::List(list) = a
    {
        let items = list.borrow(heap).clone();
        let items = repeat_items(&items, repeat_count(heap, b, items.len(), m)?);
        list.replace(heap, items);
        return Ok(a.clone());
    }
    if op == Bin::Mod && matches!(a, Value::Str(_)) {
        return Err(msg::err(heap, "percent_format", None, &[]));
    }
    if op == Bin::BitOr {
        let form = |v: &Value| matches!(v, Value::Class(_) | Value::Form(_) | Value::None);
        if form(a) && form(b) {
            return Ok(Value::Form(Rc::new(Form::Typing("Union"))));
        }
        return Err(unsupported(heap, a, b, op, inplace));
    }
    if let (Some(x), Some(y)) = (num(a), num(b)) {
        return arith(heap, &x, &y, op, m);
    }
    match (op, a, b) {
        (Bin::Add, Value::Str(x), Value::Str(y)) => {
            m.charge(heap, (x.len() + y.len()) as u64)?;
            Ok(Value::str(heap, format!("{x}{y}")))
        }
        (Bin::Add, Value::List(x), Value::List(y)) => {
            m.charge(heap, (x.borrow(heap).len() + y.borrow(heap).len()) as u64)?;
            let mut v = x.borrow(heap).clone();
            v.extend(y.borrow(heap).iter().cloned());
            Ok(Value::list(heap, v))
        }
        (Bin::Add, Value::Tuple(x), Value::Tuple(y)) => {
            m.charge(heap, (x.len() + y.len()) as u64)?;
            let mut v = x.to_vec();
            v.extend(y.iter().cloned());
            Ok(Value::tuple(heap, v))
        }
        (Bin::Add, Value::Str(_) | Value::List(_) | Value::Tuple(_), _) => Err(msg::err(
            heap,
            "concat",
            None,
            &[("left", tn(heap, a)), ("right", tn(heap, b))],
        )),
        (Bin::Mult, s, n) if is_seq(s) => {
            if n.as_int().is_some() {
                repeat(heap, s, n, m)
            } else {
                Err(msg::err(
                    heap,
                    "mul_non_int",
                    None,
                    &[("type", tn(heap, n))],
                ))
            }
        }
        (Bin::Mult, n, s) if is_seq(s) => {
            if n.as_int().is_some() {
                repeat(heap, s, n, m)
            } else {
                Err(msg::err(
                    heap,
                    "mul_non_int",
                    None,
                    &[("type", tn(heap, n))],
                ))
            }
        }
        _ => Err(unsupported(heap, a, b, op, inplace)),
    }
}

fn arith(heap: &Heap, x: &Num, y: &Num, op: Bin, m: &mut Meter) -> R<Value> {
    if let (Num::Int(a), Num::Int(b)) = (x, y) {
        m.charge(heap, ((a.bits() + b.bits()) / 64) + 1)?;
        return Ok(match op {
            Bin::Add => Value::int(heap, a + b),
            Bin::Sub => Value::int(heap, a - b),
            Bin::Mult => Value::int(heap, a * b),
            Bin::Div => {
                if b.is_zero() {
                    return Err(msg::err(heap, "div_by_zero", Some("int"), &[]));
                }
                Value::Float(int_div(heap, a, b)?)
            }
            Bin::FloorDiv => {
                if b.is_zero() {
                    return Err(msg::err(heap, "floordiv_by_zero", Some("int"), &[]));
                }
                Value::int(heap, a.div_floor(b))
            }
            Bin::Mod => {
                if b.is_zero() {
                    return Err(msg::err(heap, "mod_by_zero", Some("int"), &[]));
                }
                Value::int(heap, a.mod_floor(b))
            }
            Bin::BitOr => unreachable!("binary 先分出了 |"),
        });
    }
    let (a, b) = (float_of(heap, x)?, float_of(heap, y)?);
    Ok(Value::Float(match op {
        Bin::Add => a + b,
        Bin::Sub => a - b,
        Bin::Mult => a * b,
        Bin::Div => {
            if b == 0.0 {
                return Err(msg::err(heap, "div_by_zero", Some("float"), &[]));
            }
            a / b
        }
        Bin::FloorDiv => {
            if b == 0.0 {
                return Err(msg::err(heap, "floordiv_by_zero", Some("float"), &[]));
            }
            float_floordiv(a, b).0
        }
        Bin::Mod => {
            if b == 0.0 {
                return Err(msg::err(heap, "mod_by_zero", Some("float"), &[]));
            }
            float_floordiv(a, b).1
        }
        Bin::BitOr => unreachable!("binary 先分出了 |"),
    }))
}

pub fn unary(heap: &Heap, v: &Value, op: Un, m: &mut Meter) -> R<Value> {
    m.charge(heap, 1)?;
    match op {
        Un::Not => Ok(Value::Bool(!v.truthy(heap))),
        Un::Neg => match v {
            Value::Bool(b) => Ok(Value::int(heap, -(*b as i64))),
            Value::Int(n) => Ok(Value::int(heap, -(**n).clone())),
            Value::Float(x) => Ok(Value::Float(-x)),
            other => Err(msg::err(
                heap,
                "bad_unary",
                None,
                &[("op", "-".into()), ("type", tn(heap, other))],
            )),
        },
    }
}

/// 拆包：`n` 个目标，`star` 是带星号的那一个的位置。
pub fn unpack(
    heap: &Heap,
    v: &Value,
    n: usize,
    star: Option<usize>,
    m: &mut Meter,
) -> R<Vec<Value>> {
    if !matches!(
        v,
        Value::List(_)
            | Value::Tuple(_)
            | Value::Str(_)
            | Value::Dict(_)
            | Value::Range(_)
            | Value::View(..)
            | Value::Proxy(_)
            | Value::Iter(_)
    ) {
        return Err(msg::err(
            heap,
            "unpack_non_iterable",
            None,
            &[("type", tn(heap, v))],
        ));
    }
    let mut items = vec![];
    collect(heap, v, &mut items, m)?;
    match star {
        None => {
            if items.len() < n {
                return Err(msg::err(
                    heap,
                    "unpack_few",
                    None,
                    &[("n", n.to_string()), ("got", items.len().to_string())],
                ));
            }
            if items.len() > n {
                return Err(msg::err(heap, "unpack_many", None, &[("n", n.to_string())]));
            }
            Ok(items)
        }
        Some(s) => {
            if items.len() < n - 1 {
                let text = msg::text(
                    "unpack_few",
                    None,
                    &[
                        ("n", format!("at least {}", n - 1)),
                        ("got", items.len().to_string()),
                    ],
                );
                return Err(msg::exc(
                    heap,
                    "ValueError",
                    vec![Value::str(heap, text)],
                    None,
                ));
            }
            let after = n - 1 - s;
            let rest = items.split_off(items.len() - after);
            let mid = items.split_off(s);
            items.push(Value::list(heap, mid));
            items.extend(rest);
            Ok(items)
        }
    }
}
