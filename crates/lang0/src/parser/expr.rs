//! 表达式转换；保留任意精度整数的十进制文本，不提前收窄到 Value。
//! 参考：docs/README.md（值与类型）；docs/Subset.md（清单）。

use super::span;
use crate::ast::*;
use ruff_python_ast as py;

pub(super) fn many(es: &[py::Expr]) -> Vec<Expr> {
    es.iter().map(convert).collect()
}
fn boxed(e: &py::Expr) -> Box<Expr> {
    Box::new(convert(e))
}
fn generators(gs: &[py::Comprehension]) -> Vec<Comprehension> {
    gs.iter()
        .map(|g| Comprehension {
            span: span(g),
            target: convert(&g.target),
            iter: convert(&g.iter),
            ifs: many(&g.ifs),
            is_async: g.is_async,
        })
        .collect()
}

pub(super) fn convert(e: &py::Expr) -> Expr {
    use ExprKind as K;
    let kind = match e {
        py::Expr::BoolOp(e) => K::BoolOp {
            op: format!("{:?}", e.op),
            values: many(&e.values),
        },
        py::Expr::BinOp(e) => K::BinOp {
            left: boxed(&e.left),
            op: format!("{:?}", e.op),
            right: boxed(&e.right),
        },
        py::Expr::UnaryOp(e) => K::UnaryOp {
            op: format!("{:?}", e.op),
            operand: boxed(&e.operand),
        },
        py::Expr::If(e) => K::IfExp {
            test: boxed(&e.test),
            body: boxed(&e.body),
            orelse: boxed(&e.orelse),
        },
        py::Expr::Dict(e) => K::Dict(
            e.items
                .iter()
                .map(|i| (i.key.as_ref().map(convert), convert(&i.value)))
                .collect(),
        ),
        py::Expr::List(e) => K::List(many(&e.elts)),
        py::Expr::Tuple(e) => K::Tuple(many(&e.elts)),
        py::Expr::ListComp(e) => K::ListComp {
            elt: boxed(&e.elt),
            generators: generators(&e.generators),
        },
        py::Expr::DictComp(e) => match &e.key {
            Some(key) => K::DictComp {
                key: boxed(key),
                value: boxed(&e.value),
                generators: generators(&e.generators),
            },
            None => K::Unsupported("DictComp unpacking".into()),
        },
        py::Expr::Compare(e) => K::Compare {
            operands: many(&e.operands),
            ops: e.ops.iter().map(|o| format!("{o:?}")).collect(),
        },
        py::Expr::Call(e) => K::Call {
            func: boxed(&e.func),
            args: many(&e.arguments.args),
            keywords: e
                .arguments
                .keywords
                .iter()
                .map(|k| Keyword {
                    arg: k.arg.as_ref().map(ToString::to_string),
                    value: convert(&k.value),
                })
                .collect(),
        },
        py::Expr::StringLiteral(e) => K::Constant(Constant::Str(e.value.to_str().to_owned())),
        py::Expr::NumberLiteral(e) => match &e.value {
            py::Number::Int(n) => K::Constant(Constant::Int(n.to_string())),
            py::Number::Float(n) => K::Constant(Constant::Float(*n)),
            py::Number::Complex { .. } => K::Unsupported("complex".into()),
        },
        py::Expr::BooleanLiteral(e) => K::Constant(Constant::Bool(e.value)),
        py::Expr::NoneLiteral(_) => K::Constant(Constant::None),
        py::Expr::FString(e) => {
            let mut values = vec![];
            for part in e.value.iter() {
                match part {
                    py::FStringPartRef::Literal(s) => values.push(Expr {
                        span: span(s),
                        kind: K::Constant(Constant::Str(s.value.to_string())),
                    }),
                    py::FStringPartRef::FString(f) => {
                        for element in &f.elements {
                            values.push(match element {
                                py::InterpolatedStringElement::Literal(s) => Expr {
                                    span: span(s),
                                    kind: K::Constant(Constant::Str(s.value.to_string())),
                                },
                                py::InterpolatedStringElement::Interpolation(v) => Expr {
                                    span: span(v),
                                    kind: K::FormattedValue {
                                        value: boxed(&v.expression),
                                        conversion: v.conversion != py::ConversionFlag::None,
                                        format_spec: v.format_spec.is_some(),
                                        debug: v.debug_text.is_some(),
                                    },
                                },
                            });
                        }
                    }
                }
            }
            K::JoinedStr(values)
        }
        py::Expr::Attribute(e) => K::Attribute {
            value: boxed(&e.value),
            attr: e.attr.to_string(),
        },
        py::Expr::Subscript(e) => K::Subscript {
            value: boxed(&e.value),
            slice: boxed(&e.slice),
        },
        py::Expr::Name(e) => K::Name(e.id.to_string()),
        py::Expr::Starred(e) => K::Starred(boxed(&e.value)),
        py::Expr::Named(_) => K::Unsupported("NamedExpr".into()),
        py::Expr::Lambda(_) => K::Unsupported("Lambda".into()),
        py::Expr::Set(_) => K::Unsupported("Set".into()),
        py::Expr::SetComp(_) => K::Unsupported("SetComp".into()),
        py::Expr::Generator(_) => K::Unsupported("GeneratorExp".into()),
        py::Expr::Await(_) => K::Unsupported("Await".into()),
        py::Expr::Yield(_) => K::Unsupported("Yield".into()),
        py::Expr::YieldFrom(_) => K::Unsupported("YieldFrom".into()),
        py::Expr::BytesLiteral(_) => K::Unsupported("bytes".into()),
        py::Expr::EllipsisLiteral(_) => K::Unsupported("Ellipsis".into()),
        py::Expr::Slice(_) => K::Unsupported("Slice".into()),
        py::Expr::TString(_) => K::Unsupported("TemplateStr".into()),
        py::Expr::IpyEscapeCommand(_) => K::Unsupported("IpyEscapeCommand".into()),
    };
    Expr {
        span: span(e),
        kind,
    }
}
