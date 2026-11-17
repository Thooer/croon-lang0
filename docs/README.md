# Lang0

Lang0 是第一种 Language，ID 为 `lang0@1`。body.source 存的是 Surface：模型读写的 Python 的一个子集，写法照 Python，语义以本文为准。Surface 降到求值契约：交出与取报出 Atom，键与资源由 Lang0 自己给出。它满足 croon:docs/Kernel/Definition/Cognition.md「求值契约」的求值契约，守 croon:docs/Kernel/Language.md「守什么」的规则。

本文件夹里，本文讲写法；首版实际收的构造、名字与异常的消息见 Subset.md；求值器的语义与保证见 Evaluator.md，它的实现见 Engineering.md。
## 为什么是 Python 的写法

- 写法照 Python，子集内的语义与 Python 相同。没见过的语法，模型只能照相似的去猜；与先验每差一处，就要靠提示或报错补一处，差得越少越好。
- 算与做分工：Lang0 只算出要做什么，计算与接触外部世界交给 exec。
- 必须自己拥有的，正是 Python 不提供的：交出与取、引用值、并发、键、资源的度量。其余照用，不自造关键字。
- 没有采用别的语言作 Surface，如 Go、OCaml、Rust、TypeScript：body 搬运的多是 JSON 形状的值，静态类型的写法在每个边界多一道解码；失败默认向外传播，与 Go 的错误值、Rust 的 Result 相反；Signature 要从类型推出 Schema，Go 没有联合与字面量类型；Definition 构造完就实例化，不经编译，用它们作 Surface，要么把编译器放进构造，要么自己再写一个解释器。TypeScript 的类型对得上，但 `==` 的隐式转换、`this`、`null` 与 `undefined` 两个空值、事件循环，要拦的不会比 Python 少。

## 拦什么

这里是 Python 里有、因上面那条判据而拦下或改定的；子集里没有的其余构造同样处理。拦下的 builtin 仍有名字，调用它以确定的失败报出用什么代替：写成「用 X 代替 Y」，不写「不允许」。

| 起因 | 例子 | 处理 | 由什么推出 |
|---|---|---|---|
| 世界 | print、input、open、time.time、datetime.now、random、uuid4、os、sys、socket、subprocess | 拦。print 的报错说用 return 或交出代替，调试看 Journal；time.sleep 的报错说用 receive 的 timeout | body 只经 Interface 进出；确定性 |
| 调度与时间 | threading、asyncio、async/await；几个 future 的完成先后 | 拦，见 Evaluator.md 的「并发」 | 输出与结果到达的顺序无关 |
| 内存地址 | `id()`；对象与函数默认的 repr 带地址；默认的 hash 按身份算 | id 拦；默认 repr 由 Lang0 定为不带地址的 `<function f>`、`<X object>`；按身份算 hash 的值不能作 dict 的键，见 Subset.md | 地址取决于宿主与运行时自己的分配 |
| hash 决定的顺序 | set 的迭代顺序随元素的 hash 变 | set、frozenset 换成按插入顺序的实现。Python 没有规定 set 的顺序，所以这仍是 Python 的语义 | 同上；str 的 hash 也就不再影响行为 |
| 回收时机 | `__del__`、weakref、gc；没迭代完的生成器被回收时运行它的 finally | 前三个拦；没迭代完的生成器不随回收关闭，finally 不因回收而运行。Python 不保证回收时运行它，所以这仍是 Python 的语义 | 回收时机取决于分配，随运行时变 |
| 不可变值的同一性 | `x is 1000`、字符串驻留 | `is` 只用于 None、True、False 与可变对象；两边都是不可变的值时以确定的失败报出用 `==` | 小整数缓存与驻留取决于编译单元，降法会改变它 |
| 宿主的数学库 | math.sin、非整数次幂 | 不用宿主的 libm：不进子集，或由 Lang0 自带实现 | 宿主 |
| 按数据找名字 | eval、exec、compile、locals、globals、vars、`__import__`、dunder 属性 | 拦 | 规则 3（Required port 不能按数据找到）；兼作纵深防御 |

## 各构造

表里的「放开」指没有理由拦，不等于首版都收入：首版实际收的见 Subset.md，其余报错；后加是增量，删除会破坏已构造的 Definition。

| 构造 | 处理 | 说明 |
|---|---|---|
| global、del、`is None`、海象、星号解包、链式比较、f-string、set 字面量 | 放开 | 都是 Python；set 按上表处理 |
| class、dataclass、enum | 放开 | 值的种类不再封闭。顶层的 class 能 pickle 进并行的分支，嵌套的不能，与 Python 一样 |
| `class X(TypedDict)` | 放开 | 与 `X = TypedDict("X", {...})` 同义，Schema 都从 AST 里读 |
| with、match、装饰器 | 放开 | 它们本来就是 Python。只有 `main` 不能带装饰器，因为 Interface 要从 def 读出来 |
| yield、yield from | 放开 | 与生成器表达式是同一样东西；回收时机按上表处理 |
| 全局 | 不冻结 | 分支各有一份独立的状态，没有互相可见的可变全局，所以不需要冻结；分支按 spawn 的语义拿到构造时的模块 |
| import | 收 `croon`、白名单里的标准模块、模块目录 里的模块，见「模块」 | 开放的标准模块要能按数据量计步、确定、不接触外部世界。先开哪些见待定；re 会回溯，代价不随输入线性增长，不开 |
| functools.partial | 放开 | `.func`、`.args`、`.keywords` 读不出来：绑上的参数读不出来 |
| 失败 | Python 的异常照用 | `except KeyError` 接的就是 Python 的 KeyError，见「失败」 |

## 模块

body 是只含 source 与可选 modules 的对象。source 是 `main` 所在模块的源码字符串，modules 是目录 Bundle ID；不使用顶层 code 字段。模块目录采用 croon:docs/Tree.md「Tree」的形状，作为固定的目录 Bundle；文件和子目录都指向 Bundle。import 照 Python 的目录语义：目录顶层是起点，`a.b` 找 `a/b.py` 或 `a/b/`，包可以没有 `__init__.py`，包内可以相对 import。模块目录顶层不能占用 `croon` 或白名单模块名，路径经过 link 时拒绝。目录的 Tree、导入与物化归 croon-app:docs/Directory/README.md「Tree」。

形状是 `lang0.body`。

构造检查和 open 装载都只读 Definition Bundle 的固定闭包，不借构造者或 Instance 的额外取得范围。模块顶层在 import 时求值一次，受同样的资源上限。缺少代码依赖时构造失败；补依赖得到另一份完整 Definition 身份。

Library 的预置函数与组合见 croon-app:docs/Library/README.md「地图」；Lang0 只规定模块目录与 import 的语义。

`croon` 是 Lang0 自己的模块；名字都显式 import，清单见 Subset.md。最小 Definition 只需 body.source，不需要空目录。

## Interface

Interface 就是 `main` 带类型的签名：

- 值类型的参数是 Signature 的 in 的字段，名字就是参数名；返回类型是 Signature 的 out。
- Port 类型的参数是 Required port，`Inbox` 类型的参数是 Provided port，名字都是参数名。
- 描述取自 `main` 的 docstring。

实例化时，in 的各字段、接上的 Reference 与自己的 Provided port 按参数名交给 `main`：值放在 input 里，接线放在附带里。`main` 一生只在实例化时求值一次，不被别的 body 调用；复用代码靠 import 函数，见 croon:docs/Kernel/Definition/README.md「Cognition 与 Charter」。诞生里的其余部分不是参数，见下面的判据。

宿主要拼一份 Definition 时，用 `lang0::interface` 从 body 推出 interface，它与构造检查走同一条装载与子集检查。同一套推导适用于任何函数：没绑定的参数与返回类型给出它的 Schema，函数与各参数的描述取自 docstring 与其中的 `Args:`，规则见下面的「推导」。宿主要给模块做索引时，用 `lang0::modules` 枚举模块目录里的模块，用 `lang0::surface` 推一个模块的公开面（block `lang0.surface`）：顶层不以下划线开头的函数、TypedDict、Port 与 Schema，函数写标注的源码文本、描述与各参数的说明，声明写推出的 Schema 或 Port spec；单个声明推不出时只在那一项写 `error`，不让一项坏了整份索引。`croon.tool(f)` 用它把函数变成交给模型的工具：一个工具对象，`.name` 是函数名，`.spec` 是交给 llm 的说明；调用它时，先按推出的 Schema 查交来的 input，再调 f，再查返回值，都由 Kernel 的校验器查。所以模型写错的参数、工具误返回的不是 Value 的东西，都在这次工具调用上失败，不流到下一次交出。正文自己拼不出这些：函数名与 docstring 要经 dunder 属性，被拦下了。没绑定的参数里还有 Port、Inbox 或 `Callable` 类型的，推不出来，报错说在外层函数里捕获它，或用 partial 绑上。`Callable` 是合法的标注，不映射到 value.schema；推 Schema 只发生在 `main` 的签名与 `tool` 上，所以普通函数的参数可以用它。

引用值只从三处进来：`main` 的参数、交出的结果、到达。没有环境里的引用值，也没有环境里的 Principal：指向 Principal 的 Reference 同样作为 `main` 的一个 Port 参数接上。函数要用的引用值由调用方给，或捕获外层函数的局部变量，所以一个函数够得到什么，在源码里看得见：它的签名，加上外层函数的局部变量。判据是：作者叫得出名字、又带权利的，显式地传；叫不出名字的，如 Atom 的键，留在求值器里。

叫得出名字、不带权利的数据，看它是实例化的一方给的，还是属于这个主体：

- **实例化的一方给的**：Signature 的 in 与接线，是 `main` 的参数。
- **这个主体的**：`croon.definition()`、`croon.seat()`、`croon.roster()` 取自诞生，只在 main 里读。业务内容通过 `croon.read_json(id)`、`croon.read_text(id)` 读取，读取资格取自诞生与程序已经消费的 Atom 结果；知道 ID 不授予资格。文件快照是 Signature 的普通输入，由函数参数或闭包中的变量持有。

推导的规则随 `lang0@1` 冻结：构造时推出的 Interface 与 Definition 里写的按规范文本比，不等就构造失败；tool 的说明进交给 llm 的 Value，重放时要逐字相同。

- **docstring**：按 `inspect.cleandoc` 去掉缩进。`Args:` 一行之前的部分去掉首尾空白，是描述；之后每个 `名字: 说明` 是一个参数的说明，缩进更深的续行以一个空格接上，去掉首尾空白后成为那个参数的 Schema 的 description。没有 docstring 的，描述是空串；没有说明的参数，不写 description。
- **in 与 tool 的 input**：`{"type": "object", "properties": …, "required": […], "additionalProperties": false}`。properties 与 required 都按参数的先后，required 是没有默认值的参数；没有值类型的参数时两者为空。out 是返回类型的 Schema。键都写出，TypedDict 的字段、`T | U`、`Literal` 都按书写的先后：数组的顺序进规范文本。
- **标注不求值**：类型位置上的名字沿 import（含 as 与模块属性，如 `croon.Port`）解析，或读取本模块、import 来的模块里顶层唯一一次赋值的 AST；赋值的右边只收 `TypedDict(...)`、`Port(...)`、`Schema(...)`。普通别名如 `TD = TypedDict`、`Seq = list`、`C = croon`、`Out2 = Out` 首版不追，用到类型位置时报错；以后放宽是增量。
- **tool 的 `.spec`**：`{"name": 函数名, "description": 描述, "input": in}`，形状是 model.generate 的 tools 的一项。返回类型只用来查返回值，不进 spec。
- **Port 参数**：requires、provides 的名字是参数名，值是那份 Port spec，见「Port 与交出」。

函数要使用的 Reference 写在参数里或从外层闭包捕获；模块顶层在构造时没有接线。库函数与调用方在同一个 Instance 求值，引用与快照都由调用方显式传入。

## 值与类型

Value 在 Lang0 里就是 dict、list、字符串、数与 None，对象用下标访问，如 `r["stdout"]`。形状只作类型，用来检查，不改变值：只有一种表示，进出都不转换，字面量写出的 dict 与交来的 dict 按相等比较。这也是 Python 对带类型的 JSON 的答案：TypedDict 的值就是普通的 dict。

没有采用把有形状的对象转成用属性访问的 record：映射（目录的名字、附带的名字）成不了属性，anyOf 要先判断是哪一种才转得了、几种都匹配时定不下来，`{}` 没有形状可转，于是同一份 Value 有时是 record、有时是 dict，`value == {"op": "stop"}` 会悄悄变成 False。让 dict 也能用属性访问，同样是两种表示，键还会与方法撞名。

Lang0 的类型是 value.schema 的另一种写法，不另设类型系统，语义以 value.schema 为准：

| Lang0 | value.schema |
|---|---|
| `str`、`int`、`float`、`bool`、`None` | string、integer、number、boolean、null |
| `list[T]` | array，items 为 T |
| `dict[str, T]` | object，additionalProperties 为 T |
| `TypedDict("X", {"a": T, "b": NotRequired[U]})` | object：properties 为 a、b，required 为 a，additionalProperties: false |
| `T \| U` | anyOf |
| `Literal["a", "b"]` | enum |
| `Any` | `{}` |
| `Annotated[T, "描述"]` | T 加上 description |
| 参数有默认值 | 不进 required，缺省时用默认值 |

边角按 value.schema 定：

- integer 是值为整数的 number，所以 1.0 满足 `int`；`float` 接受整数，因为 number 包含 integer，与 PEP 484 一致。
- TypedDict 默认封闭：Python 的默认不封闭，这里选严的一边，多出的键报错，不会悄悄放过。
- 缺键与 null 分开：`NotRequired[T]` 是可以没有这个键，`T | None` 是可以为 null。JSON 区分这两种，TypedDict 也区分。
- 进来的数，规范文本里没有小数点与指数的是 int，其余是 float：交出的 `2.0` 回来是 `2`，`-0.0` 回来是 `0`。Value 不分 1 与 1.0，见 croon:docs/Kernel/Value.md「内容寻址」；这是与 Python 的 json 不同的一处。
- int 在求值器里照 Python 无界，按字长计步；只在进出 Value 的边界上（交出、done、Signature 与 `croon.tool` 的检查）超出 `number_max` 以 type 失败，不是有限数的 float 同样。不把 int 限在这个范围里：那样多一处与 Python 的差异，好处只是错报在算出它的那一步。
- 表外的标注推不出 Schema：dataclass、enum 的类之类写在 `main` 或 `tool` 的签名上，构造时报错，说用 TypedDict 或 Literal；函数体内照用，它们的实例不是 Value，交出时以 type 失败。

## Port 与交出

**引用值**是一种自定义的值类型，不透明，守 croon:docs/Kernel/Language.md「一套名字、两种值」的三条规则：没有与字符串互转的方法，repr 与编码成 JSON 时碰到就失败，进不了交出的 Value。交出统一使用 `croon.send`；引用值本身不可调用。

Port spec 本身是一个值，Required 与 Provided 只是角色：

```python
from typing import TypedDict, NotRequired
from croon import Port, Inbox
from library.model import Generate

Msg = TypedDict("Msg", {"text": str})
Answer = TypedDict("Answer", {"text": str})
Chat = Port(Msg, attach = {"reply": NotRequired[Port(Answer)]}, doc = "交来一条消息，可附带回话途径")

def main(task: str, engine: Generate, me: Chat, inbox: Inbox(Chat)) -> str:
```

- `Port(In, attach = ..., doc = ...)`：attach 不写就是不收附带，JSON 明写 `additionalProperties: false`。写成名字到 Port spec 的字面量时，生成 properties、按书写顺序的 required 与 `additionalProperties: false`；照 TypedDict，每项默认必须带，包一层 `NotRequired[P]` 的不进 required。P 收绑定到 Port 调用的名字或直接的 Port 调用；这套简写首版不收「任意 Port」与「名字任意、形状统一」。这套简写生成的 Port 没有 out；完整形状用下一条的 `Port(spec)`，Provided port 仍不能有 out。
- 写在参数上是 Required port，包一层 `Inbox` 是 Provided port，指向同一份 Port spec。所以指向自己的 Reference `me` 与 `inbox` 的形状只写一遍；实例化时用 self 把 `me` 接到 `inbox` 上。
- `Port(spec)` 收一份写成字面量的完整 Port spec，`Schema(s)` 收一份写成字面量的 Schema、当作类型用。Kernel 各 Port 的 Port spec 与 Outcome 所在的生成模块就是这样写的，见「模块」。

交出写成 `croon.send(ref, value, dependencies=[], attach={})`，报出一个 send Atom。ref 是 Required port 或运行中取得的引用值；dependencies 是 Bundle ID 的 list；attach 是名字到引用值的 dict。三者分别交付数据、不可变内容和权利，不共享名字空间。返回总是回执，与 Port 是否声明附带无关。普通 Python 函数仍按普通函数调用。

引用值的本地类型来自自己声明的 Port spec；声明为任意规格时用 Kernel 在结果中给出的完整 spec。Kernel 按派生链上的每一节检查，收窄在用到时以 denied 出现。

`croon.deliver(value, dependencies)` 构造结束交付，供 main 返回；Signature 与工具类型仍标注业务 value 的类型。普通 main 返回值自动以空 dependencies 结束。deliver 只作为 main 的最终返回值，工具与交出的业务 Value 不接受它。

`croon.read_json(id)` 读 JSON，`croon.read_text(id)` 读 UTF-8 字节；不可读或类型不符以 croon.Failure 失败。读取不产生 Atom，不推进键；它把已经按程序顺序消费的 Atom 键交给 Read，恢复时不能用尚未消费的结果提前读未来内容。宿主读取故障独立返回，停下会话，不能由 body 接住。

## 运行时对象

形状里有引用值的，是运行时对象，用属性访问；形状是 Value 的 Schema 的，是 dict。回执、到达、派生出的那一份是运行时对象：

- **回执**：`.value` 是 out 的 Value；`.dependencies` 是本次实际交付的 Bundle ID list；`.attach` 是结果带来的引用值，名字 → 引用值；`.derived` 是附带的名字 → 派生出的那一份。
- **派生出的那一份**：`.handle` 是数据，就是对方沿它交回来时到达的 `.via`；`.revoker` 是引用值，用 send 交出就撤销这一份。
- **到达**：`.port`、`.sender`、`.via`、`.value`、`.dependencies`、`.attach`。

它们不可变，记的是已经发生的事。它们不是 Value，交出时按规则 2 以 type 失败，报错说用 `.value` 取出数据。交给并发时按组成部分过去：数据深拷贝，引用值原样传。所以它们是同一种东西：由数据与引用值组成的不可变的值。

## 取

```python
a = receive(inbox)  # 一直等
a = receive([inbox, done], timeout = 30)  # 没等到返回 None
a = receive(inbox, via = h)  # 只取沿这一份交回来的
a = receive(inbox, sender = s)  # 只取某个 Instance 交来的
```

- 等不到返回 None，不抛错：等不到是正常的控制流，如轮询与超时，抛错就要每次包一层 try。这与 Kernel 的结果为 null 一致。
- `a.port` 是传进来的那个 Inbox 本身，可以写 `if a.port == done:`。
- via 与 sender 收字符串，认出别人的标识是数据。via 取自回执的 `derived[名字].handle` 或别的到达的 `.via`，sender 取自 `.sender` 或名册。
- 参数名与 Kernel 的键相同：`timeout`、`sender`。Kernel 不用 from，因为它是 Python 的保留字。

## 失败

在契约层的 kind 之外，Lang0 加一种 limit，其余用 Python 异常的类名。种类用 `croon.kind(e)` 读，data 用 `croon.data(e)`，`str(e)` 是消息：`type(e).__name__` 是 dunder，被拦下了；不给 Python 的异常加属性，免得多一处与 Python 的差异。

| kind | 异常 | 何时 |
|---|---|---|
| use、denied、type | `croon.Failure`，Exception 的子类 | 与 croon:docs/Kernel/Definition/Cognition.md「失败」相同；data 固定为 `{error: 原业务 data, dependencies: 实际交付根}`，保留失败产物 |
| limit | `croon.Limit`，只是 BaseException 的子类 | 超出 Evaluator.md 的「资源」里的上限 |
| 类名 | Python 的异常 | 运行时错误与主动 raise 的；data 是 null；Lang0 自己抛出的消息见 Subset.md |

terminated 是 Kernel 造成的结束，包含给在途交出写下的收尾结果，不送回 Lang0，所以上表不把它映射成可接住的异常。

limit 不是 Exception：ReAct 这类代码用 `except Exception` 把工具的失败交还模型，limit 不该被它接住；接住了也空转不了，计数到下一个 Atom 才清零。kind 由异常对象的类定，裸 `raise` 重抛，kind 不变。失败默认向外传播，没接住的，主体以失败结束，结束状态的 error 就是 kind、`str(e)` 与 data。

## 待定

- Bundle 字段的写法：croon 的 value.schema 若能标出「这个字段是 Bundle」（见 croon:docs/Kernel/Value.md「待定」），Lang0 要有对应的类型写法，如 `croon` 里的一个类型名，推导时生成那个关键字。随上游定。场景：一个 Definition 收两个目录。
- 模块的白名单：首版只开 typing，见 Subset.md。候选还有 dataclasses、enum、functools、json、math、itertools、collections；每开一个都随 `lang0@1` 冻结，关掉会破坏用到它的 Definition，所以从少开起。concurrent.futures 首版不收：并发还没有场景验收，见 Evaluator.md 的待定，带着它冻结，以后再加入显式 Bundle 输入与产物。
- Library 的代码与组合见 croon-app:docs/Library/README.md「地图」；Lang0 只规定它所运行的模块目录与 import 语义。
- use、denied、type 要不要再继承 Python 的异常类，如 type 同时继承 TypeError，让具体的 except 也接得住；加了会改变哪些 except 接得住，所以在 `lang0@1` 发布前定。没有场景。

## 名词表

一个概念一行，按 Lang0 的写法与求值顺序排。表中沿用 Croon 的名词定义；本仓库只补 Lang0 的用法。

| English | 中文 | 正文 | 用法 |
|---|---|---|---|
| Ref | 引用值 | 中 | Language 里指向一个 Reference 的值；Ref 与 Reference 太近，所以用中文 |
| Lang0 | 第一种语言 | 英 | 占位名 |
| Surface | 表层 | 英 | 模型读写的 Python 写法 |
| Evaluator | 求值器 | 中 | |
| Lowering | 降 | 中 | 从 Surface 降到求值契约 |
| Checkpoint | 检查点 | 英 | 从这里能接着走的 Language 状态 |
| branch / history branch | 分支 / 历史分支 | 中 | 不带限定的分支只指 Lang0 并发的分支 |

参考：Subset.md（清单、拦下时说什么、异常的消息）；Evaluator.md（引擎、求值、并发、键、资源、文本、构造时检查、待定）；Engineering.md（选择、降、隔离）。
