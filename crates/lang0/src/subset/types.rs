//! 类型位置与推导用的字面量：类型位置只收类型的写法；TypedDict、Port、Schema 的字面量参数不论 main 用不用到都查。
//! 参考：docs/Lang0/Subset.md（清单）；docs/Lang0/README.md（Interface、Port 与交出）。

use super::*;

/// 推导 Schema 或 Port spec 的调用头。
pub(super) const CONSTRUCTORS: &[&str] = &[
    "typing.TypedDict",
    "croon.Port",
    "croon.Inbox",
    "croon.Schema",
];

impl Checker<'_> {
    /// 类型位置：Name、Attribute、Subscript、None、`|`，以及 Inbox、Port 的调用；Literal 与 Annotated 的参数另查。
    pub(super) fn type_expr(&mut self, e: &Expr) -> Result<(), String> {
        use ExprKind as K;
        self.allowed("expressions", e.name(), &e.span)?;
        match &e.kind {
            K::Constant(Constant::None) => Ok(()),
            K::Name(_) | K::Attribute { .. } => self.expression(e, true),
            K::BinOp { left, op, right } => {
                self.operator(op, true, &e.span)?;
                self.type_expr(left)?;
                self.type_expr(right)
            }
            K::Subscript { value, slice } => {
                self.expression(value, true)?;
                let args: Vec<&Expr> = match &slice.kind {
                    K::Tuple(es) => es.iter().collect(),
                    _ => vec![slice],
                };
                match self.builtin(value).as_deref() {
                    Some("typing.Literal") => args.iter().try_for_each(|a| self.literal_arg(a)),
                    Some("typing.Annotated") => {
                        let Some((first, rest)) = args.split_first() else {
                            return self.fail(&e.span, "Annotated 至少要一个类型");
                        };
                        self.type_expr(first)?;
                        for a in rest {
                            if a.string().is_none() {
                                return self.fail(&a.span, "Annotated 的说明须是字符串字面量");
                            }
                        }
                        Ok(())
                    }
                    Some("typing.Callable") => args.iter().try_for_each(|a| match &a.kind {
                        K::List(es) => es.iter().try_for_each(|x| self.type_expr(x)),
                        _ => self.type_expr(a),
                    }),
                    _ => args.iter().try_for_each(|a| self.type_expr(a)),
                }
            }
            K::Call { func, .. } => {
                // 先按类型位置查调用头：别名在这里报「首版不收类型别名」。
                self.expression(func, true)?;
                if matches!(
                    self.builtin(func).as_deref(),
                    Some("croon.Inbox" | "croon.Port")
                ) {
                    self.constructor_call(e)
                } else {
                    self.fail(&e.span, unsupported("类型位置上的 Call"))
                }
            }
            _ => self.fail(&e.span, unsupported(&format!("类型位置上的 {}", e.name()))),
        }
    }

    /// TypedDict、Port、Inbox、Schema 的调用：参数的形状、关键字与字面量的重复键。
    pub(super) fn constructor_call(&mut self, e: &Expr) -> Result<(), String> {
        let ExprKind::Call {
            func,
            args,
            keywords,
        } = &e.kind
        else {
            unreachable!("调用方查过")
        };
        self.expression(func, false)?;
        let head = self.builtin(func).unwrap_or_default();
        let (positional, allowed): (usize, &[&str]) = match head.as_str() {
            "typing.TypedDict" => (2, &[]),
            "croon.Port" => (1, &["attach", "doc"]),
            _ => (1, &[]),
        };
        if args.len() != positional || args.iter().any(|a| matches!(a.kind, ExprKind::Starred(_))) {
            return self.fail(&e.span, format!("{head} 收 {positional} 个位置参数"));
        }
        let mut names = std::collections::BTreeSet::new();
        for k in keywords {
            let Some(name) = k.arg.as_deref().filter(|n| allowed.contains(n)) else {
                return self.fail(&k.value.span, format!("{head} 不收这个关键字参数"));
            };
            if !names.insert(name) {
                return self.fail(&k.value.span, "调用的关键字参数名重复");
            }
            match name {
                "attach" => self.type_dict(&k.value, "attach 的名字重复")?,
                _ if k.value.string().is_some() => {}
                _ => return self.fail(&k.value.span, "doc 须是字符串字面量"),
            }
        }
        match head.as_str() {
            "typing.TypedDict" => {
                if args[0].string().is_none() {
                    return self.fail(&args[0].span, "TypedDict 的名字须是字符串字面量");
                }
                self.type_dict(&args[1], "TypedDict 的字段名重复")
            }
            "croon.Schema" => self.json_literal(&args[0]),
            "croon.Port" if matches!(args[0].kind, ExprKind::Dict(_)) => {
                if !keywords.is_empty() {
                    return self.fail(
                        &e.span,
                        "Port(spec) 写完整的 Port spec，不再收 attach 与 doc",
                    );
                }
                self.json_literal(&args[0])
            }
            _ => self.type_expr(&args[0]),
        }
    }

    /// 名字（字符串字面量，不重复）→ 类型的 dict 字面量。
    fn type_dict(&mut self, e: &Expr, duplicate: &str) -> Result<(), String> {
        let ExprKind::Dict(items) = &e.kind else {
            return self.fail(&e.span, "这里须是 dict 字面量");
        };
        let mut names = std::collections::BTreeSet::new();
        for (key, value) in items {
            let Some(key) = key else {
                return self.fail(&value.span, "这里不收 **");
            };
            let Some(name) = key.string() else {
                return self.fail(&key.span, "键须是字符串字面量");
            };
            if !names.insert(name) {
                return self.fail(&key.span, duplicate);
            }
            self.type_expr(value)?;
        }
        Ok(())
    }

    /// Schema 或 Port spec 的 JSON 字面量：dict（字符串键，不重复）、list、字符串、数、布尔、None。
    fn json_literal(&mut self, e: &Expr) -> Result<(), String> {
        match &e.kind {
            ExprKind::Constant(_) => Ok(()),
            ExprKind::UnaryOp { op, operand }
                if op == "USub"
                    && matches!(
                        operand.kind,
                        ExprKind::Constant(Constant::Int(_) | Constant::Float(_))
                    ) =>
            {
                Ok(())
            }
            ExprKind::List(es) => es.iter().try_for_each(|x| self.json_literal(x)),
            ExprKind::Dict(items) => {
                let mut names = std::collections::BTreeSet::new();
                for (key, value) in items {
                    let Some(key) = key else {
                        return self.fail(&value.span, "这里不收 **");
                    };
                    let Some(name) = key.string() else {
                        return self.fail(&key.span, "键须是字符串字面量");
                    };
                    if !names.insert(name) {
                        return self.fail(&key.span, "Schema 或 Port spec 的字面量键重复");
                    }
                    self.json_literal(value)?;
                }
                Ok(())
            }
            _ => self.fail(&e.span, "Schema 与 Port spec 只收 JSON 字面量"),
        }
    }

    fn literal_arg(&mut self, e: &Expr) -> Result<(), String> {
        match &e.kind {
            ExprKind::Constant(_) => Ok(()),
            ExprKind::UnaryOp { op, operand }
                if op == "USub"
                    && matches!(
                        operand.kind,
                        ExprKind::Constant(Constant::Int(_) | Constant::Float(_))
                    ) =>
            {
                Ok(())
            }
            _ => self.fail(&e.span, "Literal 的参数须是字面量"),
        }
    }
}
