//! Python 3.12 的解析与到自有 AST 的转换；版本兼容错误也属于解析失败。
//! 参考：docs/Engineering.md（选择）；docs/Subset.md（清单）。

mod expr;

use crate::ast::*;
use ruff_python_ast as py;
use ruff_python_parser::{Mode, ParseOptions};
use ruff_text_size::Ranged;

fn span(node: &impl Ranged) -> Span {
    node.start().to_usize()..node.end().to_usize()
}

pub fn parse(source: &str, path: &str) -> Result<Module, String> {
    let parsed = ruff_python_parser::parse(
        source,
        ParseOptions::from(Mode::Module).with_target_version(py::PythonVersion::PY312),
    )
    .map_err(|e| diagnostic(path, source, &span(&e), e.to_string()))?;
    if let Some(e) = parsed.unsupported_syntax_errors().first() {
        return Err(diagnostic(path, source, &span(e), e.to_string()));
    }
    let py::Mod::Module(module) = parsed.syntax() else {
        unreachable!("module mode")
    };
    Ok(Module {
        body: suite(&module.body),
    })
}

fn suite(body: &[py::Stmt]) -> Vec<Stmt> {
    body.iter().map(statement).collect()
}
fn optional(e: Option<&py::Expr>) -> Option<Expr> {
    e.map(expr::convert)
}
fn aliases(names: &[py::Alias]) -> Vec<Alias> {
    names
        .iter()
        .map(|a| Alias {
            span: span(a),
            name: a.name.to_string(),
            asname: a.asname.as_ref().map(ToString::to_string),
        })
        .collect()
}

fn statement(s: &py::Stmt) -> Stmt {
    use StmtKind as K;
    let kind = match s {
        py::Stmt::Expr(s) => K::Expr(expr::convert(&s.value)),
        py::Stmt::Assign(s) => K::Assign {
            targets: expr::many(&s.targets),
            value: expr::convert(&s.value),
        },
        py::Stmt::AugAssign(s) => K::AugAssign {
            target: expr::convert(&s.target),
            op: format!("{:?}", s.op),
            value: expr::convert(&s.value),
        },
        py::Stmt::FunctionDef(s) if !s.is_async => {
            let p = &s.parameters;
            let parameter_error = if !p.posonlyargs.is_empty()
                || !p.kwonlyargs.is_empty()
                || p.vararg.is_some()
                || p.kwarg.is_some()
            {
                Some(span(p.as_ref()))
            } else {
                None
            };
            K::FunctionDef(FunctionDef {
                name: s.name.to_string(),
                args: p
                    .args
                    .iter()
                    .map(|a| Arg {
                        span: span(&a.parameter),
                        name: a.parameter.name.to_string(),
                        annotation: optional(a.parameter.annotation.as_deref()),
                        default: optional(a.default.as_deref()),
                    })
                    .collect(),
                returns: optional(s.returns.as_deref()),
                body: suite(&s.body),
                decorators: s
                    .decorator_list
                    .iter()
                    .map(|d| expr::convert(&d.expression))
                    .collect(),
                parameter_error,
                type_params: s.type_params.as_deref().map(span),
            })
        }
        py::Stmt::FunctionDef(_) => K::Unsupported("AsyncFunctionDef".into()),
        py::Stmt::Return(s) => K::Return(optional(s.value.as_deref())),
        py::Stmt::If(s) => {
            let mut orelse = vec![];
            for clause in s.elif_else_clauses.iter().rev() {
                orelse = if let Some(test) = &clause.test {
                    vec![Stmt {
                        span: span(clause),
                        kind: K::If {
                            test: expr::convert(test),
                            body: suite(&clause.body),
                            orelse,
                        },
                    }]
                } else {
                    suite(&clause.body)
                };
            }
            K::If {
                test: expr::convert(&s.test),
                body: suite(&s.body),
                orelse,
            }
        }
        py::Stmt::For(s) if !s.is_async => K::For {
            target: expr::convert(&s.target),
            iter: expr::convert(&s.iter),
            body: suite(&s.body),
            orelse: suite(&s.orelse),
        },
        py::Stmt::For(_) => K::Unsupported("AsyncFor".into()),
        py::Stmt::While(s) => K::While {
            test: expr::convert(&s.test),
            body: suite(&s.body),
            orelse: suite(&s.orelse),
        },
        py::Stmt::Break(_) => K::Break,
        py::Stmt::Continue(_) => K::Continue,
        py::Stmt::Pass(_) => K::Pass,
        py::Stmt::Try(s) if !s.is_star => K::Try {
            body: suite(&s.body),
            orelse: suite(&s.orelse),
            finalbody: suite(&s.finalbody),
            handlers: s
                .handlers
                .iter()
                .map(|h| {
                    let py::ExceptHandler::ExceptHandler(h) = h;
                    ExceptHandler {
                        typ: optional(h.type_.as_deref()),
                        name: h.name.as_ref().map(ToString::to_string),
                        body: suite(&h.body),
                    }
                })
                .collect(),
        },
        py::Stmt::Try(_) => K::Unsupported("TryStar".into()),
        py::Stmt::Raise(s) => K::Raise {
            exc: optional(s.exc.as_deref()),
            cause: optional(s.cause.as_deref()),
        },
        py::Stmt::Import(s) => K::Import(aliases(&s.names)),
        py::Stmt::ImportFrom(s) => K::ImportFrom {
            module: s
                .module
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
            level: s.level as usize,
            names: aliases(&s.names),
        },
        py::Stmt::ClassDef(_) => K::Unsupported("ClassDef".into()),
        py::Stmt::Delete(_) => K::Unsupported("Delete".into()),
        py::Stmt::TypeAlias(_) => K::Unsupported("TypeAlias".into()),
        py::Stmt::AnnAssign(_) => K::Unsupported("AnnAssign".into()),
        py::Stmt::With(s) => K::Unsupported(if s.is_async { "AsyncWith" } else { "With" }.into()),
        py::Stmt::Match(_) => K::Unsupported("Match".into()),
        py::Stmt::Assert(_) => K::Unsupported("Assert".into()),
        py::Stmt::Global(_) => K::Unsupported("Global".into()),
        py::Stmt::Nonlocal(_) => K::Unsupported("Nonlocal".into()),
        py::Stmt::IpyEscapeCommand(_) => K::Unsupported("IpyEscapeCommand".into()),
    };
    Stmt {
        span: span(s),
        kind,
    }
}

/// str.isidentifier 也接受关键字；词法只需完整的一枚名字 token。
pub(crate) fn is_identifier(text: &str) -> bool {
    use py::token::TokenKind;
    if text.is_empty() {
        return false;
    }
    let mut lexer = ruff_python_parser::lexer::lex(text, Mode::Expression);
    let kind = lexer.next_token();
    (kind == TokenKind::Identifier || kind.is_keyword())
        && lexer.current_range().start().to_usize() == 0
        && lexer.current_range().end().to_usize() == text.len()
}
