//! Lang0 的 block：body、首版子集与消息；规则见 `croon_eval::blocks`。
//! 参考：docs/README.md（模块）；docs/Subset.md。

use croon_eval::blocks::{Block, chain, expanded, find};

pub static BLOCKS: &[Block] =
    croon_eval::block_files!["lang0.body", "lang0.messages", "lang0.subset", "lang0.surface",];

/// 本 crate 的 block，找不到的去它依赖的 crate。
pub fn block(name: &str) -> Option<&'static Block> {
    chain(find(BLOCKS, name), || croon_eval::blocks::block(name))
}

/// 一份 block，`$block` 已展开。
pub fn json(name: &str) -> serde_json::Value {
    expanded(name, &block)
}
