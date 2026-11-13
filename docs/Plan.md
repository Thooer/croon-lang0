# 开发规划

全文是打算，按待定读：设计以各部分为准，这里只说 Lang0 在哪个阶段、做完了什么、下一步做什么、怎样算做完。必要的代码验证可以穿插进行，不要求全部设计完成才开始实现。

## 现在

生命周期的 0。首刀与第二刀已完成，运行闭环还差真实的外部执行。已完成：

- **第二刀**：Lang0 顺序求值、CPython 差分、WASM 桥与新的显式交付场景通过；Delegate 在兼容顺序、提交点和每个 provider Atom 在途崩溃下 Journal 相同。Lang0 与 App 的集成验证在对应仓库维护。
- **CPython 差分**（5a29d4e）：子集程序的结束值与 Atom 先后通过 CPython 3.12 对照[test:crates/lang0/src/eval/tests.rs:cpython_differential]，已列出的差异单独检查[test:crates/lang0/src/eval/tests.rs:documented_cpython_differences]。`cargo test` 要有 python3.12；新增构造时补进 crates/lang0/cpython/differential.py。
- **Language 装载**：Lang0 的 body、模块目录与构造读取已改为完整 Definition Bundle 的固定闭包；WASM 桥仍按受控 Read 与宿主预算运行。
- **WASM 隔离**：Lang0 WASM 的受控读取、宿主预算、故障局部性与重开测试通过；运行 workspace 测试前先构建 guest，命令见 Engineering.md 的「隔离」。
- **验证收尾**：spike 已删，结论与重跑位置在各自正文；Lang0 隔离验证见 Engineering.md。

**尚未落地**：Language 的系统内构造不是本闭环的前提；正式对象形状、执行条件与终止语义留在 croon:docs/Kernel/Language.md「待定」，等独立的构造使用场景需要它时推进。

## 下一步

顺序是推断，前一步的结果会改变后一步。文档整理仍只做两类：现在能显著集中语义的，以及现在成本低、以后成本高的；其余随设计改写。Draft 保留在 context，可视化只在 Draft 里整理，不进正文。

1. **完成场景迁移**：让 Lang0/WASM 场景全部使用显式 Bundle、delivery、accepted 和新的 Definition body。
2. **模型与客户端**：接真实模型适配及经 socket 的 root entry，完成真实任务交付；与 Lang0 的实现可按契约并行。

## 第二刀

已完成；以下保留验收的范围与判据。

- **范围**：Kernel 场景只使用 fake Language 和显式 Bundle；Lang0、模型、文件执行与 Library 的 Delegate 集成在对应仓库维护。
- **判据**：源码通过构造检查，实际求值得到预期的交办、等待、结果交付与并入；Journal 与预期相符、满足四条不变量；在每个事务提交前后中断，恢复后得到与不中断时相同的 Journal。预期写成可取用的规格，不由求值器自己的输出充当预期。
- **CPython 差分的范围**：Lang0 仓库中的 CPython 绑定给出相同的假结果，对照结束值与 Atom 先后；覆盖文本、消息、dict 顺序、推导式的求值顺序与整除，已列出的语义差异单独检查。
- **暂不扩展**：并发、生成器、通用 class、with、Checkpoint、Directory 的一等纯函数操作，不因求值器以后可能需要而提前实现；类型声明所需的写法按首版清单定。资源按 Evaluator.md 的规则计量，验证用的上限只作试验参数，实测后再定冻结的数值，不把临时值当成 `lang0@1` 的发布语义。

## 测试

- **CPython 差分的范围**：Lang0 仓库中的 CPython 绑定给出相同的假结果，对照结束值与 Atom 先后；覆盖文本、消息、dict 顺序、推导式的求值顺序与整除，已列出的语义差异单独检查。
- **WASM 隔离**：Lang0 WASM 的受控读取、宿主预算、故障局部性与重开测试通过；同一个程序在原生与 WASM 路线上，Atom、结果与 Journal 必须相同。运行 workspace 测试前先构建 guest，命令见 Engineering.md 的「隔离」。
- **确定性**：假的录下结果再回放；重放的确定性用性质测试；对账者与恢复本身再崩溃一次，结果收敛。

## 以后的方向

场景需要时再纳入：

- LLM 表现量化：Lang0 的写法与检查行为可运行、任务及判据明确之后。同一场景写两版，先写不受限的 Python、再写 Lang0，每处差异标出归处，不能有悄悄改变语义又不报错的；之后测模型写 Lang0 的正确率、构造检查通过率与修复轮数。

参考：README.md（为什么是 Python 的写法、拦什么、各构造、模块、Interface、值与类型、Port 与交出）；Subset.md（清单、异常的消息）；Evaluator.md（引擎、求值、并发、键、资源、文本、构造时检查、待定）；Engineering.md（选择、降、隔离）。
