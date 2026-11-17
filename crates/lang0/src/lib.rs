//! Lang0：构造检查与确定的顺序求值。
//!
//! 宿主要知道的推导入口，规则相同（标注、docstring 与其中的 `Args:`）：
//!
//! - [`interface`]：从 Definition 的 body 推出 `main` 的 Interface。
//! - [`modules`] 与 [`surface`]：枚举模块目录里的模块，推出一个模块的公开面（block `lang0.surface`），
//!   供宿主做包的索引。模块写错时报诊断；单个声明推不出只在那一项写 `error`。公开面随 `lang0@1` 冻结。
//!
//! 参考：docs/README.md（模块、Interface）；docs/Evaluator.md（构造时检查）；croon:docs/Kernel/Language.md「插口」。

mod ast;
mod blocks;
mod docstring;
mod eval;
mod infer;
mod modules;
mod parser;
mod subset;
mod symbols;

pub mod library;

/// 一份 block，`$block` 已展开：本 crate 的与上游的。
pub fn blocks_json(name: &str) -> Json {
    blocks::json(name)
}

/// 本 crate 的 block，找不到的去它依赖的 crate；下游的 block 要 `$block` 引用 `lang0.surface` 时接进自己的链。
pub fn block(name: &str) -> Option<&'static croon_eval::blocks::Block> {
    blocks::block(name)
}

use croon_eval::host::{CheckError, HostError, Language, Read, Session};
use serde_json::Value as Json;
use std::collections::BTreeMap;

/// `lang0@1` 的宿主插口。
pub struct Lang0;

/// 从 Definition 的 body 推出 Interface：与 `check` 走同一条装载与子集检查，只是不与写明的 interface 比较。
/// 宿主拼 Definition 时用它；构造时仍由 `check` 核对两者按规范文本相等。
pub fn interface(definition: &Json, read: &dyn Read) -> Result<Json, CheckError> {
    let program = modules::load(definition, read)?;
    subset::check(&program)?;
    Ok(infer::interface(&program)?)
}

/// 模块目录里所有模块的点分名字，按字典序；`modules` 是模块目录的 Tree ID，要在 `read` 读得到的范围里。
pub fn modules(modules: &str, read: &dyn Read) -> Result<Vec<String>, CheckError> {
    modules::module_names(modules, read)
}

/// 模块目录里一个模块的公开面，形状是 block `lang0.surface`：顶层不以下划线开头的函数、TypedDict、
/// Port 与 Schema，推导规则与 `interface`、`croon.tool` 相同。装载与子集检查同样走一遍，写错的模块报诊断；
/// 单个声明推不出时只在那一项写 `error`。
pub fn surface(modules: &str, name: &str, read: &dyn Read) -> Result<Json, CheckError> {
    let program = modules::load_module(modules, name, read)?;
    subset::check(&program).map_err(CheckError::Rejected)?;
    Ok(infer::surface(&program, name))
}

impl Language for Lang0 {
    fn id(&self) -> &str {
        "lang0@1"
    }

    fn check(&self, definition: &Json, read: &dyn Read) -> Result<(), CheckError> {
        let program = modules::load(definition, read)?;
        subset::check(&program)?;
        let inferred = infer::interface(&program)?;
        let declared = definition
            .get("interface")
            .ok_or("/interface: 缺少 Interface")?;
        let canonical =
            |v: &Json| croon_eval::value::canonical(v).map_err(|e| format!("/interface: {e}"));
        if canonical(&inferred)? != canonical(declared)? {
            return Err(program.modules[""]
                .error(
                    &infer::main_span(&program),
                    "/interface: 与 main 推出的 Interface 按规范文本不相等",
                )
                .into());
        }
        eval::Vm::new(program, None, BTreeMap::new())
            .check_top(read)
            .map_err(Into::into)
    }

    fn open(
        &self,
        _instance: &str,
        birth: &Json,
        results: &BTreeMap<String, Json>,
        read: &dyn Read,
    ) -> Result<Box<dyn Session>, HostError> {
        let program = modules::load(&birth["definition"], read).map_err(|error| match error {
            CheckError::Host(error) => error,
            CheckError::Rejected(message) => HostError(message),
        })?;
        Ok(Box::new(eval::Vm::new(
            program,
            Some(birth.clone()),
            results.clone(),
        )))
    }
}
