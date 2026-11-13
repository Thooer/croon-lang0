# 研究

这里收 Lang0 自己需要的研究线索，不是语义和实现的权威。`lang0@1` 的写法与语义以 `docs/README.md`、`docs/Subset.md` 和 `docs/Evaluator.md` 为准。

## 分诊

- `先例.md` 中关于 Python Surface、AST、解释器、IR、确定性求值、WASM、fuel、资源计量、checkpoint 和 continuation 的内容归这里。
- `前提.md` 中关于模型写法、结构化程度、错误反馈和语言选择，只有直接影响 Lang0 Surface 或求值语义的部分归这里。
- 其余关于 ReAct、上下文、委派、组织、监督、模型服务和外部协议的内容不归这里，归应用侧研究。
- 关于 ocap、Journal、Reference、制度和 Kernel 边界的内容不归这里，归 Kernel 侧研究。

## 规则

研究材料只保存观察、出处和推断。Lang0 的冻结语义、资源规则、异常消息与隔离要求直接写在正文中，不在研究材料里另立一份规则。