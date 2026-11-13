//! 首版清单从文档取；作用域先收绑定，再查每次使用。
//! 参考：docs/Lang0/Subset.md（清单、拦下时说什么）；docs/Lang0/Evaluator.md（构造时检查）。

mod expression;
mod types;

use crate::{
    ast::*,
    modules::{Program, Resolved, SourceModule},
    symbols::{self, Binding, Scope},
};

pub fn check(program: &Program) -> Result<(), String> {
    for (name, module) in &program.modules {
        let mut checker = Checker {
            program,
            name,
            module,
            scopes: vec![module.globals.clone()],
            functions: 0,
            loops: 0,
        };
        checker.allowed("statements", "Module", &(0..0))?;
        checker.suite(&module.ast.body)?;
    }
    Ok(())
}

struct Checker<'a> {
    program: &'a Program,
    name: &'a str,
    module: &'a SourceModule,
    scopes: Vec<Scope>,
    functions: usize,
    loops: usize,
}

impl Checker<'_> {
    fn fail<T>(&self, span: &Span, message: impl AsRef<str>) -> Result<T, String> {
        Err(self.module.error(span, message))
    }
    fn has(&self, group: &str, name: &str) -> bool {
        crate::modules::listed(&self.program.subset[group], name)
    }
    fn allowed(&self, group: &str, name: &str, span: &Span) -> Result<(), String> {
        if self.has(group, name) {
            Ok(())
        } else {
            self.fail(span, unsupported(name))
        }
    }
    fn operator(&self, op: &str, annotation: bool, span: &Span) -> Result<(), String> {
        if annotation && self.has("annotation_only", op) {
            Ok(())
        } else {
            self.allowed("operators", op, span)
        }
    }
    fn dunder(&self, name: &str, span: &Span) -> Result<(), String> {
        if dunder(name) {
            self.fail(span, "首版不收 dunder 名字")
        } else {
            Ok(())
        }
    }
    fn binding(&self, name: &str) -> Option<&Vec<Binding>> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }
    fn resolved(&self, e: &Expr) -> Option<Resolved> {
        match &e.kind {
            ExprKind::Name(name) => match self.binding(name).map(Vec::as_slice) {
                Some([Binding::Module(m)]) => Some(Resolved::Module(m.clone())),
                Some([Binding::From { module, name }]) => self.program.resolve(module, name).ok(),
                Some(_) => {
                    if self.scopes.iter().skip(1).any(|s| s.contains_key(name)) {
                        None
                    } else {
                        self.program.resolve(self.name, name).ok()
                    }
                }
                None if self.has("builtins", name) || self.has("exceptions", name) => {
                    Some(Resolved::Builtin(name.clone()))
                }
                None => None,
            },
            ExprKind::Attribute { value, attr } => match self.resolved(value)? {
                Resolved::Module(m) => self.program.resolve(&m, attr).ok(),
                _ => None,
            },
            _ => None,
        }
    }
    fn annotation_binding(&self, e: &Expr) -> Result<(), String> {
        let resolved = self
            .program
            .resolve_expr(self.name, e)
            .map_err(|m| self.module.error(&e.span, m))?;
        match resolved {
            Resolved::Builtin(_) | Resolved::Module(_) => Ok(()),
            Resolved::Binding {
                module,
                binding: Binding::Value(value),
            } => {
                let ExprKind::Call { func, .. } = value.kind else {
                    return self.fail(&e.span, unsupported("类型别名"));
                };
                if matches!(self.program.resolve_expr(&module, &func), Ok(Resolved::Builtin(n)) if matches!(n.as_str(), "typing.TypedDict" | "croon.Port" | "croon.Schema"))
                {
                    Ok(())
                } else {
                    self.fail(&e.span, unsupported("类型别名"))
                }
            }
            _ => self.fail(&e.span, "标注须是模块顶层唯一的一次赋值；首版不收类型别名"),
        }
    }
    fn builtin(&self, e: &Expr) -> Option<String> {
        match self.resolved(e) {
            Some(Resolved::Builtin(n)) => Some(n),
            _ => None,
        }
    }
    fn suite(&mut self, body: &[Stmt]) -> Result<(), String> {
        for s in body {
            self.statement(s)?;
        }
        Ok(())
    }
    fn statement(&mut self, s: &Stmt) -> Result<(), String> {
        use StmtKind as K;
        self.allowed("statements", s.name(), &s.span)?;
        match &s.kind {
            K::Expr(e) => self.expression(e, false)?,
            K::Assign { targets, value } => {
                for t in targets {
                    self.target(t)?;
                }
                self.expression(value, false)?;
            }
            K::AugAssign { target, op, value } => {
                if !matches!(
                    target.kind,
                    ExprKind::Name(_) | ExprKind::Attribute { .. } | ExprKind::Subscript { .. }
                ) {
                    return self.fail(&target.span, "增量赋值的目标须是名字、属性或下标");
                }
                self.target(target)?;
                self.operator(op, false, &s.span)?;
                self.expression(value, false)?;
            }
            K::FunctionDef(f) => {
                self.dunder(&f.name, &s.span)?;
                if let Some(d) = f.decorators.first() {
                    return self.fail(&d.span, unsupported("装饰器"));
                }
                if let Some(span) = &f.parameter_error {
                    return self.fail(span, unsupported("函数参数分隔符、*args 或 **kwargs"));
                }
                if let Some(span) = &f.type_params {
                    return self.fail(span, unsupported("type_params"));
                }
                let mut names = std::collections::BTreeSet::new();
                for arg in &f.args {
                    if !names.insert(&arg.name) {
                        return self.fail(&arg.span, "函数参数名重复");
                    }
                    self.dunder(&arg.name, &arg.span)?;
                    if let Some(e) = &arg.annotation {
                        self.expression(e, true)?;
                    }
                    if let Some(e) = &arg.default {
                        self.expression(e, false)?;
                    }
                }
                if let Some(e) = &f.returns {
                    self.expression(e, true)?;
                }
                let mut scope = symbols::collect(&f.body, &self.module.imports, false);
                for arg in &f.args {
                    symbols::bind(&mut scope, &arg.name, Binding::Local);
                }
                self.scopes.push(scope);
                self.functions += 1;
                let loops = std::mem::replace(&mut self.loops, 0);
                self.suite(&f.body)?;
                self.loops = loops;
                self.functions -= 1;
                self.scopes.pop();
            }
            K::Return(e) => {
                if self.functions == 0 {
                    return self.fail(&s.span, "return 须在函数里");
                }
                if let Some(e) = e {
                    self.expression(e, false)?;
                }
            }
            K::If { test, body, orelse } => {
                self.expression(test, false)?;
                self.suite(body)?;
                self.suite(orelse)?;
            }
            K::For {
                target,
                iter,
                body,
                orelse,
            } => {
                self.target(target)?;
                self.expression(iter, false)?;
                self.loops += 1;
                self.suite(body)?;
                self.loops -= 1;
                self.suite(orelse)?;
            }
            K::While { test, body, orelse } => {
                self.expression(test, false)?;
                self.loops += 1;
                self.suite(body)?;
                self.loops -= 1;
                self.suite(orelse)?;
            }
            K::Break | K::Continue if self.loops == 0 => {
                return self.fail(&s.span, format!("{} 须在循环里", s.name()));
            }
            K::Try {
                body,
                handlers,
                orelse,
                finalbody,
            } => {
                if !orelse.is_empty() {
                    return self.fail(&orelse[0].span, unsupported("try else"));
                }
                self.suite(body)?;
                for h in handlers {
                    if let Some(t) = &h.typ {
                        self.expression(t, false)?;
                    }
                    if let Some(name) = &h.name {
                        self.dunder(name, &s.span)?;
                    }
                    self.suite(&h.body)?;
                }
                self.suite(finalbody)?;
            }
            K::Raise { exc, cause } => {
                if let Some(e) = cause {
                    return self.fail(&e.span, unsupported("raise from"));
                }
                if let Some(e) = exc {
                    self.expression(e, false)?;
                }
            }
            K::Import(names) | K::ImportFrom { names, .. } => {
                if let K::ImportFrom { module, .. } = &s.kind {
                    for name in module.split('.') {
                        self.dunder(name, &s.span)?;
                    }
                }
                for a in names {
                    for name in a.name.split('.') {
                        self.dunder(name, &a.span)?;
                    }
                    if let Some(name) = &a.asname {
                        self.dunder(name, &a.span)?;
                    }
                }
            }
            K::Pass | K::Break | K::Continue => {}
            K::Unsupported(_) => unreachable!("rejected by whitelist"),
        }
        Ok(())
    }

    fn target(&mut self, e: &Expr) -> Result<(), String> {
        self.allowed("expressions", e.name(), &e.span)?;
        match &e.kind {
            ExprKind::Name(name) => self.dunder(name, &e.span),
            ExprKind::List(es) | ExprKind::Tuple(es) => {
                let mut starred = false;
                for e in es {
                    if let ExprKind::Starred(inner) = &e.kind {
                        self.allowed("expressions", "Starred", &e.span)?;
                        if starred {
                            return self.fail(&e.span, "同一层解包只能有一个星号目标");
                        }
                        starred = true;
                        self.target(inner)?;
                    } else {
                        self.target(e)?;
                    }
                }
                Ok(())
            }
            ExprKind::Attribute { .. } | ExprKind::Subscript { .. } => self.expression(e, false),
            _ => self.fail(
                &e.span,
                "赋值目标须是名字、属性、下标或解包；星号目标须放在 list 或 tuple 里",
            ),
        }
    }
}
