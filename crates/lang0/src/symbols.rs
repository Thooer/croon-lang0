//! 绑定按整个作用域收集：赋值、参数、import、except 与推导式各归自己的作用域。
//! 参考：docs/Lang0/Subset.md（清单）；docs/Lang0/README.md（Interface）。

use crate::ast::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub enum Binding {
    Value(Expr),
    Function,
    Module(String),
    From { module: String, name: String },
    Local,
}

pub type Scope = BTreeMap<String, Vec<Binding>>;
pub type Imports = BTreeMap<usize, Vec<(String, Binding)>>;

pub fn bind(scope: &mut Scope, name: &str, value: Binding) {
    scope.entry(name.to_owned()).or_default().push(value);
}

pub fn target_names(e: &Expr, out: &mut Vec<String>) {
    match &e.kind {
        ExprKind::Name(name) => out.push(name.clone()),
        ExprKind::Tuple(es) | ExprKind::List(es) => {
            for e in es {
                target_names(e, out);
            }
        }
        ExprKind::Starred(e) => target_names(e, out),
        _ => {}
    }
}

fn target(scope: &mut Scope, e: &Expr, value: Binding) {
    let mut names = vec![];
    target_names(e, &mut names);
    for name in names {
        bind(scope, &name, value.clone());
    }
}

pub fn collect(body: &[Stmt], imports: &Imports, direct: bool) -> Scope {
    fn visit(body: &[Stmt], imports: &Imports, direct: bool, out: &mut Scope) {
        for s in body {
            match &s.kind {
                StmtKind::Assign { targets, value } => {
                    for t in targets {
                        let binding = if direct && matches!(t.kind, ExprKind::Name(_)) {
                            Binding::Value(value.clone())
                        } else {
                            Binding::Local
                        };
                        target(out, t, binding);
                    }
                }
                StmtKind::AugAssign { target: t, .. } | StmtKind::For { target: t, .. } => {
                    target(out, t, Binding::Local)
                }
                StmtKind::FunctionDef(f) => bind(
                    out,
                    &f.name,
                    if direct {
                        Binding::Function
                    } else {
                        Binding::Local
                    },
                ),
                StmtKind::Import(names) | StmtKind::ImportFrom { names, .. } => {
                    if let Some(bindings) = imports.get(&s.span.start) {
                        for (name, binding) in bindings {
                            bind(out, name, binding.clone());
                        }
                    } else {
                        for a in names {
                            let name = a.asname.as_deref().unwrap_or_else(|| {
                                if matches!(s.kind, StmtKind::Import(_)) {
                                    a.name.split('.').next().unwrap()
                                } else {
                                    &a.name
                                }
                            });
                            bind(out, name, Binding::Local);
                        }
                    }
                }
                StmtKind::Try { handlers, .. } => {
                    for h in handlers {
                        if let Some(name) = &h.name {
                            bind(out, name, Binding::Local);
                        }
                    }
                }
                _ => {}
            }
            for suite in s.suites() {
                visit(suite, imports, false, out);
            }
        }
    }
    let mut out = Scope::new();
    visit(body, imports, direct, &mut out);
    out
}

pub fn import_statements(body: &[Stmt], out: &mut Vec<Stmt>) {
    for s in body {
        match &s.kind {
            StmtKind::Import(_) | StmtKind::ImportFrom { .. } => out.push(s.clone()),
            StmtKind::FunctionDef(f) => import_statements(&f.body, out),
            _ => {}
        }
        for suite in s.suites() {
            import_statements(suite, out);
        }
    }
}
