# 研究

这里收 Lang0 自己需要的研究线索，不是语义和实现的权威。`lang0@1` 的写法与语义以 `docs/README.md`、`docs/Subset.md` 和 `docs/Evaluator.md` 为准。

## 收什么

- `先例.md`：求值器、解析器、确定性、资源计量、checkpoint、并发与隔离的先例。
- `前提.md`：语法与格式那一条，即模型对没见过的语法要靠猜、借熟悉的写法更可靠。

## 不收

- 支持 Kernel 的保证与放置的线索，归 croon 的 `docs/Research/`。
- 关于 ReAct、上下文、委派、组织、监督、模型服务和外部协议的，归 croon-app 的 `docs/Research/`。

## 规则

研究材料只保存观察、出处和推断。Lang0 的冻结语义、资源规则、异常消息与隔离要求直接写在正文中，不在研究材料里另立一份规则。

参考：docs/README.md（模块）；docs/Evaluator.md（资源）；docs/Engineering.md（隔离）。
