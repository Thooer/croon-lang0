# Evaluator

Lang0 的求值器：一个程序怎样运行、每一步花多少、错在哪里。写什么在 README.md；求值器本身的实现（降与隔离）在 Engineering.md。

## 引擎

`lang0@1` 冻结的是本文列出的构造子集及其语义。子集内的语义与 Python 相同；子集与不同之处列在 README.md 的「各构造」「拦什么」，引擎带来的差异列在本文；做不到相同的构造不进子集。列表之外的构造在构造时报错，说出用什么代替。Python 是写法的锚，也是差分测试的参照：子集内的程序在两边得到同样的结果，不同就是求值器的错，或要列进差异；语义承诺只到子集为止。子集或语义要变，就是新的 Language 版本，旧版本留着，还有 Instance 在用就不删；求值器修正成与文档一致，不算改语义。

冻结子集，不冻结某个现成引擎的构建：行为都在列表里，看得见；不会有没人列出过的行为，如异常的消息文本、Unicode 表、str 的 hash、libm，随构建一起成为冻结的语义。

判据一条：**程序的行为不能取决于宿主、Lang0 自己的实现（降法与运行时）或调度。** 所以同样的程序在不同时间、不同机器上给出同样的结果。

由此三句：

- 冻结的是规则，不是降法与隔离。规则按 Surface 的结构与运算写；降法换了实现，规则不变。
- 异常的类名用 Python 的，消息由 Lang0 给出，文本不承诺与 CPython 一致：这是列出的差异之一。
- 各条隔离路线要给出同样的行为。做不到的路线不跑真正的 Instance，只用来调试，见 Engineering.md 的「隔离」。

## 求值

- 求值一直活着。交出与取报出 Atom 后阻塞，等 Kernel 送回结果再返回，`main` 返回的 deliver 或普通值映射为带依赖的 done。
- 恢复时从头调用 `main`：有记录的 Atom，builtin 直接返回记录里的结果，不再报出；到第一个没有记录的，就接着运行。代价随这一生的 Atom 数线性增长，只在恢复时付。
- Instance 结束或换代际时，隔离单元被杀掉：finally 不运行，与进程被杀时一样；恢复后重放到那里时照常运行。except 接不住这次结束，也接不住重放分歧：两者都不是 body 的失败，结束不由 body 造成，分歧说明求值不确定，接住了只会在不确定的状态上接着走。

约束：

- **失败默认向外传播**，直到被接住；没有接住，Cognizer 以失败结束。不返回 Result：没检查的 Result 会当作普通数据流下去，校验拦不住。
- **重试与补偿在 Library**：见 croon-app:docs/Library/README.md「原则」；重试就是循环里 try，每次尝试是新的 Atom，有自己的键。
- **类型在边界上于运行时检查**：实例化的输入与结果按 Signature 查，工具的参数与返回值按 `croon.tool` 推出的 Schema 查，交出按派生链上每一节查，都由 value.schema 的校验器查。LLM 产出的数据结构运行时才确定，静态推断收益小。
- **Definition 的结构写 Definition 时就确定**：没有按数据调用 Definition 的写法。运行时的决定，包括 LLM 选哪个工具、跑刚造出来的东西，都是数据，交给某个 Port。灵活性的上限由 Definition 的声明与接线决定，构造与实例化时看得见；上限之内实际拿到了什么，记在 Journal 里。

## 并发

首版不收，理由见 README.md 的待定「首版子集的清单」；本节是收了之后的语义。

Surface 里直接写 `concurrent.futures.ProcessPoolExecutor`，不另造并行的 builtin。它的语义本来就是要的那一套：参数 pickle 过去，函数只能是顶层的，with 退出时等全部分支，正在跑的分支取消不了。

- **接受**：`with ProcessPoolExecutor(...) as ex:`、`ex.submit(f, *args)`、`ex.map(f, xs)`、`fut.result()`、`fut.exception()`，都不带 timeout。
- **拦下**：as_completed、`wait(FIRST_COMPLETED)`、done、running、cancel、add_done_callback、带 timeout 的 result 与 exception、`shutdown(cancel_futures=True)`，以及不放在 with 里的 executor：分支不能比发出它的地方活得长。ThreadPoolExecutor 也拦，报错说用 ProcessPoolExecutor，因为线程之间共享内存。
- **分支拿到什么**：构造时的模块，加上 pickle 过去的参数，即 spawn 的语义。引用值原样传：分支与发出它的一方是同一个 Instance、同一份 c-list。这与 Python 的默认启动方式无关。
- **submit 立即返回**：分支运行的同时，发出它的一方可以接着做事、发出 Atom。父分支只在 `result()` 处等，等的是一个确定的分支，不读完成的先后，所以结果与调度无关。这比「等全部结束的 builtin」能表达的多，又没有多出不确定性。
- **失败按 future 分开**：`result()` 抛出那个分支的异常；没调 `result()` 的失败被丢掉，与 Python 一样，是程序自己的选择。
- **max_workers**：用它与 W 中较小的那个，不影响结果与键。W 随 `lang0@1` 定下，几方互相等的程序才有可复现的行为。

取消不了分支，所以赛马对的是 Agent：选先通过检验的，暂停其余的。

## 键

键是分支的路径加分支里的序号：

- **分支**：最外层是一个分支，路径为空；submit 开出的分支，路径是父分支的路径加这次 submit 的序号。
- **序号**按 Python 的求值顺序编，每个分支从 0 起：每个 Atom、每次 submit 各占一个。循环、推导式、生成器、函数调用都不另起计数，所以 `[delegate(t) for t in tasks]` 与逐项 append 的 for 得到同样的键。
- **写法**：路径上的序号与自己的序号用 `/` 连起来，十进制，如 `3`、`2/0`。同一份 Journal 换了求值器的实现重放，键要逐字相同。

一个分支顺序执行：它发出的 Atom 与序号只取决于它的参数与它收到的结果；父分支只在 `result()` 处等一个确定的分支，不读完成的先后。所以键与调度无关，一个分支往前走，不会挪动兄弟分支的键。

Checkpoint 连同每个分支的计数器一起存下：计数器是 Language 状态的一部分，从 Checkpoint 接着走，之后的键与没有中断时相同。所以不必每一轮开一个作用域，让后面轮次的键不依赖前面的。

首版不收 concurrent.futures，键就是最外层的一个计数器。分支的规则现在随 `lang0@1` 一起定下，以后收 concurrent.futures，顺序程序的键不变，是增量。

序号就是效果的先后，所以求值顺序以 Python 的规定为准，由降法保证，见 Engineering.md 的「降」。没有采用全局的序号：一个分支有了结果、发出下一个 Atom，会占掉兄弟分支原来的序号。键里也不放源码位置：Definition 不可变，键只需要在同一份 Definition 的重放里不变；按帧折叠显示要的源码位置，由重放算出。

## 资源

无论 Definition 怎样写，求值都以 done 或确定的失败结束，见 croon:docs/Kernel/Definition/Cognition.md「求值契约」。Lang0 的度量都按 Surface 的运算定，与降法、引擎、一轮在哪里运行无关：

| 资源 | 度量 | 超出 |
|---|---|---|
| 计算 | 步数：每一轮、每次调用计 1；运算、builtin、方法按读入与产出的数据量计，产出先算好大小、计入步数再分配 | 失败 |
| 深度 | 调用层数，模块的顶层求值也算一层，穿过轮次与并发的分支照样累计；深遍历（repr、比较、格式化、`in`、hash、交出时的编码）每进一层也算一层 | 失败 |
| 活数据 | 一个分支够得到的数据量：各层的局部变量、排队的分支的参数、运行时对象与模块能到达的，减去构造结束时模块能到达的量 | 失败 |

数据量的单位：字符串按字节，list 与 dict 按元素个数，整数按字长。

- 步数每个分支各数各的，最外层也是一个分支；并发的分支从 0 起，父分支等待时不计；这个分支发出 Atom 时清零，上限是 B。交出时 Value 的编码计入步数，结果的解码不计，所以 B 大于 Value 的大小上限。
- 活数据在产出的数据量达到上次量得的总量、且不少于一个下限时量一次，上限是 L。产出本来就在步数里，所以量的时刻是确定的；每次量的代价不超过之前的产出，摊下来是常数。
- 可变的值（list、dict、闭包捕获的变量、运行时对象）不论几处引用只算一次；不可变的值（字符串、数、tuple）在每处出现各算一次：它们的同一性在 Lang0 里看不见（`is` 不收），只算一次就要取决于实现是否共用。这偏严，以后少计是增量。
- 与在哪里运行有关的开销都不计，如一轮换到新模块时的拷贝、量活数据时的遍历：计了，会不会失败就取决于实现。
- 超出上限是普通的失败，能接住。接住后也空转不了：计数到下一个 Atom 才清零，在那之前每一步都再失败。
- 构造时顶层的求值受同样的上限。

token、时间、花费的上限不在这里，在 croon-app:docs/Library/README.md「原则」：Language 不读时间，两个 Atom 之间也没有别的东西花钱。

## 文本

repr、str 与 f-string 的文本会流进交出的值，如 ReAct 交还模型的工具结果，重放时要逐字相同，所以照 CPython 3.12 冻结，不取决于宿主：

- None、True、False 照字面；int 是十进制；float 是能还原同一个 double 的最短十进制，指数小于 -4 或不小于 16 时写成 `1e+16`、`1e-05` 的形式，否则是定点、至少带一位小数，如 `1.0`、`1000000000000000.0`；`-0.0` 照写；非有限的是 `inf`、`-inf`、`nan`，它们只在求值器里，进不了 Value。这几样的 str 与 repr 相同。
- 字符串的 repr：含单引号而不含双引号时用双引号，否则用单引号；转义 `\\`、所用的引号、`\n`、`\r`、`\t`，其余不可打印的字符按码位写成 `\xhh`、`\uhhhh` 或 `\Uhhhhhhhh`。不可打印指 Unicode 15.0 里类别为 Cc、Cf、Cs、Co、Cn、Zl、Zp 的字符与空格以外的 Zs，与 CPython 3.12 相同，不随宿主的 Unicode 表变。`split`、`strip` 认的空白同样按 Unicode 15.0，是 CPython 3.12 的 `str.isspace()` 为真的字符。
- list、dict、tuple 的 repr：`[a, b]`、`{k: v}`（按插入顺序）、`(a,)`、`(a, b)`、`()`，元素用 repr；它们的 str 与 repr 相同。引用值与含引用值的运行时对象，repr 与 str 都失败，见 croon:docs/Kernel/Language.md「一套名字、两种值」的规则 1。
- 其余的 repr 照 CPython 3.12，去掉地址：`range(0, 3)`、`range(1, 5, 2)`；`dict_keys([1])`、`dict_values([2])`、`dict_items([(1, 2)])`；函数 `<function outer.<locals>.inner>`，用 qualname；builtin `<built-in function len>`，绑到值上的方法 `<built-in method append of list object>`，类上取的方法 `<method 'pop' of 'dict' objects>`；类 `<class 'int'>`、`<class 'croon.Failure'>`；异常 `ValueError('x')`、`ValueError()`、`ValueError('a', 1)`，即类名加 args 的 repr，只有一个时没有逗号；模块 `<module 'typing'>`；enumerate、zip 与别的对象 `<X object>`。
- 字符串转数照 CPython 3.12：去掉首尾的空白（str.isspace() 为真的字符，U+001C 到 U+001F 除外），可以有符号与数字之间的下划线，数字是 Unicode 15.0 里的十进制数字；float 还收不分大小写的 `inf`、`infinity`、`nan`。
- 异常的 str：没有 args 是空串；一个是它的 str，KeyError 是它的 repr；几个是 args 这个 tuple 的 repr。Lang0 自己抛出的异常的 args 见 Subset.md 的「异常的消息」。

## 构造时检查

构造检查包括静态检查与受资源上限约束的顶层求值；前者通过不代表后者通过。第二刀先验静态部分，再随求值器补齐顶层求值，不把中间结果当成完整构造验收。

- 源码能解析：按 Engineering.md 选定的目标语法版本解析，再按 Subset.md 拒绝未收的写法，报错说出用什么代替。
- 用到的名字都有绑定；import 的模块在 `croon`、白名单或模块目录 里，模块目录 里的按同一套规则检查，见 README.md 的「模块」。
- 有 `main`；它的参数与返回值都标了类型，没标的报错说值类型进 Signature、Port 类型是 Required port。
- 顶层没有引用值：写了 `exec = Exec()` 这样的，报错说引用值写在 `main` 的参数里，工具在外层函数里捕获它。
- 交给并发的写的是 lambda 或嵌套的 def 的，报错说用顶层 def，要固定的参数用 functools.partial 绑上。
- 顶层能求值一次，求值受与运行同一套资源上限。
- 错误带位置，说出违反了哪条规则。

## 待定

- 并发的键按现在的规则还没有验证：一个有嵌套并行、循环与函数调用的程序，跑遍它的全部完成顺序，发出的 Atom 应当相同。随收 concurrent.futures 一起做。没有场景：首版不收 concurrent.futures。
- B、深度上限、L、W 与活数据量的下限的数值，实测后定；croon-app:docs/Library/README.md「原则」中的 `max_turns` 默认值随它。场景：Delegate。

参考：Subset.md（异常的消息）；README.md（为什么是 Python 的写法、拦什么、各构造、模块、Interface、值与类型、Port 与交出）；Engineering.md（选择、降、隔离）。
