//! repr、str 与字符串转数，照 CPython 3.12 冻结，不取决于宿主。
//! 参考：docs/Evaluator.md（文本）；docs/Subset.md（形状之外的规则、异常的消息）。
use super::heap::Heap;

use num_bigint::BigInt;
use num_traits::Num as _;

use super::msg;
use super::unicode::{DECIMAL, NOT_PRINTABLE, SPACE};
use super::value::{Form, Value, View};

pub type R<T> = Result<T, Value>;

/// int 与字符串互转的位数上限，照 CPython 3.12 的默认值。
pub const INT_DIGITS: usize = 4300;

fn within(table: &[(u32, u32)], c: char) -> bool {
    let c = c as u32;
    table
        .binary_search_by(|&(a, b)| {
            if c < a {
                std::cmp::Ordering::Greater
            } else if c > b {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

pub fn is_space(c: char) -> bool {
    within(SPACE, c)
}

/// Unicode 的十进制数字的值。
fn decimal(c: char) -> Option<u32> {
    let x = c as u32;
    DECIMAL
        .iter()
        .find(|&&(a, b)| a <= x && x <= b)
        .map(|&(a, _)| (x - a) % 10)
}

pub fn repr_str(s: &str) -> String {
    let mut out = String::new();
    write_quoted(s, &mut out);
    out
}

fn write_quoted(s: &str, out: &mut impl std::fmt::Write) {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    out.write_char(quote).unwrap();
    for c in s.chars() {
        match c {
            '\\' => out.write_str("\\\\").unwrap(),
            '\n' => out.write_str("\\n").unwrap(),
            '\r' => out.write_str("\\r").unwrap(),
            '\t' => out.write_str("\\t").unwrap(),
            c if c == quote => {
                out.write_char('\\').unwrap();
                out.write_char(c).unwrap();
            }
            c if within(NOT_PRINTABLE, c) => {
                let x = c as u32;
                if x < 0x100 {
                    write!(out, "\\x{x:02x}").unwrap();
                } else if x < 0x10000 {
                    write!(out, "\\u{x:04x}").unwrap();
                } else {
                    write!(out, "\\U{x:08x}").unwrap();
                }
            }
            c => out.write_char(c).unwrap(),
        }
    }
    out.write_char(quote).unwrap();
}

/// float 的 repr：能还原同一个 double 的最短十进制。
pub fn repr_float(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.into();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    // Rust 的 {:e} 给出最短的有效数字，如 1.2345e3。
    let e = format!("{x:e}");
    let (mantissa, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let (sign, mantissa) = mantissa
        .strip_prefix('-')
        .map_or(("", mantissa), |m| ("-", m));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    if (-4..16).contains(&exp) {
        let point = exp + 1;
        let body = if point <= 0 {
            format!("0.{}{digits}", "0".repeat((-point) as usize))
        } else if point as usize >= digits.len() {
            format!("{digits}{}.0", "0".repeat(point as usize - digits.len()))
        } else {
            format!(
                "{}.{}",
                &digits[..point as usize],
                &digits[point as usize..]
            )
        };
        format!("{sign}{body}")
    } else {
        let m = if digits.len() == 1 {
            digits
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        format!(
            "{sign}{m}e{}{:02}",
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        )
    }
}

pub fn int_text(heap: &Heap, n: &BigInt) -> R<String> {
    let s = n.to_string();
    if s.trim_start_matches('-').len() > INT_DIGITS {
        return Err(msg::err(heap, "int_digits", Some("to_str"), &[]));
    }
    Ok(s)
}

/// 引用值与含引用值的运行时对象转不成文本，见 docs/Evaluator.md 的「文本」。
pub fn carries_ref(heap: &Heap, v: &Value) -> bool {
    match v {
        Value::Ref(_) | Value::Inbox(_) | Value::Derived(_) => true,
        Value::Receipt(r) => {
            let r = r.borrow(heap);
            r.attach.borrow(heap).len() > 0 || r.derived.borrow(heap).len() > 0
        }
        Value::Arrival(a) => a.borrow(heap).attach.borrow(heap).len() > 0,
        _ => false,
    }
}

/// 只累计字节数；与产出共用遍历，大小与文本的转义、标点保持一致。
#[derive(Default)]
struct TextSize(u64);
impl std::fmt::Write for TextSize {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0 = self.0.saturating_add(s.len() as u64);
        Ok(())
    }
}

pub fn metered_str(heap: &Heap, v: &Value, meter: &mut super::ops::Meter) -> R<String> {
    let mut size = TextSize::default();
    write_text(heap, v, meter.depth, false, &mut size)?;
    meter.charge(heap, size.0)?;
    str_of(heap, v, meter.depth)
}

/// `depth` 是还能往下走的层数，深遍历每进一层用掉一层。
fn write_text(
    heap: &Heap,
    v: &Value,
    depth: usize,
    repr_: bool,
    out: &mut impl std::fmt::Write,
) -> R<()> {
    if depth == 0 {
        return Err(msg::limit(heap, "depth"));
    }
    if carries_ref(heap, v) {
        return Err(msg::err(heap, "ref_text", None, &[]));
    }
    let d = depth - 1;
    match v {
        Value::Str(s) if !repr_ => out.write_str(s).unwrap(),
        Value::Str(s) => write_quoted(s, out),
        Value::List(l) => write_sequence(heap, &l.borrow(heap), d, "[", "]", false, out)?,
        Value::Tuple(t) => write_sequence(heap, t, d, "(", ")", true, out)?,
        Value::Dict(m) | Value::Proxy(m) => {
            if matches!(v, Value::Proxy(_)) {
                out.write_str("mappingproxy(").unwrap();
            }
            out.write_str("{").unwrap();
            for (i, (k, v)) in m.borrow(heap).items().enumerate() {
                if i > 0 {
                    out.write_str(", ").unwrap();
                }
                write_text(heap, k, d, true, out)?;
                out.write_str(": ").unwrap();
                write_text(heap, v, d, true, out)?;
            }
            out.write_str("}").unwrap();
            if matches!(v, Value::Proxy(_)) {
                out.write_str(")").unwrap();
            }
        }
        Value::View(view, m) => {
            write!(out, "{}([", v.type_name(heap)).unwrap();
            for (i, (k, v)) in m.borrow(heap).items().enumerate() {
                if i > 0 {
                    out.write_str(", ").unwrap();
                }
                match view {
                    View::Keys => write_text(heap, k, d, true, out)?,
                    View::Values => write_text(heap, v, d, true, out)?,
                    View::Items => {
                        out.write_str("(").unwrap();
                        write_text(heap, k, d, true, out)?;
                        out.write_str(", ").unwrap();
                        write_text(heap, v, d, true, out)?;
                        out.write_str(")").unwrap();
                    }
                }
            }
            out.write_str("])").unwrap();
        }
        Value::Exc(e) => {
            let e = e.borrow(heap);
            if repr_ {
                write!(out, "{}(", e.class).unwrap();
                for (i, a) in e.args.iter().enumerate() {
                    if i > 0 {
                        out.write_str(", ").unwrap();
                    }
                    write_text(heap, a, d, true, out)?;
                }
                out.write_str(")").unwrap();
            } else {
                write_exception(heap, e.class, &e.args, depth, out)?;
            }
        }
        Value::Function(f) => write!(out, "<function {}>", f.borrow(heap).code.qualname).unwrap(),
        Value::Method(m) => {
            let m = m.borrow(heap);
            write!(
                out,
                "<built-in method {} of {} object>",
                m.1,
                m.0.type_name(heap)
            )
            .unwrap();
        }
        Value::Unbound(t, name) => write!(out, "<method '{name}' of '{t}' objects>").unwrap(),
        Value::Module(name) => write!(out, "<module '{name}'>").unwrap(),
        Value::None => out.write_str("None").unwrap(),
        Value::Bool(b) => out.write_str(if *b { "True" } else { "False" }).unwrap(),
        Value::Int(n) => out.write_str(&int_text(heap, n)?).unwrap(),
        Value::Float(x) => out.write_str(&repr_float(*x)).unwrap(),
        Value::Range(r) => {
            if r.step == BigInt::from(1) {
                write!(
                    out,
                    "range({}, {})",
                    int_text(heap, &r.start)?,
                    int_text(heap, &r.stop)?
                )
                .unwrap()
            } else {
                write!(
                    out,
                    "range({}, {}, {})",
                    int_text(heap, &r.start)?,
                    int_text(heap, &r.stop)?,
                    int_text(heap, &r.step)?
                )
                .unwrap()
            }
        }
        Value::Builtin(name) => write!(out, "<built-in function {name}>").unwrap(),
        Value::Class(c) => match c {
            super::value::Class::Type(t) => write!(out, "<class '{t}'>").unwrap(),
            super::value::Class::Exc(e @ ("Failure" | "Limit")) => {
                write!(out, "<class 'croon.{e}'>").unwrap()
            }
            super::value::Class::Exc(e) => write!(out, "<class '{e}'>").unwrap(),
        },
        Value::Ref(_) | Value::Inbox(_) | Value::Derived(_) => {
            unreachable!("carries_ref 先拦下了引用值")
        }
        Value::Delivery(_) => out.write_str("<Delivery object>").unwrap(),
        Value::Receipt(_) => out.write_str("<Receipt object>").unwrap(),
        Value::Arrival(_) => out.write_str("<Arrival object>").unwrap(),
        Value::Iter(_) | Value::Tool(_) => write!(out, "<{} object>", v.type_name(heap)).unwrap(),
        Value::Form(f) => match &**f {
            Form::Port => out.write_str("<Port object>").unwrap(),
            Form::Schema => out.write_str("<Schema object>").unwrap(),
            Form::Typing(name) => write!(out, "typing.{name}").unwrap(),
        },
    }
    Ok(())
}

fn write_sequence(
    heap: &Heap,
    items: &[Value],
    depth: usize,
    open: &str,
    close: &str,
    singleton: bool,
    out: &mut impl std::fmt::Write,
) -> R<()> {
    out.write_str(open).unwrap();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.write_str(", ").unwrap();
        }
        write_text(heap, item, depth, true, out)?;
    }
    if singleton && items.len() == 1 {
        out.write_str(",").unwrap();
    }
    out.write_str(close).unwrap();
    Ok(())
}

pub fn repr(heap: &Heap, v: &Value, depth: usize) -> R<String> {
    let mut out = String::new();
    write_text(heap, v, depth, true, &mut out)?;
    Ok(out)
}

pub fn str_of(heap: &Heap, v: &Value, depth: usize) -> R<String> {
    let mut out = String::new();
    write_text(heap, v, depth, false, &mut out)?;
    Ok(out)
}

pub fn exc_str(heap: &Heap, class: &str, args: &[Value], depth: usize) -> R<String> {
    let mut out = String::new();
    write_exception(heap, class, args, depth, &mut out)?;
    Ok(out)
}

fn write_exception(
    heap: &Heap,
    class: &str,
    args: &[Value],
    depth: usize,
    out: &mut impl std::fmt::Write,
) -> R<()> {
    match args {
        [] => {}
        [a] => write_text(heap, a, depth, class == "KeyError", out)?,
        _ => {
            if depth == 0 {
                return Err(msg::limit(heap, "depth"));
            }
            write_sequence(heap, args, depth - 1, "(", ")", true, out)?;
        }
    }
    Ok(())
}

/// 去掉首尾的 Unicode 空白，十进制数字换成 ASCII；None 是含别的字符。
fn ascii_number(s: &str) -> Option<String> {
    let t = s.trim_matches(|c| is_space(c) && !matches!(c, '\u{001c}'..='\u{001f}'));
    t.chars()
        .map(|c| match decimal(c) {
            Some(d) => Some(char::from_digit(d, 10).unwrap()),
            None if c.is_ascii() => Some(c),
            None => None,
        })
        .collect()
}

/// 数字之间可以有一个下划线。
fn underscored(digits: &str) -> Option<String> {
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
    {
        return None;
    }
    let plain: String = digits.chars().filter(|c| *c != '_').collect();
    plain.chars().all(|c| c.is_ascii_digit()).then_some(plain)
}

/// `int(str)`，十进制。
pub fn parse_int(heap: &Heap, s: &str) -> R<BigInt> {
    let bad = || msg::err(heap, "int_literal", None, &[("repr", repr_str(s))]);
    let t = ascii_number(s).ok_or_else(bad)?;
    let (neg, body) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, &t[..]),
    };
    let digits = underscored(body).ok_or_else(bad)?;
    if digits.len() > INT_DIGITS {
        return Err(msg::err(
            heap,
            "int_digits",
            Some("from_str"),
            &[("n", digits.len().to_string())],
        ));
    }
    let n = BigInt::from_str_radix(&digits, 10).map_err(|_| bad())?;
    Ok(if neg { -n } else { n })
}

/// `float(str)`。
pub fn parse_float(heap: &Heap, s: &str) -> R<f64> {
    let bad = || msg::err(heap, "float_literal", None, &[("repr", repr_str(s))]);
    let t = ascii_number(s).ok_or_else(bad)?;
    let (neg, body) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, &t[..]),
    };
    let lower = body.to_ascii_lowercase();
    let x = match lower.as_str() {
        "inf" | "infinity" => f64::INFINITY,
        "nan" => f64::NAN,
        _ => {
            let (mant, exp) = match lower.find('e') {
                Some(i) => (&lower[..i], Some(&lower[i + 1..])),
                None => (&lower[..], None),
            };
            let (int, frac) = match mant.find('.') {
                Some(i) => (&mant[..i], Some(&mant[i + 1..])),
                None => (mant, None),
            };
            let int = if int.is_empty() {
                Some(String::new())
            } else {
                underscored(int)
            };
            let frac = match frac {
                None => Some(String::new()),
                Some("") => Some(String::new()),
                Some(f) => underscored(f),
            };
            let (Some(int), Some(frac)) = (int, frac) else {
                return Err(bad());
            };
            if int.is_empty() && frac.is_empty() {
                return Err(bad());
            }
            let exp = match exp {
                None => String::new(),
                Some(e) => {
                    let (sign, digits) = match e.as_bytes().first() {
                        Some(b'-') => ("-", &e[1..]),
                        Some(b'+') => ("", &e[1..]),
                        _ => ("", e),
                    };
                    format!("e{sign}{}", underscored(digits).ok_or_else(bad)?)
                }
            };
            let text = format!(
                "{}.{}{exp}",
                if int.is_empty() { "0" } else { &int },
                if frac.is_empty() { "0" } else { &frac }
            );
            text.parse::<f64>().map_err(|_| bad())?
        }
    };
    Ok(if neg { -x } else { x })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_print_like_cpython() {
        for (x, s) in [
            (1.0, "1.0"),
            (0.1, "0.1"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (123.456, "123.456"),
            (-2.5e-7, "-2.5e-07"),
            (1.5e300, "1.5e+300"),
            (f64::MAX, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
            (-0.0, "-0.0"),
        ] {
            assert_eq!(repr_float(x), s);
        }
    }

    #[test]
    fn strings_repr_like_cpython() {
        assert_eq!(repr_str("a'b"), "\"a'b\"");
        assert_eq!(repr_str("a'\"b"), "'a\\'\"b'");
        assert_eq!(repr_str("\n\x00\u{200b}é"), "'\\n\\x00\\u200bé'");
    }

    #[test]
    fn strings_parse_like_cpython() {
        assert_eq!(
            parse_int(&Heap::default(), " 1_000 ").unwrap(),
            BigInt::from(1000)
        );
        assert_eq!(parse_int(&Heap::default(), "٣").unwrap(), BigInt::from(3));
        assert!(parse_int(&Heap::default(), "1__0").is_err());
        assert_eq!(parse_float(&Heap::default(), "1_0.5e1_0").unwrap(), 10.5e10);
        assert_eq!(parse_float(&Heap::default(), ".5").unwrap(), 0.5);
        assert_eq!(parse_float(&Heap::default(), "5.").unwrap(), 5.0);
        assert!(parse_float(&Heap::default(), "-InFinity").unwrap() == f64::NEG_INFINITY);
        assert!(parse_float(&Heap::default(), ".").is_err());
        assert!(parse_float(&Heap::default(), "1e").is_err());
    }
}
