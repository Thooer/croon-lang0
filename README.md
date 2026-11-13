# croon-lang0

`croon-lang0` 实现 `lang0@1`，是 Croon 的第一种 Language。它提供一种接近 Python 的 Surface，把程序降为 Kernel 能驱动的交出与取，并在自己的求值器中固定语义、键和资源计量。

## 内容

- `crates/lang0/`：Lang0 的解析、构造检查、类型推导与求值器。
- `crates/lang0-wasm/`：WASM 装载与隔离桥；guest 是独立的 WASM workspace。
- `docs/`：Surface、首版子集、求值语义、资源、异常、降法、隔离和场景。

## 状态

`lang0@1` 的首版子集与语义正在冻结。CPython 差分、顺序求值、资源计量、WASM 隔离和恢复是当前实现与验证重点。已经发布的 Language ID 语义只增加，不删除或改写；需要改变既有 Definition 含义时创建新的 Language 版本。

## 开发

本仓库是独立 Cargo workspace。构建与测试只使用本仓库的 crates 和依赖。依赖并排放置的 croon 仓库（path 依赖），目录布局为 `../croon`。WASM bridge 测试前需要先构建 guest，具体命令写在 `docs/Engineering.md`。

文档入口是 `docs/README.md`。写法、类型、Port 与交出在 `docs/README.md`，首版清单在 `docs/Subset.md`，语义在 `docs/Evaluator.md`，实现与隔离在 `docs/Engineering.md`。
