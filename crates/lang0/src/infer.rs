//! Interface 从 AST 推导，不对标注求值；名字只追到模块顶层唯一的绑定。
//! 参考：docs/README.md（Interface、值与类型、Port 与交出）。

mod constructors;
mod literal;

use crate::{
    ast::*,
    docstring,
    modules::{Program, Resolved},
    symbols::Binding,
};
use serde_json::{Map, Value as Json, json};
use std::collections::BTreeSet;

#[derive(Clone)]
enum Type {
    Schema(Json),
    Port(Json),
    Inbox(Json),
    Constructor(String),
}

struct Infer<'a> {
    program: &'a Program,
    visiting: BTreeSet<(String, usize, usize)>,
}

/// `def main` 那一行；没有 main 时是开头。
pub fn main_span(program: &Program) -> Span {
    program.modules[""]
        .ast
        .body
        .iter()
        .find(|s| matches!(&s.kind, StmtKind::FunctionDef(f) if f.name == "main"))
        .map_or(0..0, |s| s.span.clone())
}

pub fn interface(program: &Program) -> Result<Json, String> {
    let module = &program.modules[""];
    let main = module
        .ast
        .body
        .iter()
        .find(|s| matches!(&s.kind, StmtKind::FunctionDef(f) if f.name == "main"))
        .ok_or_else(|| {
            module.error(
                &(0..0),
                "须有顶层 def main；值类型进 Signature，Port 类型是 Required port",
            )
        })?;
    if module.globals.get("main").is_none_or(|b| b.len() != 1) {
        return Err(module.error(&main.span, "main 须是模块顶层唯一的函数定义"));
    }
    let main_span = main.span.clone();
    let StmtKind::FunctionDef(main) = &main.kind else {
        unreachable!()
    };
    let mut infer = Infer {
        program,
        visiting: BTreeSet::new(),
    };
    let (description, args) = docstring::extract(main);
    let (mut properties, mut provides, mut requires) = (Map::new(), Map::new(), Map::new());
    let mut required = vec![];
    for arg in &main.args {
        let annotation = arg.annotation.as_ref().ok_or_else(|| {
            module.error(
                &arg.span,
                "main 的参数须标类型；值类型进 Signature，Port 类型是 Required port",
            )
        })?;
        match infer.typ("", annotation)? {
            Type::Port(spec) => {
                requires.insert(arg.name.clone(), spec);
            }
            Type::Inbox(spec) => {
                provides.insert(arg.name.clone(), spec);
            }
            typ => {
                let mut schema = infer.value_schema("", annotation, typ)?;
                if let Some(doc) = args.get(&arg.name) {
                    schema["description"] = json!(doc);
                }
                properties.insert(arg.name.clone(), schema);
                if arg.default.is_none() {
                    required.push(arg.name.clone());
                }
            }
        }
    }
    let returns = main.returns.as_ref().ok_or_else(|| {
        module.error(
            &main_span,
            "main 的返回值须标类型；值类型进 Signature，Port 类型是 Required port",
        )
    })?;
    let output = infer.schema("", returns)?;
    let interface = json!({
        "description": description,
        "signature": { "in": object(properties, required), "out": output },
        "provides": provides, "requires": requires,
    });
    // 推出的 Port spec 与 Schema 也交给唯一的 Kernel 校验器。
    croon_eval::definition::check_shape(
        &json!({ "language": "lang0@1", "body": "", "interface": interface }),
    )
    .map_err(|es| {
        module.error(
            &returns.span,
            es.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("；"),
        )
    })?;
    Ok(interface)
}

/// 类型声明在构造时按 AST 解释；运行时只保存已经检查过的形状。
pub(crate) fn declaration(
    program: &Program,
    module: &str,
    expr: &Expr,
) -> Result<(bool, Json), String> {
    let mut infer = Infer {
        program,
        visiting: BTreeSet::new(),
    };
    match infer.typ(module, expr)? {
        Type::Port(spec) => Ok((true, spec)),
        Type::Schema(schema) => Ok((false, schema)),
        _ => Err(infer.error(module, expr, "不是值类型或 Port 声明")),
    }
}

/// 工具与 main 共用标注、docstring 和对象 Schema 的推导。
pub(crate) fn tool(
    program: &Program,
    module: &str,
    f: &FunctionDef,
) -> Result<(Json, Json), String> {
    let mut infer = Infer {
        program,
        visiting: BTreeSet::new(),
    };
    let (description, docs) = docstring::extract(f);
    let mut properties = Map::new();
    let mut required = vec![];
    for arg in &f.args {
        let annotation = arg
            .annotation
            .as_ref()
            .ok_or_else(|| format!("工具参数 {} 须标类型", arg.name))?;
        let mut schema = infer.schema(module, annotation)?;
        if let Some(doc) = docs.get(&arg.name) {
            schema["description"] = json!(doc);
        }
        properties.insert(arg.name.clone(), schema);
        if arg.default.is_none() {
            required.push(arg.name.clone());
        }
    }
    let returns = f.returns.as_ref().ok_or("工具返回值须标类型")?;
    Ok((
        json!({"name": f.name, "description": description, "input": object(properties, required)}),
        infer.schema(module, returns)?,
    ))
}

/// 一个模块的公开面：顶层不以下划线开头的函数、TypedDict、Port 与 Schema，形状是 block `lang0.surface`。
/// 函数写它的签名（按源码里的标注）、描述与各参数的说明；声明写推出的 Schema 或 Port spec。
/// 单项推不出时写 `error`，不让一项坏了整份索引。
pub fn surface(program: &Program, module: &str) -> Json {
    let m = &program.modules[module];
    let text = |e: &Expr| m.source.get(e.span.clone()).unwrap_or("").to_owned();
    let doc = match m.ast.body.first().map(|s| &s.kind) {
        Some(StmtKind::Expr(e)) => e.string().map(str::trim).unwrap_or("").to_owned(),
        _ => String::new(),
    };
    let mut items = vec![];
    for stmt in &m.ast.body {
        match &stmt.kind {
            StmtKind::FunctionDef(f) if !f.name.starts_with('_') => {
                let (description, docs) = docstring::extract(f);
                let params: Vec<Json> = f
                    .args
                    .iter()
                    .map(|a| {
                        let mut p = json!({"name": a.name});
                        if let Some(t) = &a.annotation {
                            p["type"] = json!(text(t));
                        }
                        if let Some(d) = &a.default {
                            p["default"] = json!(text(d));
                        }
                        if let Some(d) = docs.get(&a.name) {
                            p["description"] = json!(d);
                        }
                        p
                    })
                    .collect();
                let mut item = json!({"name": f.name, "kind": "function", "description": description, "params": params});
                if let Some(r) = &f.returns {
                    item["returns"] = json!(text(r));
                }
                items.push(item);
            }
            StmtKind::Assign { targets, value } if targets.len() == 1 => {
                let ExprKind::Name(name) = &targets[0].kind else {
                    continue;
                };
                if name.starts_with('_') {
                    continue;
                }
                let ExprKind::Call { func, .. } = &value.kind else {
                    continue;
                };
                let is = |n: &str| {
                    matches!(program.resolve_expr(module, func),
                        Ok(Resolved::Builtin(b)) if b == n)
                };
                if !(is("typing.TypedDict") || is("croon.Port") || is("croon.Schema")) {
                    continue;
                }
                let mut item = json!({"name": name});
                match declaration(program, module, value) {
                    Ok((true, spec)) => {
                        item["kind"] = json!("port");
                        item["spec"] = spec;
                    }
                    Ok((false, schema)) => {
                        item["kind"] = json!("schema");
                        item["spec"] = schema;
                    }
                    Err(error) => {
                        item["kind"] = json!(if is("croon.Port") { "port" } else { "schema" });
                        item["error"] = json!(error);
                    }
                }
                items.push(item);
            }
            _ => {}
        }
    }
    json!({"module": module, "description": doc, "items": items})
}

pub fn is_port(program: &Program, module: &str, expr: &Expr) -> bool {
    matches!(
        (Infer {
            program,
            visiting: BTreeSet::new()
        })
        .typ(module, expr),
        Ok(Type::Port(_))
    )
}

fn object(properties: Map<String, Json>, required: Vec<String>) -> Json {
    json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
}

impl Infer<'_> {
    fn error(&self, module: &str, e: &Expr, message: impl AsRef<str>) -> String {
        self.program.modules[module].error(&e.span, message)
    }
    fn schema(&mut self, module: &str, e: &Expr) -> Result<Json, String> {
        let typ = self.typ(module, e)?;
        self.value_schema(module, e, typ)
    }
    fn value_schema(&self, module: &str, e: &Expr, typ: Type) -> Result<Json, String> {
        match typ {
            Type::Schema(schema) => Ok(schema),
            Type::Constructor(name) if name == "list" || name == "dict" => {
                Err(self.error(module, e, "用 `dict[str, Any]` 或 TypedDict"))
            }
            Type::Constructor(name) if name == "typing.Callable" => Err(self.error(
                module,
                e,
                "Callable 推不出 Schema；在外层函数里捕获它，或用 partial 绑上",
            )),
            _ => Err(self.error(module, e, "推不出值的 Schema；用 TypedDict 或 Literal")),
        }
    }
    fn typ(&mut self, module: &str, e: &Expr) -> Result<Type, String> {
        let key = (module.to_owned(), e.span.start, e.span.end);
        if !self.visiting.insert(key.clone()) {
            return Err(self.error(module, e, "标注绑定成环"));
        }
        let result = self.inner(module, e);
        self.visiting.remove(&key);
        result
    }
    fn inner(&mut self, module: &str, e: &Expr) -> Result<Type, String> {
        use ExprKind as K;
        match &e.kind {
            K::Constant(Constant::None) => Ok(Type::Schema(json!({"type": "null"}))),
            K::Name(_) | K::Attribute { .. } => {
                match self
                    .program
                    .resolve_expr(module, e)
                    .map_err(|m| self.error(module, e, m))?
                {
                    Resolved::Builtin(name) => Ok(match name.as_str() {
                        "str" => Type::Schema(json!({"type": "string"})),
                        "int" => Type::Schema(json!({"type": "integer"})),
                        "float" => Type::Schema(json!({"type": "number"})),
                        "bool" => Type::Schema(json!({"type": "boolean"})),
                        "typing.Any" => Type::Schema(json!({})),
                        _ => Type::Constructor(name),
                    }),
                    Resolved::Binding {
                        module: m,
                        binding: Binding::Value(expr),
                    } => {
                        let ExprKind::Call { func, .. } = &expr.kind else {
                            return Err(self.error(module, e, unsupported("类型别名")));
                        };
                        let constructor = self.program.resolve_expr(&m, func);
                        if !matches!(constructor, Ok(Resolved::Builtin(ref n)) if matches!(n.as_str(), "typing.TypedDict" | "croon.Port" | "croon.Schema"))
                        {
                            return Err(self.error(module, e, unsupported("类型别名")));
                        }
                        self.typ(&m, &expr)
                    }
                    _ => Err(self.error(
                        module,
                        e,
                        "标注须是模块顶层唯一的一次赋值；用 TypedDict 或 Literal",
                    )),
                }
            }
            K::BinOp { op, .. } if op == "BitOr" => {
                let mut schemas = vec![];
                self.union(module, e, &mut schemas)?;
                Ok(Type::Schema(json!({"anyOf": schemas})))
            }
            K::Subscript { value, slice } => self.subscript(module, e, value, slice),
            K::Call {
                func,
                args,
                keywords,
            } => self.call(module, e, func, args, keywords),
            _ => Err(self.error(module, e, "推不出 Schema；用 TypedDict 或 Literal")),
        }
    }
    fn union(&mut self, module: &str, e: &Expr, out: &mut Vec<Json>) -> Result<(), String> {
        if let ExprKind::BinOp { left, op, right } = &e.kind
            && op == "BitOr"
        {
            self.union(module, left, out)?;
            self.union(module, right, out)?;
            return Ok(());
        }
        out.push(self.schema(module, e)?);
        Ok(())
    }
    fn head(&mut self, module: &str, e: &Expr) -> Result<String, String> {
        match self.typ(module, e)? {
            Type::Constructor(name) => Ok(name),
            _ => Err(self.error(module, e, "不是类型构造器；用 TypedDict 或 Literal")),
        }
    }
    fn subscript(
        &mut self,
        module: &str,
        e: &Expr,
        value: &Expr,
        slice: &Expr,
    ) -> Result<Type, String> {
        let args = match &slice.kind {
            ExprKind::Tuple(es) => es.as_slice(),
            _ => std::slice::from_ref(slice),
        };
        let name = self.head(module, value)?;
        let schema = match name.as_str() {
            "list" if args.len() == 1 => {
                json!({"type": "array", "items": self.schema(module, &args[0])?})
            }
            "dict" if args.len() == 2 => {
                if self.schema(module, &args[0])? != json!({"type": "string"}) {
                    return Err(self.error(
                        module,
                        &args[0],
                        "用 dict[str, T]；Value 的键是字符串",
                    ));
                }
                json!({"type": "object", "additionalProperties": self.schema(module, &args[1])?})
            }
            "typing.Literal" if !args.is_empty() => {
                let values: Result<Vec<_>, _> =
                    args.iter().map(|a| self.literal(module, a)).collect();
                json!({"enum": values?})
            }
            "typing.Annotated" if args.len() == 2 => {
                let mut schema = self.schema(module, &args[0])?;
                schema["description"] = json!(self.text(module, &args[1])?);
                schema
            }
            "typing.Callable" => return Ok(Type::Constructor(name)),
            _ => {
                return Err(self.error(module, e, "类型参数不合推导规则；用 TypedDict 或 Literal"));
            }
        };
        Ok(Type::Schema(schema))
    }
}
