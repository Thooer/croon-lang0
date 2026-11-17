# 首版子集

`lang0@1` 首版收的构造、builtin、方法、模块与 `croon` 的名字，以及 Lang0 自己给出的异常消息。构造时的检查与求值器都从 `lang0.subset` 与 `lang0.messages` 读；列表外的在构造时报错，报错文字见「拦下时说什么」。README.md 的「各构造」说的是哪些构造没有理由拦，这里说首版实际收了哪些：以 W0、S、ReAct 与文件工具（library/）用到的为底，加上写同类程序几乎必用、实现上不另起机制的几样。加是增量，删会破坏已构造的 Definition，所以从少起。

## 清单

名字用 Python 标准库 `ast` 的节点名，与 CPython 的文档对得上；解析器换了，名字不变。builtin 与方法写的是首版收的调用形式：Python 的形参写法，不含 self，参数名、种类与默认值照 CPython 3.12；有几种形式的，按书写的先后列出。

清单是 `lang0.subset`。

形状之外的规则：

- 函数的参数只有普通的位置或关键字参数，可以带默认值；`*args`、`**kwargs`、仅位置与仅关键字的分隔符不收。调用可以写 `*xs`、`**d` 与关键字参数。
- `Try` 收 except 与 finally，except 可以写 `as e`；else 不收。`Raise` 收 `raise X(...)` 与裸 `raise`，`from` 不收。
- `BitOr` 只在类型位置出现：标注、TypedDict 的字段类型、Port 与 Inbox 的类型参数、类型的下标参数；如 `Inbox | None`。类型名字的解析见 README.md 的「Interface」；顶层 `X = A | B` 首版不收。
- f-string 只收 `{x}`，等于 `str(x)`；转换（`!r`）与格式说明（`:>8`）不收。`str.format` 收 `{}` 与 `{名字}`，名字是 Python 标识符，Unicode 也可；两种可以混用，`{{`、`}}` 是字面的花括号。数字索引、属性、下标、转换与格式说明不收。字面量模板在构造时查，动态模板在运行时查，不收的字段报 `format_field`。
- 拦下的 builtin 按绑定判，不按字面名字：参数或局部变量叫 `exec`、`print` 是合法的，使用未被遮住的 builtin 才拒绝。形如 `__x__` 的 dunder 标识符首版一律拒绝，包括属性、变量、参数、import 绑定与关键字参数；这不限制普通字符串的内容。
- 属性名只收清单里的方法名、运行时对象的属性与 import 来的模块的顶层名字。字面量接收者按类型检查；动态接收者的方法按运行时类型分派。清单对 Python 封闭：一个方法名收进来，Python 里有这个方法的子集类型都收它，所以动态接收者上过了构造检查的名字，运行时不支持就是 Python 自己的 AttributeError（`no_attribute`），不另设报错。
- `import *` 与路径上经过 Tree 的 link 条目的模块首版拒绝：名字展开与链接跟随尚未定。普通 import 的规则见 README.md 的「模块」。
- 装饰器首版全部拒绝。Constant 首版不收 bytes、complex 与 Ellipsis；`...` 用 pass 代替。
- 推导 Schema 或 Port spec 用的字面量不收重复键，包括 TypedDict、attach、`Schema(...)` 与 `Port(...)` 的参数；避免后值覆盖悄悄丢掉声明。普通 dict 的后值覆盖照 Python。
- tuple 能写、能解包；进 Value 时以 type 失败，见 `not_value`。
- 运行时对象的 `.attach`、`.derived` 是只读的映射，支持下标、`get`、`in` 与迭代。
- `receive` 的 ports、sender、via 收单个或列表。send 的目标与 value 是两个位置参数，dependencies 与 attach 可按位置或关键字给；deliver 收 value 与 dependencies；read_json、read_text 各收一个 ID 字符串。回执与到达的 dependencies 读取时产生独立 list，修改不改变交付事实。
- 调用 builtin 与方法，按清单里的形式绑定参数，规则照 Python 的函数调用。清单没写的参数首版不收，与 Python 里没有这个参数一样报错，如 `int(x, 16)`、`str(b, "utf-8")`。几种形式的，用第一种绑得上的；都绑不上，报第一种位置参数个数合得上的那一种的错，个数都不合，报 `expected_count`。异常类收任意个位置参数，不收关键字参数。
- 绑定失败的消息不分 builtin 与用户函数，都用 `missing_args` 到 `expected_count` 这一组；CPython 自己的 builtin 措辞各不相同，这是列出的差异。消息里的名字：用户函数是 qualname，如 `outer.<locals>.inner`；builtin 是它的名字；方法是 `类型.方法`，如 `dict.pop`；异常类是类名。
- 下标、`list.pop`、序列的重复次数等要放进机器字长的整数，按 64 位的 CPython 定：下标绝对值超出 `[-2**63, 2**63)` 报 `index_size`，`list.pop`、`len(range(...))`、`str.split` 的 maxsplit、`str.replace` 的 count 超出报 `ssize_t`。不随宿主变。重复与别的产出先算出大小、计入步数，超出资源上限时以 limit 失败，不等分配内存失败。
- int 与字符串互转超过 4300 位报 `int_digits`，与 CPython 3.12 的默认上限相同。
- dict 的键与 `in dict` 只收 None、bool、int、float、str，以及元素也都是这几种的 tuple；别的报 `unhashable`。Python 里函数、异常、range 能作键，是按身份算 hash，见 README.md 的「拦什么」，所以这里不收。
- `is`、`is not` 只在一边是 None、True、False，或两边都是 list、dict 时求值；别的报 `identity_of_value`。一边是别的字面量的，构造时就拒。
- `%` 左边是字符串的报 `percent_format`；左边是字符串字面量或 f-string 的，构造时就拒。
- 迭代 dict 或它的 keys、values、items 时改了 dict 的大小或键，照 Python 报 `dict_changed`；list 照 Python 不查。

## 拦下时说什么

列表外的构造在构造时报出位置与「`lang0@1` 首版不收 X」。下面这些另外说用什么代替，按 README.md 的「拦什么」写成「用 X 代替 Y」：

| 写了 | 报错说 |
|---|---|
| `print` | 用 return 或交出代替；调试看 Journal |
| `time.sleep` | 用 receive 的 timeout 代替 |
| `type(e).__name__` 及其他 dunder 属性 | 失败的种类用 `croon.kind(e)` |
| 其余 dunder 标识符 | 首版不收 dunder 名字 |
| 普通类型别名，如 `Seq = list` 后用 `Seq[T]` | 首版不收类型别名 |
| `import *`、import 经过 link、装饰器、bytes、complex | 首版不收对应写法 |
| `...` | 用 pass 代替 |
| `class`（包括 `class X(TypedDict)`） | 用 `X = TypedDict("X", {...})` 与函数 |
| `lambda`、嵌套的 def 交给并发 | 用顶层 def，要固定的参数用 functools.partial（首版不收并发） |
| `with`、`yield` | 首版不收；with 改成 try/finally |
| 签名的返回类型写裸 `dict`、`list` | 用 `dict[str, Any]` 或 TypedDict |
| 顶层的引用值，如 `exec = Exec()` | 引用值写在 `main` 的参数里，工具在外层函数里捕获它 |
| 字面量上用 `is`，如 `x is 1` | 用 `==` 比较值 |
| 字符串字面量上用 `%` | 用 f-string 或 str.format 代替 % |

## 异常的消息

Lang0 自己抛出的异常，类名用 Python 的，消息以下面为准：它们会经 `str(e)` 流进交出的值（如 ReAct 交还模型的工具结果），重放时要逐字相同，所以随 `lang0@1` 冻结。没标 `own` 的取自 CPython 3.12 的实际输出，由 crates/lang0/cpython/messages.py 在 CPython 上逐条对照；以后 CPython 改了措辞，这里不跟。标了 `own` 的是 Lang0 自己的。

占位：`{type}`、`{left}`、`{right}` 是值的类型名，见 `type_names`；`{repr}` 是 repr，见 Evaluator.md 的「文本」；`{name}` 是绑定失败时按上面的规则写的名字，别处是标识符；其余照字面。message 是对象的，按键选：类型名、方法名或注明的情形。`args` 写的是异常的 args，没写的就是 `[消息]`。

消息是 `lang0.messages`。

message 里的 `{{`、`}}` 是字面的花括号。列表外的情形在实现时补进这里，没标 `own` 的先在 messages.py 里加一条对照；`lang0@1` 发布之前这张表算待定，发布之后只增加。

参考：README.md（拦什么、各构造、Interface、失败）；Evaluator.md（引擎、文本）；Engineering.md（降）。
