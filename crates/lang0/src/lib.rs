//! Lang0：构造检查与确定的顺序求值。
//! 参考：docs/Lang0/README.md（模块、Interface）；docs/Lang0/Evaluator.md（构造时检查）；docs/Kernel/Language.md（插口）。

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

use croon_eval::host::{CheckError, HostError, Language, Read, Session};
use serde_json::Value as Json;
use std::collections::BTreeMap;

/// `lang0@1` 的宿主插口。
pub struct Lang0;

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
