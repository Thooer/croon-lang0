//! Schema 与 Port spec 的字面量读取：不执行调用或模块代码。
//! 参考：docs/Lang0/README.md（Interface、Port 与交出）；docs/Kernel/Value.md（内容寻址）。

use super::*;

impl Infer<'_> {
    pub(super) fn text(&self, module: &str, e: &Expr) -> Result<String, String> {
        e.string()
            .map(str::to_owned)
            .ok_or_else(|| self.error(module, e, "须是字符串字面量"))
    }

    pub(super) fn literal(&self, module: &str, e: &Expr) -> Result<Json, String> {
        let value = match &e.kind {
            ExprKind::Constant(Constant::None) => Json::Null,
            ExprKind::Constant(Constant::Bool(b)) => json!(b),
            ExprKind::Constant(Constant::Str(s)) => json!(s),
            ExprKind::Constant(Constant::Int(n)) => {
                serde_json::from_str(n).map_err(|_| self.error(module, e, "整数不是 Value"))?
            }
            ExprKind::Constant(Constant::Float(n)) => serde_json::Number::from_f64(*n)
                .map(Json::Number)
                .ok_or_else(|| self.error(module, e, "非有限数不是 Value"))?,
            ExprKind::UnaryOp { op, operand } if op == "USub" => match &operand.kind {
                ExprKind::Constant(Constant::Int(n)) => serde_json::from_str(&format!("-{n}"))
                    .map_err(|_| self.error(module, e, "整数不是 Value"))?,
                ExprKind::Constant(Constant::Float(n)) => serde_json::Number::from_f64(-n)
                    .map(Json::Number)
                    .ok_or_else(|| self.error(module, e, "非有限数不是 Value"))?,
                _ => return Err(self.error(module, e, "须是 Value 的字面量")),
            },
            ExprKind::List(es) => Json::Array(
                es.iter()
                    .map(|e| self.literal(module, e))
                    .collect::<Result<_, _>>()?,
            ),
            ExprKind::Dict(items) => {
                let mut out = Map::new();
                for (key, value) in items {
                    let key = key
                        .as_ref()
                        .ok_or_else(|| self.error(module, e, "须是完整的字面量，不收解包"))?;
                    if out
                        .insert(self.text(module, key)?, self.literal(module, value)?)
                        .is_some()
                    {
                        return Err(self.error(module, key, "Schema 或 Port spec 的字面量键重复"));
                    }
                }
                Json::Object(out)
            }
            _ => return Err(self.error(module, e, "须是 Value 的字面量")),
        };
        croon_eval::value::canonical(&value)
            .map_err(|err| self.error(module, e, err.to_string()))?;
        Ok(value)
    }
}
