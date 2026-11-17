//! import 只经 Read 展开模块目录；宿主文件系统不参与名字解析。
//! 参考：docs/README.md（模块）；croon:docs/Kernel/Definition/Cognition.md「body 的边界」。

use crate::{
    ast::*,
    parser,
    symbols::{self, Binding, Imports, Scope},
};
use croon_eval::host::{CheckError, HostError, Read};
use serde_json::Value as Json;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

/// 清单的一组里有没有这个名字：组是名字的数组，或以名字为键、值是调用形式的对象。
pub fn listed(group: &Json, name: &str) -> bool {
    match group {
        Json::Array(names) => names.iter().any(|v| v.as_str() == Some(name)),
        Json::Object(forms) => forms.contains_key(name),
        _ => false,
    }
}

pub struct SourceModule {
    pub path: String,
    pub source: String,
    pub ast: Module,
    pub package: bool,
    pub globals: Scope,
    pub imports: Imports,
}

impl SourceModule {
    pub fn error(&self, span: &Span, message: impl AsRef<str>) -> String {
        diagnostic(&self.path, &self.source, span, message)
    }
}

pub struct Program {
    pub modules: BTreeMap<String, SourceModule>,
    pub subset: Json,
}

#[derive(Clone)]
pub enum Resolved {
    Builtin(String),
    Module(String),
    Binding { module: String, binding: Binding },
}

impl Program {
    pub fn builtin_module(&self, name: &str) -> bool {
        self.subset["modules"].get(name).is_some()
    }

    pub fn resolve(&self, module: &str, name: &str) -> Result<Resolved, String> {
        self.resolve_inner(module, name, &mut BTreeSet::new())
    }

    fn resolve_inner(
        &self,
        module: &str,
        name: &str,
        seen: &mut BTreeSet<(String, String)>,
    ) -> Result<Resolved, String> {
        if self.builtin_module(module) {
            if self.subset["modules"][module]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str() == Some(name))
            {
                return Ok(Resolved::Builtin(format!("{module}.{name}")));
            }
            return Err(unsupported(&format!("{module}.{name}")));
        }
        if !seen.insert((module.into(), name.into())) {
            return Err(format!("标注绑定成环：{module}.{name}"));
        }
        let m = self
            .modules
            .get(module)
            .ok_or_else(|| format!("模块未读到：{module}"))?;
        if let Some(bindings) = m.globals.get(name) {
            if bindings.len() != 1 {
                return Err(format!("标注的名字 {name} 须是模块顶层唯一的一次绑定"));
            }
            match &bindings[0] {
                Binding::Module(module) => Ok(Resolved::Module(module.clone())),
                Binding::From { module, name } => self.resolve_inner(module, name, seen),
                binding => Ok(Resolved::Binding {
                    module: module.into(),
                    binding: binding.clone(),
                }),
            }
        } else if listed(&self.subset["builtins"], name) || listed(&self.subset["exceptions"], name)
        {
            Ok(Resolved::Builtin(name.into()))
        } else {
            Err(format!("名字 {name} 没有绑定"))
        }
    }

    pub fn resolve_expr(&self, module: &str, expr: &Expr) -> Result<Resolved, String> {
        match &expr.kind {
            ExprKind::Name(name) => self.resolve(module, name),
            ExprKind::Attribute { value, attr } => {
                let Resolved::Module(m) = self.resolve_expr(module, value)? else {
                    return Err("标注须引用模块顶层的绑定".into());
                };
                if !self.builtin_module(&m) && !self.modules[&m].globals.contains_key(attr) {
                    return Err(format!("模块 {m} 没有 {attr}"));
                }
                self.resolve(&m, attr)
            }
            _ => Err("标注须引用模块顶层的绑定".into()),
        }
    }
}

pub fn load(definition: &Json, read: &dyn Read) -> Result<Program, CheckError> {
    let code = CodeRead {
        read,
        fault: RefCell::new(None),
    };
    let result = load_code(definition, &code, None);
    match code.fault.into_inner() {
        Some(error) => Err(CheckError::Host(error)),
        None => result.map_err(CheckError::Rejected),
    }
}

/// 以空源码为正文，只装载模块目录里的一个模块（连同它 import 的），供推导公开面。
pub fn load_module(modules: &str, name: &str, read: &dyn Read) -> Result<Program, CheckError> {
    let code = CodeRead {
        read,
        fault: RefCell::new(None),
    };
    let definition = serde_json::json!({"body": {"source": "", "modules": modules}});
    let result = load_code(&definition, &code, Some(name));
    match code.fault.into_inner() {
        Some(error) => Err(CheckError::Host(error)),
        None => result.map_err(CheckError::Rejected),
    }
}

/// 模块目录里所有模块的点分名字，按字典序：`x.py` 是 x，包目录里的 `__init__.py` 是包名，目录里的 `y.py` 是 `目录.y`。
pub fn module_names(modules: &str, read: &dyn Read) -> Result<Vec<String>, CheckError> {
    let code = CodeRead {
        read,
        fault: RefCell::new(None),
    };
    let mut out = vec![];
    let result = walk(&code, modules, "", &mut out);
    match code.fault.into_inner() {
        Some(error) => Err(CheckError::Host(error)),
        None => result.map(|()| out).map_err(CheckError::Rejected),
    }
}

fn walk(code: &CodeRead<'_>, id: &str, prefix: &str, out: &mut Vec<String>) -> Result<(), String> {
    let tree = code.directory(id)?;
    let entries = tree.as_object().ok_or("/body/modules: 须是 Tree")?;
    for (name, entry) in entries {
        if let Some(dir) = entry.get("dir").and_then(Json::as_str) {
            if dunder(name) || name.contains('.') {
                continue;
            }
            let path = format!("{prefix}{name}");
            walk(code, dir, &format!("{path}."), out)?;
        } else if let Some(stem) = name.strip_suffix(".py") {
            if stem == "__init__" {
                if !prefix.is_empty() {
                    out.push(prefix.trim_end_matches('.').to_owned());
                }
            } else if !dunder(stem) && !stem.contains('.') {
                out.push(format!("{prefix}{stem}"));
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(())
}

struct CodeRead<'a> {
    read: &'a dyn Read,
    fault: RefCell<Option<HostError>>,
}
impl CodeRead<'_> {
    fn take<T>(&self, result: Result<Option<T>, HostError>) -> Option<T> {
        match result {
            Ok(value) => value,
            Err(error) => {
                *self.fault.borrow_mut() = Some(error);
                None
            }
        }
    }
    fn json(&self, id: &str) -> Option<Json> {
        self.take(self.read.code_json(id))
    }
    fn bytes(&self, id: &str) -> Option<Vec<u8>> {
        self.take(self.read.code_bytes(id))
    }
    fn bundle(&self, id: &str) -> Option<croon_eval::store::Bundle> {
        self.take(self.read.code_bundle(id))
    }
    fn content(&self, id: &str) -> Result<String, String> {
        self.bundle(id)
            .map(|b| b.content)
            .ok_or_else(|| format!("读不到 Bundle {id}"))
    }
    fn directory(&self, id: &str) -> Result<Json, String> {
        let bundle = self
            .bundle(id)
            .ok_or_else(|| format!("读不到目录 Bundle {id}"))?;
        let content = self
            .json(&bundle.content)
            .ok_or_else(|| format!("读不到目录内容 {}", bundle.content))?;
        croon_tree::validate(&bundle, &content).map_err(|error| error.to_string())?;
        Ok(content)
    }
}

fn load_code(
    definition: &Json,
    read: &CodeRead<'_>,
    ensure: Option<&str>,
) -> Result<Program, String> {
    if croon_eval::schema::validate(&crate::blocks::json("lang0.body"), &definition["body"])
        .is_err()
    {
        return Err("/body: 须符合 lang0.body（source 与可选 modules）".into());
    }
    let source = definition["body"]["source"]
        .as_str()
        .ok_or("/body/source: Lang0 source 须是源码字符串")?;
    let subset = crate::blocks::json("lang0.subset");
    let root = match definition["body"].get("modules") {
        Some(id) => {
            let id = id.as_str().ok_or("/body/modules: 须是 Tree 的 ID")?;
            let tree = read.directory(id)?;
            let entries = tree.as_object().ok_or("/body/modules: 须是 Tree")?;
            for name in entries.keys() {
                let module = name.strip_suffix(".py").unwrap_or(name);
                if subset["modules"].get(module).is_some() {
                    return Err(format!(
                        "/body/modules/{name}: 改名；模块目录顶层不能占用 {module}"
                    ));
                }
            }
            tree
        }
        None => Json::Object(Default::default()),
    };
    let mut loader = Loader {
        read,
        root,
        program: Program {
            modules: BTreeMap::new(),
            subset,
        },
    };
    loader.add("", "/body".into(), source.into(), false)?;
    if let Some(name) = ensure {
        loader.ensure(name)?;
    }
    Ok(loader.program)
}

struct Loader<'a> {
    read: &'a CodeRead<'a>,
    root: Json,
    program: Program,
}

impl Loader<'_> {
    fn tree(&self, entry: &Json, path: &str) -> Result<Json, String> {
        if entry.get("link").is_some() {
            return Err(format!(
                "/body/modules/{path}: {}",
                unsupported("import 经符号链接")
            ));
        }
        let id = entry["dir"]
            .as_str()
            .ok_or_else(|| format!("/body/modules/{path}: 不是包目录"))?;
        let tree = self.read.directory(id)?;
        if !tree.is_object() {
            return Err(format!("/body/modules/{path}: 不是 Tree"));
        }
        Ok(tree)
    }

    fn file(&self, entry: &Json, path: &str) -> Result<String, String> {
        if entry.get("link").is_some() {
            return Err(format!(
                "/body/modules/{path}: {}",
                unsupported("import 经符号链接")
            ));
        }
        let id = entry["file"]
            .as_str()
            .ok_or_else(|| format!("/body/modules/{path}: 不是源码文件"))?;
        let content = self.read.content(id)?;
        let bundle = self.read.bundle(id).unwrap();
        if !bundle.dependencies.is_empty() {
            return Err(format!("/body/modules/{path}: 文件 Bundle 依赖须为空"));
        }
        let bytes = self
            .read
            .bytes(&content)
            .ok_or_else(|| format!("/body/modules/{path}: 读不到源码 {id}"))?;
        String::from_utf8(bytes).map_err(|_| format!("/body/modules/{path}: 源码须是 UTF-8"))
    }

    fn locate(&self, name: &str) -> Result<Option<(String, String, bool)>, String> {
        let parts: Vec<_> = name.split('.').collect();
        let mut tree = self.root.clone();
        for (i, part) in parts.iter().enumerate() {
            let path = parts[..=i].join("/");
            if i + 1 < parts.len() {
                let Some(entry) = tree.get(*part) else {
                    return Ok(None);
                };
                tree = self.tree(entry, &path)?;
            } else {
                let directory = match tree.get(*part) {
                    Some(entry) if entry.get("dir").is_some() || entry.get("link").is_some() => {
                        Some(self.tree(entry, &path)?)
                    }
                    _ => None,
                };
                if let Some(init) = directory.as_ref().and_then(|t| t.get("__init__.py")) {
                    let path = format!("{path}/__init__.py");
                    return Ok(Some((
                        format!("/body/modules/{path}"),
                        self.file(init, &path)?,
                        true,
                    )));
                }
                let filename = format!("{part}.py");
                if let Some(entry) = tree.get(&filename) {
                    let path = format!("{path}.py");
                    return Ok(Some((
                        format!("/body/modules/{path}"),
                        self.file(entry, &path)?,
                        false,
                    )));
                }
                if directory.is_some() {
                    return Ok(Some((
                        format!("/body/modules/{path}/"),
                        String::new(),
                        true,
                    )));
                }
            }
        }
        Ok(None)
    }

    fn ensure(&mut self, name: &str) -> Result<(), String> {
        if self.program.builtin_module(name) || self.program.modules.contains_key(name) {
            return Ok(());
        }
        if let Some((parent, _)) = name.rsplit_once('.') {
            self.ensure(parent)?;
            if self.program.builtin_module(parent) || !self.program.modules[parent].package {
                return Err(format!("{parent} 不是包"));
            }
            // 包的 __init__.py 可能已经 import 了这个子模块。
            if self.program.modules.contains_key(name) {
                return Ok(());
            }
        }
        let (path, source, package) = self.locate(name)?.ok_or_else(|| {
            if name == "time" {
                "用 receive 的 timeout 代替".into()
            } else {
                format!("模块 {name} 不在 croon、白名单或模块目录里")
            }
        })?;
        self.add(name, path, source, package)?;
        if let Some((parent, child)) = name.rsplit_once('.') {
            self.program
                .modules
                .get_mut(parent)
                .unwrap()
                .globals
                .insert(child.into(), vec![Binding::Module(name.into())]);
        }
        Ok(())
    }

    fn absolute(&self, current: &str, module: &str, level: usize) -> Result<String, String> {
        if level == 0 {
            return Ok(module.into());
        }
        let mut parts: Vec<_> = current.split('.').filter(|s| !s.is_empty()).collect();
        if !self.program.modules[current].package {
            parts.pop();
        }
        if level > parts.len() {
            return Err("相对 import 超出模块目录 的包".into());
        }
        parts.truncate(parts.len() + 1 - level);
        if !module.is_empty() {
            parts.push(module);
        }
        Ok(parts.join("."))
    }

    fn add(
        &mut self,
        name: &str,
        path: String,
        source: String,
        package: bool,
    ) -> Result<(), String> {
        let ast = parser::parse(&source, &path)?;
        let mut statements = vec![];
        symbols::import_statements(&ast.body, &mut statements);
        let globals = symbols::collect(&ast.body, &Imports::new(), true);
        self.program.modules.insert(
            name.into(),
            SourceModule {
                path,
                source,
                ast,
                package,
                globals,
                imports: Imports::new(),
            },
        );
        for stmt in statements {
            let result = self.import(name, &stmt);
            let bindings = result.map_err(|e| self.program.modules[name].error(&stmt.span, e))?;
            self.program
                .modules
                .get_mut(name)
                .unwrap()
                .imports
                .insert(stmt.span.start, bindings);
        }
        let m = self.program.modules.get_mut(name).unwrap();
        let children: Vec<_> = m
            .globals
            .iter()
            .filter(|(_, bs)| matches!(bs.as_slice(), [Binding::Module(_)]))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        m.globals = symbols::collect(&m.ast.body, &m.imports, true);
        for (k, v) in children {
            m.globals.entry(k).or_insert(v);
        }
        Ok(())
    }

    fn import(&mut self, current: &str, stmt: &Stmt) -> Result<Vec<(String, Binding)>, String> {
        let mut out = vec![];
        let names = match &stmt.kind {
            StmtKind::Import(names) => names,
            StmtKind::ImportFrom { module, names, .. } => {
                if module.split('.').any(dunder) {
                    return Err("首版不收 dunder 名字".into());
                }
                names
            }
            _ => unreachable!(),
        };
        for a in names {
            if a.name == "*" {
                return Err(unsupported("import *"));
            }
            if a.name.split('.').any(dunder) || a.asname.as_deref().is_some_and(dunder) {
                return Err("首版不收 dunder 名字".into());
            }
        }
        match &stmt.kind {
            StmtKind::Import(names) => {
                for a in names {
                    self.ensure(&a.name)?;
                    let root = a.name.split('.').next().unwrap();
                    out.push((
                        a.asname.as_deref().unwrap_or(root).into(),
                        Binding::Module(if a.asname.is_some() {
                            a.name.clone()
                        } else {
                            root.into()
                        }),
                    ));
                }
            }
            StmtKind::ImportFrom {
                module,
                level,
                names,
            } => {
                let module = self.absolute(current, module, *level)?;
                self.ensure(&module)?;
                for a in names {
                    let binding = if self.program.builtin_module(&module) {
                        self.program.resolve(&module, &a.name)?;
                        Binding::From {
                            module: module.clone(),
                            name: a.name.clone(),
                        }
                    } else if self.program.modules[&module].globals.contains_key(&a.name) {
                        Binding::From {
                            module: module.clone(),
                            name: a.name.clone(),
                        }
                    } else {
                        let child = format!("{module}.{}", a.name);
                        if !self.program.modules[&module].package {
                            return Err(format!("模块 {module} 没有 {}", a.name));
                        }
                        self.ensure(&child)?;
                        Binding::Module(child)
                    };
                    out.push((a.asname.as_deref().unwrap_or(&a.name).into(), binding));
                }
            }
            _ => unreachable!(),
        }
        Ok(out)
    }
}
