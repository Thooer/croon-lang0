# Lang0 的实现

Lang0 的求值器、降与隔离。写法在 README.md，语义在 Evaluator.md；Kernel 一侧的 Language 插口见 croon:docs/Kernel/Language.md「实现」。这里的一切都不随 `lang0@1` 冻结：README.md「引擎」里冻结的子集与语义不变，就可以换。

## 选择

| 部分 | 选择 | 理由 |
|---|---|---|
| 引擎 | Lang0 自己的求值器 | 冻结的是子集与语义，不是别人的构建，见 Evaluator.md 的「引擎」；步数、活数据与键按 Surface 的运算定，要进值的表示与帧，由自己的求值器给出 |
| 状态 | 是数据：源码编译成一个小的栈式 IR，帧、续体与堆都是求值器自己的数据结构；Rust 写，每个 Instance 一份独立的堆 | 停在 Atom 上的 Instance 就是一个值：可以留在内存里，可以丢掉、有到达时再重放，以后可以存成 Checkpoint；步数、深度与活数据在自己的结构上量；并发的分支是另一条帧链。没有采用借宿主的栈（递归的树遍历）：最快写出来，但停在 Atom 上要靠一个挂起的线程，每个在取上等着的 Instance 占一个，Checkpoint 存不下，以后的生成器与分支要多个挂起的栈，换掉它就是重写求值器 |
| 解析 | `ruff_python_parser`，版本钉死，目标语法 3.12；经一层转换得到自己的 AST，节点名照 Python 的 `ast` | 语法就是 Python 的；类型、描述与规则 3 的分析都在 AST 上做，报错的位置也取自它。它解析得了 3.12 的 f-string（PEP 701），`rustpython-parser` 0.4.0 解析不了；两者都给出字节偏移，都解析得了四份草稿（fcd3b6a）。它自述是 Ruff 的内部组件、不承诺接口，所以只在转换层里用，升级时重跑 spike 的样例 |
| 类型 | 标注与 docstring 在 AST 上读，推出 Schema；运行时的检查调 Kernel 的 value.schema 校验器，不另写一份 | 只有一个校验器，Lang0 的类型就是 value.schema：如 integer 是值为整数的 number，`6 / 2` 得 `3.0`，满足 `int` |
| 隔离 | 每个 Instance 一个隔离单元，见「隔离」 | 求值器写错时，body 可能绕过语言层的限制 |

没有采用：

- 钉死一个 CPython 的构建：构建里没人列出过的行为，如异常的消息文本、Unicode 表、libm，会一起成为冻结的语义；确定性与隔离还要靠 WASI 或子进程另外兜住。
- 现成的类 Python 解释器，如 Monty、Ouros、RustPython：冻结要跟着它们的行为与版本。Python 的子集由我们自己列，与 Python 的差异归我们追，并逐条列出；这是有意的选择。
- 现成的 Starlark 实现：计步、挂起、回收都没有公开的入口，要为它补一批只因宿主是 Starlark 才需要的机制；没有 class、try、with、while，与模型从 Python 来的先验差得多。
- 把某种语言编进 WASM 当引擎：确定性来自我们规定的子集，不来自宿主语言的运行时，不需要这一层。WASM 承担隔离，不决定语言语义。

## 降

- **求值顺序**：按 Python 规定的顺序求值，可能发出 Atom 的子表达式也一样；短路照留，`a() and b()` 的 `b()` 只在 `a()` 为真时求值。键的序号就是效果的先后，所以顺序由求值器规定，不留给宿主。
- **取决于之前结果的读取**：业务读取按 Python 求值顺序执行。Vm 从空消费键集合开始，每次解码 send 或 receive 结果之前记入该 Atom 键；包含失败与空到达。Read 使用这份轨迹重建当前资格，不能把 results 中未来的记录当成已取得。代码装载使用独立的固定闭包。
- **分析在 AST 上**：规则 3 的上限与闭包捕获的引用值，在 Surface 的 AST 上算；降下去之后，引用值的流向看不清。
- **计数归 Lang0**：步数、深度、活数据由 Lang0 的求值器按 Surface 的运算计，不借宿主的计数：那些随版本变。理由见 Kernel 的 Engineering 文档的「选择」。
- **失败归 Lang0**：失败的文本由 Lang0 给出；Python 异常类的失败，类名用 Python 的。
- **set 与 repr**：按插入顺序的 set、不带地址的默认 repr 是求值器的语义，见 README.md 的「拦什么」。
- **测试**：同一个程序在不同的隔离路线上，发出的 Atom、结果与 Journal 必须相同。子集内的程序以 CPython 为参照：同样的输入，结果与 Atom 的先后相同；不同就是求值器的错，或要列进 README.md 的差异。

## 隔离

求值器写错时，body 可能绕过语言层的限制。Kernel 的保证不依赖它，见 croon:docs/Kernel/Definition/Cognition.md「求值契约」。隔离要满足三条：

1. 每个 Instance 一个隔离单元。共用的话，绕过限制的 body 读得到别人的内存。
2. 单元里没有 I/O、时钟与随机数，也碰不到内容库：Value 只经 Kernel 按「能读的 ID」拿到，否则存储就是后门。
3. 宿主一侧兜住逃过计数的计算与内存。

登记的 lang0@1 正式以 wasm32-unknown-unknown 模块运行：每个 Instance 一个单元，正常时保留 guest 会话状态，模块可以缓存。不开 WASI、线程、I/O、时钟与随机数；只导入受控读取。宿主只认检查与会话 JSON、受控读取、校验及故障，AST、IR、帧与堆留在 guest。校验器从同一源码编进 guest，Kernel 独立检查每次行动。指针、长度先验证范围与宿主消息上限，再分配、复制及解析；trap 后丢弃 Store。

宿主可调整每次调用的 fuel、内存、消息与时间保护，检查也受保护；它们与 Surface 的 limit 分开，不进入 Definition、ID 或 Journal。长前缀重放可能需要提高宿主额度，耗尽按宿主故障处理，调整后原 Instance 可以继续。guest 的栈容许 Lang0 的深度上限先到。故障传播、局部停止与显式重开以 Kernel 的 Language、Instance 与 Journal 文档为准。

构建命令是 `cargo build --offline --manifest-path crates/lang0-wasm/guest/Cargo.toml --target wasm32-unknown-unknown --release`；产物留在 guest 的 target，不进 Git。装配方显式提供模块路径与宿主保护，把装载对象放进 Config.languages；加载失败可修复后重试，不隐式读取环境变量或按工作目录找模块。保护覆盖实例化与调用，不覆盖固定模块的宿主编译和受控读取服务本身；模块字节与读取服务由可信装配方提供。桥测试用 `cargo test --offline -p lang0-wasm --test bridge`，显式使用上述构建产物。运行器在具体实现一侧，Kernel 不依赖 Lang0 或 wasmtime。

### 装载与调用

登记模块只导出 memory、alloc(i32) → i32、call(i32, i32) → i64；call 收一段 JSON 字节，返回值低 32 位是输出指针，高 32 位是长度。输入由 guest 接管并释放，输出由 guest 保留到下一次 call。只导入 kernel.read(kind, request_ptr, request_len, out_ptr, capacity) → i32：kind 为 0 读字节、1 读 JSON、2 读 Bundle 描述；request 是含 id 与 consumed_keys 的 JSON，代码装载的 consumed_keys 为 null；capacity 为 0 时询问长度，否则填充缓冲区。不可读返回 −1，桥或读取服务故障交回宿主。每次读取都使用当前调用的读取者，不跨调用保留资格。校验器与求值器同源码编入模块；Kernel 独立检查行动。

```json lang0.wasm
{
  "in": {
    "anyOf": [
      {"type":"object","properties":{"op":{"enum":["check"]},"definition":{}},"required":["op","definition"],"additionalProperties":false},
      {"type":"object","properties":{"op":{"enum":["open"]},"instance":{"type":"string"},"birth":{"$block":"definition.eval#/in/properties/birth"},"results":{"$block":"definition.eval#/in/properties/results"}},"required":["op","instance","birth","results"],"additionalProperties":false},
      {"type":"object","properties":{"op":{"enum":["resume"]},"results":{"$block":"definition.eval#/in/properties/results"}},"required":["op","results"],"additionalProperties":false}
    ]
  },
  "out": {
    "anyOf": [
      {"type":"object","properties":{"ok":{"enum":[true]}},"required":["ok"],"additionalProperties":false},
      {"type":"object","properties":{"ok":{"enum":[true]},"value":{"$block":"definition.eval#/out"}},"required":["ok","value"],"additionalProperties":false},
      {"type":"object","properties":{"ok":{"enum":[false]},"message":{"type":"string"}},"required":["ok","message"],"additionalProperties":false},
      {"type":"object","properties":{"error":{"type":"string"}},"required":["error"],"additionalProperties":false}
    ]
  }
}
```

check 的正常拒绝是 ok:false；open 的成功是仅有 ok:true；resume 成功的 value 是契约输出；error 或与操作不符的响应是宿主故障。模块可共享编译结果，单元与读取资格不共享；故障单元丢弃，修复后显式重开。

原生求值器继续供差分验证使用。同一个程序在原生与 WASM 路线上，Atom、结果与 Journal 必须相同；语言层的计量、失败文本与 croon-app:docs/Library/README.md「地图」所述生成源码保持原样。隔离路线的早期验证见 0acf027；本次接入不引入内容寻址 Language、Checkpoint 或新的 fuel 语义。

## 堆

- **什么进堆**：每个 Instance 一个堆。可变的、能成环的值（list、dict、闭包捕获的变量、运行时对象）放在堆的槽里，别处只存槽的下标；标量、字符串与 tuple 直接存。tuple 里可以有下标，环只经过槽。
- **量活数据就是回收**：在 Evaluator.md「资源」定的时刻，从根标记一遍。根是各帧的局部变量与求值栈、排队的分支的参数、运行时对象，以及模块。标到的按「数据量的单位」累加，每个槽只算一次，就是活数据量，超过 L 以 limit 失败；没标到的槽释放。模块也是根；构造结束时量得的量作为基线，从以后每次量得的总量里减去，见 Evaluator.md「资源」。
- **释放的时机不影响结果**：Lang0 没有 finalizer、weakref、`id()`，槽的下标不对外暴露，dict 的顺序按插入，不按下标。所以回收只影响内存，确定性只要求量的时刻确定，而它由步数定下；释放哪些槽、下标怎样复用是内部实现。
- 没有采用 `Rc` 加环检测：量活数据本来就要遍历一遍可达的值，计量与回收是同一次遍历，分开就是两套。也没有采用现成的 GC 库：回收的时机不归求值器控制，量不出确定的活数据量。

## 待定

- 在 receive 上等待的 Instance 一直留在内存里，还是被丢掉、有到达时再重放；Checkpoint 能缩短这次重放，Kernel 怎样记下、交还它见 croon:docs/Kernel/Journal.md「待定」。并发的分支的参数按大小深拷贝，成本没有测；大的东西是 Tree，走 ID。场景：W0 持续对话。

参考：README.md（拦什么、失败）；Evaluator.md（引擎、并发、键、资源）。
