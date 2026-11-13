//! 表达式的清单、类型位置与静态可知的方法限制。
//! 参考：docs/Lang0/Subset.md（清单、拦下时说什么）；docs/Lang0/README.md（运行时对象）。

use super::*;

impl Checker<'_> {
    pub(super) fn expression(&mut self, e: &Expr, annotation: bool) -> Result<(), String> {
        use ExprKind as K;
        if annotation && !matches!(e.kind, K::Name(_) | K::Attribute { .. }) {
            return self.type_expr(e);
        }
        self.allowed("expressions", e.name(), &e.span)?;
        match &e.kind {
            K::Name(name) => {
                self.dunder(name, &e.span)?;
                if annotation {
                    self.annotation_binding(e)?;
                }
                if self.binding(name).is_none()
                    && !self.has("builtins", name)
                    && !self.has("exceptions", name)
                {
                    return self.fail(
                        &e.span,
                        if name == "print" {
                            unsupported(name)
                        } else {
                            format!("{}；名字 {name} 没有绑定", unsupported(name))
                        },
                    );
                }
            }
            K::BoolOp { op, values } => {
                self.operator(op, annotation, &e.span)?;
                self.expressions(values, annotation)?;
            }
            K::BinOp { left, op, right } => {
                self.operator(op, annotation, &e.span)?;
                if op == "Mod"
                    && matches!(left.kind, K::Constant(Constant::Str(_)) | K::JoinedStr(_))
                {
                    return self.fail(&e.span, "用 f-string 或 str.format 代替 %");
                }
                self.expression(left, annotation)?;
                self.expression(right, annotation)?;
            }
            K::UnaryOp { op, operand } => {
                self.operator(op, annotation, &e.span)?;
                self.expression(operand, annotation)?;
            }
            K::IfExp { test, body, orelse } => {
                self.expression(test, annotation)?;
                self.expression(body, annotation)?;
                self.expression(orelse, annotation)?;
            }
            K::Dict(items) => {
                for (key, value) in items {
                    if let Some(key) = key {
                        self.expression(key, false)?;
                    }
                    self.expression(value, annotation)?;
                }
            }
            K::List(es) | K::Tuple(es) | K::JoinedStr(es) => self.expressions(es, annotation)?,
            K::ListComp { elt, generators } => self.comprehension(generators, &[elt])?,
            K::DictComp {
                key,
                value,
                generators,
            } => self.comprehension(generators, &[key, value])?,
            K::Compare { operands, ops } => {
                for (i, op) in ops.iter().enumerate() {
                    self.operator(op, annotation, &e.span)?;
                    let value_literal = |x: &Expr| {
                        matches!(
                            x.kind,
                            K::Constant(Constant::Int(_) | Constant::Float(_) | Constant::Str(_))
                                | K::JoinedStr(_)
                                | K::Tuple(_)
                        )
                    };
                    if matches!(op.as_str(), "Is" | "IsNot")
                        && (value_literal(&operands[i]) || value_literal(&operands[i + 1]))
                    {
                        return self.fail(&e.span, "用 == 比较值");
                    }
                }
                self.expressions(operands, annotation)?;
            }
            K::Call {
                func,
                args,
                keywords,
            } => {
                if let K::Attribute { value, attr } = &func.kind
                    && attr == "sleep"
                    && matches!(&value.kind, K::Name(n) if n == "time")
                    && self.binding("time").is_none()
                {
                    return self.fail(&func.span, "用 receive 的 timeout 代替");
                }
                if self
                    .builtin(func)
                    .is_some_and(|h| types::CONSTRUCTORS.contains(&h.as_str()))
                {
                    return self.constructor_call(e);
                }
                self.expression(func, false)?;
                if self.functions == 0
                    && self.resolved(func).is_some()
                    && crate::infer::is_port(self.program, self.name, func)
                {
                    return self.fail(
                        &e.span,
                        "引用值写在 `main` 的参数里，工具在外层函数里捕获它",
                    );
                }
                for arg in args {
                    self.expression(arg, false)?;
                }
                let mut names = std::collections::BTreeSet::new();
                for k in keywords {
                    if let Some(name) = &k.arg {
                        if !names.insert(name) {
                            return self.fail(&k.value.span, "调用的关键字参数名重复");
                        }
                        self.dunder(name, &k.value.span)?;
                    }
                    self.expression(&k.value, false)?;
                }
                if let K::Attribute { value, attr } = &func.kind
                    && attr == "format"
                    && let Some(text) = value.string()
                    && !simple_format(text)
                {
                    return self.fail(
                        &value.span,
                        unsupported("str.format 的转换、格式说明或字段路径；只收 {} 与 {名字}"),
                    );
                }
            }
            K::Constant(_) => {}
            K::FormattedValue {
                value,
                conversion,
                format_spec,
                debug,
            } => {
                if *conversion || *format_spec || *debug {
                    return self.fail(
                        &e.span,
                        unsupported("f-string 的转换、格式说明或调试写法；只收 {x}"),
                    );
                }
                self.expression(value, false)?;
            }
            K::Attribute { value, attr } => {
                if dunder(attr) {
                    return self.fail(&e.span, "失败的种类用 `croon.kind(e)`");
                }
                self.expression(value, annotation)?;
                if annotation {
                    self.annotation_binding(e)?;
                }
                if let Some(Resolved::Module(module)) = self.resolved(value) {
                    let exists = if self.program.builtin_module(&module) {
                        self.program.subset["modules"][&module]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|v| v.as_str() == Some(attr))
                    } else {
                        self.program.modules[&module].globals.contains_key(attr)
                    };
                    if !exists {
                        return self.fail(&e.span, unsupported(&format!(".{attr}")));
                    }
                } else {
                    let typ = match &value.kind {
                        K::Dict(_) | K::DictComp { .. } => Some("dict"),
                        K::List(_) | K::ListComp { .. } => Some("list"),
                        K::Constant(Constant::Str(_)) | K::JoinedStr(_) => Some("str"),
                        K::Tuple(_) => Some("tuple"),
                        K::Constant(_) => Some(""),
                        _ => None,
                    };
                    let methods = &self.program.subset["methods"];
                    let allowed =
                        if let Some(typ) = typ {
                            crate::modules::listed(&methods[typ], attr)
                        } else {
                            methods
                                .as_object()
                                .unwrap()
                                .values()
                                .any(|forms| crate::modules::listed(forms, attr))
                                || [
                                    "value",
                                    "dependencies",
                                    "attach",
                                    "derived",
                                    "handle",
                                    "revoker",
                                    "port",
                                    "sender",
                                    "via",
                                    "name",
                                    "spec",
                                ]
                                .contains(&attr.as_str())
                                || self.program.modules.iter().any(|(name, m)| {
                                    !name.is_empty() && m.globals.contains_key(attr)
                                })
                                || self.program.modules.values().any(|m| {
                                    m.imports.values().flatten().any(|(_, b)| {
                                        let module = match b {
                                            Binding::Module(module)
                                            | Binding::From { module, .. } => module,
                                            _ => return false,
                                        };
                                        self.program.subset["modules"][module]
                                            .as_array()
                                            .is_some_and(|names| {
                                                names.iter().any(|n| n.as_str() == Some(attr))
                                            })
                                    })
                                })
                        };
                    if !allowed {
                        return self.fail(&e.span, unsupported(&format!(".{attr}")));
                    }
                }
            }
            K::Subscript { value, slice } => {
                let head = self.builtin(value);
                if matches!(
                    head.as_deref(),
                    Some(
                        "list"
                            | "dict"
                            | "typing.NotRequired"
                            | "typing.Annotated"
                            | "typing.Callable"
                            | "typing.Literal"
                    )
                ) {
                    return self.type_expr(e);
                }
                self.expression(value, false)?;
                self.expression(slice, false)?;
            }
            K::Starred(value) => self.expression(value, annotation)?,
            K::Unsupported(_) => unreachable!("rejected by whitelist"),
        }
        Ok(())
    }

    fn expressions(&mut self, es: &[Expr], annotation: bool) -> Result<(), String> {
        for e in es {
            self.expression(e, annotation)?;
        }
        Ok(())
    }

    fn comprehension(
        &mut self,
        generators: &[Comprehension],
        values: &[&Expr],
    ) -> Result<(), String> {
        let Some(first) = generators.first() else {
            return Ok(());
        };
        self.expression(&first.iter, false)?;
        let mut scope = Scope::new();
        for g in generators {
            if g.is_async {
                return self.fail(&g.span, unsupported("async comprehension"));
            }
            let mut names = vec![];
            symbols::target_names(&g.target, &mut names);
            for name in names {
                symbols::bind(&mut scope, &name, Binding::Local);
            }
        }
        self.scopes.push(scope);
        for (i, g) in generators.iter().enumerate() {
            if i > 0 {
                self.expression(&g.iter, false)?;
            }
            self.target(&g.target)?;
            self.expressions(&g.ifs, false)?;
        }
        for value in values {
            self.expression(value, false)?;
        }
        self.scopes.pop();
        Ok(())
    }
}

fn simple_format(text: &str) -> bool {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '{' && c != '}' {
            continue;
        }
        if chars.peek() == Some(&c) {
            chars.next();
            continue;
        }
        if c == '}' {
            return false;
        }
        let mut field = String::new();
        loop {
            match chars.next() {
                Some('}') => break,
                Some(c) => field.push(c),
                None => return false,
            }
        }
        if !field.is_empty() && !crate::parser::is_identifier(&field) {
            return false;
        }
    }
    true
}
