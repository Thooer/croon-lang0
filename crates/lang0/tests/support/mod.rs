//! 构造检查的最小源码与内容库。
//! 参考：docs/README.md（模块、Interface）。

use croon_eval::{host::Read, value::ValueId};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Contents {
    pub json: BTreeMap<String, Json>,
    pub bytes: BTreeMap<String, Vec<u8>>,
}

impl Read for Contents {
    fn code_bytes(&self, id: &str) -> Result<Option<Vec<u8>>, croon_eval::host::HostError> {
        Ok(self.bytes.get(id).cloned())
    }
    fn code_json(&self, id: &str) -> Result<Option<Json>, croon_eval::host::HostError> {
        Ok(self.json.get(id).cloned())
    }
    fn code_bundle(
        &self,
        id: &str,
    ) -> Result<Option<croon_eval::store::Bundle>, croon_eval::host::HostError> {
        Ok(self.json.get(id).and_then(|v| {
            Some(croon_eval::store::Bundle {
                content: v["content"].as_str()?.into(),
                dependencies: serde_json::from_value(v["dependencies"].clone()).ok()?,
            })
        }))
    }
    fn bytes(
        &self,
        id: &str,
        _: &[String],
    ) -> Result<Option<Vec<u8>>, croon_eval::host::HostError> {
        self.code_bytes(id)
    }
    fn json(&self, id: &str, _: &[String]) -> Result<Option<Json>, croon_eval::host::HostError> {
        self.code_json(id)
    }
    fn bundle(
        &self,
        id: &str,
        _: &[String],
    ) -> Result<Option<croon_eval::store::Bundle>, croon_eval::host::HostError> {
        self.code_bundle(id)
    }
}

impl Contents {
    pub fn put_json(&mut self, v: Json) -> String {
        let id = ValueId::of_json(&v).unwrap().to_string();
        let dependencies = v
            .as_object()
            .map(|entries| {
                let mut ids: Vec<_> = entries
                    .values()
                    .filter_map(|entry| entry.get("file").or_else(|| entry.get("dir")))
                    .filter_map(Json::as_str)
                    .map(str::to_owned)
                    .collect();
                ids.sort();
                ids.dedup();
                ids
            })
            .unwrap_or_default();
        self.json.insert(id.clone(), v);
        let bundle = json!({"content":id,"dependencies":dependencies});
        let id = ValueId::of_json(&bundle).unwrap().to_string();
        self.json.insert(id.clone(), bundle);
        id
    }
    pub fn put_source(&mut self, source: &str) -> Json {
        let id = ValueId::of_bytes(source.as_bytes()).to_string();
        self.bytes.insert(id.clone(), source.as_bytes().to_vec());
        let bundle = json!({"content":id,"dependencies":[]});
        let id = ValueId::of_json(&bundle).unwrap().to_string();
        self.json.insert(id.clone(), bundle);
        json!({"file": id, "x": false})
    }
    pub fn code(&mut self, files: &[(&str, &str)]) -> String {
        let mut tree = serde_json::Map::new();
        for (name, source) in files {
            tree.insert((*name).into(), self.put_source(source));
        }
        self.put_json(Json::Object(tree))
    }
}

pub fn definition(body: &str) -> Json {
    json!({"language": "lang0@1", "body": {"source":body}, "interface": {
        "description": "", "signature": {
            "in": {"type": "object", "properties": {}, "required": [], "additionalProperties": false},
            "out": {"type": "null"}
        }, "provides": {}, "requires": {}
    }})
}
