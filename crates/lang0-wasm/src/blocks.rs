//! block 表：schema/ 下的 JSON 文件直接编进来，不经过 build.rs。
//! 参考：croon:docs/Kernel/Engineering.md「接口」。

pub use croon_eval::blocks::{Block, json_in};

pub static BLOCKS: &[Block] = &[
    Block { name: "lang0.wasm", lang: "json", text: include_str!("../schema/lang0.wasm.json"), file: "crates/lang0-wasm/schema/lang0.wasm.json", line: 0 },
];

pub fn json(name: &str) -> serde_json::Value {
    json_in(BLOCKS, name)
}
