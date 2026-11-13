# AGENTS

croon-lang0 仓库的规则。协作原则、认识、方法见仓库族根目录的 AGENTS.md。文中的「我」是用户，「你」是 AI agent。

croon-lang0 实现 lang0@1：第一种冻结语义的 Language。已发布的 ID 语义只增不删；写法与语义的权威在 docs/Lang0/。

## 位置

| 位置 | 收什么 |
|---|---|
| README.md | croon-lang0 是什么、定位、当前状态 |
| AGENTS.md | 本文 |
| docs/Lang0/README.md | 写法、拦什么、Interface、模块、Port 与交出 |
| docs/Lang0/Subset.md | 首版子集：构造、类型、表达式 |
| docs/Lang0/Evaluator.md | 语义、资源、键、文本 |
| docs/Lang0/Engineering.md | 隔离、装载、降；WASM 桥协议 |

- 置信度由位置决定：正文是已确认的，「待定」是未决的；没确认的留在讨论里，或写成待定问题。
- 写代码之前，先把机制写进文档，并让我看懂。
- lang0@1 的语义一旦冻结只增不删：改写法之前确认不破坏已有的 Definition。

## 文档与代码

### 唯一出处

- 一个事实只有一处权威。别处要用，引用它的名字，不复写。
- 形状以 `crates/*/schema/*.json` 为权威，代码用 `include_str!` 取用，不手写第二份。
- 跨仓库的形状（如 `definition.eval`）经 `croon_eval` crate 取用，不读 croon 的文档。
- 测试的预期能写成机器读得懂的规格的，写在文档里，代码读它；写不成的放在代码一侧。

### 载体

| 载体 | 承担什么 | 代码能否取用 |
|---|---|---|
| 表格 | 枚举、映射、清单 | 否 |
| JSON | schema、接口、注册表 | 能 |
| 散文 | 判断、理由、权衡 | 否 |

- 自己定义的 JSON 字段与枚举字符串用 snake_case；JSON Schema 关键字照标准。

### 检查

提交时自动检查，没通过就不准提交。检查由 croon/crates/spec/ 的 check-spec 执行（开发期 path 依赖）。

- **测试存在**：不变量后标 `[test:文件:用例]`，每一个都要在测试里找得到。
- **名字唯一**：fence block 的名字只出现一次。
- **参考可解析**：`参考：` 行的路径要解析得到。Rust 文件开头的 `//!` 模块注释也用这一格式，路径相对仓库根。
- **篇幅**：按 `file.limits` 判定。

```json file.limits
{
  "min": 15,
  "healthy": 250,
  "danger": 400
}
```

### 上下文上限

```json context.limits
{
  "window": 1000000,
  "ratio": 0.2,
  "tokenizer": "o200k_base"
}
```

```json context.usage
{
  "tokens": 0,
  "percent": 0,
  "over_healthy": {}
}
```

### 文件边界

- 在非耦合的位置拆分。每部分先写成一个文件，最小 `min` 行，健康值 `healthy` 行以内，超过 `danger` 行拆成文件夹。
- 正文不插链接，末尾标「参考：」：只列仓库内的路径，路径相对本文件。

### 历史

历史在 Git 里。文档直接改写，不留版本标记或废弃段落。关键决策克制地写为什么。
