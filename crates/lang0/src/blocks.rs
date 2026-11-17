//! Lang0 的 block：body、首版子集与消息；规则见 `croon_eval::blocks`。
//! 参考：docs/README.md（模块）；docs/Subset.md。

use croon_eval::blocks::{Block, expanded, find};

pub static BLOCKS: &[Block] = &[
    Block {
        name: "lang0.body",
        lang: "json",
        text: r#"{
  "type": "object",
  "properties": {
    "source": {"type": "string"},
    "modules": {"type": "string", "description": "固定模块目录 Bundle ID"}
  },
  "required": ["source"],
  "additionalProperties": false
}"#,
        file: "crates/lang0/src/blocks.rs",
        line: 0,
    },
    Block {
        name: "lang0.messages",
        lang: "json",
        text: include_str!("../schema/lang0.messages.json"),
        file: "crates/lang0/schema/lang0.messages.json",
        line: 0,
    },
    Block {
        name: "lang0.subset",
        lang: "json",
        text: include_str!("../schema/lang0.subset.json"),
        file: "crates/lang0/schema/lang0.subset.json",
        line: 0,
    },
];

/// 本 crate 的 block，找不到的去它依赖的 crate。
pub fn block(name: &str) -> Option<&'static Block> {
    find(BLOCKS, name).or_else(|| croon_eval::blocks::block(name))
}

/// 一份 block，`$block` 已展开。
pub fn json(name: &str) -> serde_json::Value {
    expanded(name, &block)
}
