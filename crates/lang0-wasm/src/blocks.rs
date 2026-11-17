//! WASM 桥的 block：host 与 guest 之间的协议；规则见 `croon_eval::blocks`。
//! 参考：docs/Engineering.md。

use croon_eval::blocks::{Block, chain, expanded, find};

pub static BLOCKS: &[Block] = croon_eval::block_files!["lang0.wasm",];

/// 本 crate 的 block，找不到的去它依赖的 crate。
pub fn block(name: &str) -> Option<&'static Block> {
    chain(find(BLOCKS, name), || croon_eval::blocks::block(name))
}

/// 一份 block，`$block` 已展开。
pub fn json(name: &str) -> serde_json::Value {
    expanded(name, &block)
}
