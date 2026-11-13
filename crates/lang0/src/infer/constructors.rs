//! TypedDict、Schema、Port 与 Inbox 的静态构造。
//! 参考：docs/Lang0/README.md（值与类型、Port 与交出）。

use super::*;

impl Infer<'_> {
    pub(super) fn call(
        &mut self,
        module: &str,
        e: &Expr,
        func: &Expr,
        args: &[Expr],
        keywords: &[Keyword],
    ) -> Result<Type, String> {
        let name = self.head(module, func)?;
        match name.as_str() {
            "typing.TypedDict" if args.len() == 2 && keywords.is_empty() => {
                self.text(module, &args[0])?;
                let ExprKind::Dict(items) = &args[1].kind else {
                    return Err(self.error(module, &args[1], "TypedDict 的字段须写成字面量 dict"));
                };
                let (mut properties, mut required) = (Map::new(), vec![]);
                for (key, value) in items {
                    let key = key.as_ref().ok_or_else(|| {
                        self.error(module, &args[1], "TypedDict 的字段须写明名字")
                    })?;
                    let name = self.text(module, key)?;
                    let (typ, optional) = self.optional(module, value)?;
                    if properties
                        .insert(name.clone(), self.schema(module, typ)?)
                        .is_some()
                    {
                        return Err(self.error(module, key, "TypedDict 的字段名重复"));
                    }
                    if !optional {
                        required.push(name);
                    }
                }
                Ok(Type::Schema(object(properties, required)))
            }
            "croon.Schema" if args.len() == 1 && keywords.is_empty() => {
                let schema = self.literal(module, &args[0])?;
                croon_eval::schema::check_schema(&schema).map_err(|es| {
                    self.error(
                        module,
                        &args[0],
                        es.iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join("；"),
                    )
                })?;
                Ok(Type::Schema(schema))
            }
            "croon.Port" if args.len() == 1 => {
                let mut seen = BTreeSet::new();
                for k in keywords {
                    if !matches!(k.arg.as_deref(), Some("attach" | "doc"))
                        || !seen.insert(k.arg.clone())
                    {
                        return Err(self.error(
                            module,
                            &k.value,
                            "Port 只收 attach 与 doc，各写一次",
                        ));
                    }
                }
                if matches!(args[0].kind, ExprKind::Dict(_)) {
                    if !keywords.is_empty() {
                        return Err(self.error(
                            module,
                            e,
                            "Port(spec) 的完整字面量不另收 attach 或 doc",
                        ));
                    }
                    return Ok(Type::Port(self.literal(module, &args[0])?));
                }
                let mut spec = json!({"description": "", "in": self.schema(module, &args[0])?, "attach": {"additionalProperties": false}});
                for k in keywords {
                    match k.arg.as_deref() {
                        Some("doc") => spec["description"] = json!(self.text(module, &k.value)?),
                        Some("attach") => spec["attach"] = self.attach(module, &k.value)?,
                        _ => unreachable!(),
                    }
                }
                Ok(Type::Port(spec))
            }
            "croon.Inbox" if args.len() == 1 && keywords.is_empty() => {
                match self.typ(module, &args[0])? {
                    Type::Port(spec) => Ok(Type::Inbox(spec)),
                    _ => Err(self.error(module, &args[0], "Inbox 须包一份 Port spec")),
                }
            }
            _ => Err(self.error(module, e, "类型构造不合推导规则；用 TypedDict 或 Literal")),
        }
    }

    fn optional<'a>(&mut self, module: &str, e: &'a Expr) -> Result<(&'a Expr, bool), String> {
        if let ExprKind::Subscript { value, slice } = &e.kind
            && matches!(self.typ(module, value)?, Type::Constructor(n) if n == "typing.NotRequired")
        {
            return Ok((slice, true));
        }
        Ok((e, false))
    }

    fn attach(&mut self, module: &str, e: &Expr) -> Result<Json, String> {
        let ExprKind::Dict(items) = &e.kind else {
            return Err(self.error(module, e, "attach 须写成名字到 Port spec 的字面量 dict"));
        };
        let (mut properties, mut required) = (Map::new(), vec![]);
        for (key, value) in items {
            let key = key
                .as_ref()
                .ok_or_else(|| self.error(module, e, "attach 须写明名字"))?;
            let name = self.text(module, key)?;
            let (value, optional) = self.optional(module, value)?;
            if !matches!(value.kind, ExprKind::Name(_) | ExprKind::Call { .. }) {
                return Err(self.error(module, value, unsupported("attach 的 Port spec 写法")));
            }
            let Type::Port(spec) = self.typ(module, value)? else {
                return Err(self.error(module, value, "attach 的值须是 Port spec"));
            };
            if properties.insert(name.clone(), spec).is_some() {
                return Err(self.error(module, key, "attach 的名字重复"));
            }
            if !optional {
                required.push(name);
            }
        }
        Ok(json!({"properties": properties, "required": required, "additionalProperties": false}))
    }
}
