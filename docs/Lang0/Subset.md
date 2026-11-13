# 首版子集

`lang0@1` 首版收的构造、builtin、方法、模块与 `croon` 的名字，以及 Lang0 自己给出的异常消息。构造时的检查与求值器都从这里的 block 读；列表外的在构造时报错，报错文字见「拦下时说什么」。README.md 的「各构造」说的是哪些构造没有理由拦，这里说首版实际收了哪些：以 W0、S、ReAct 与文件工具（library/）用到的为底，加上写同类程序几乎必用、实现上不另起机制的几样。加是增量，删会破坏已构造的 Definition，所以从少起。

## 清单

名字用 Python 标准库 `ast` 的节点名，与 CPython 的文档对得上；解析器换了，名字不变。builtin 与方法写的是首版收的调用形式：Python 的形参写法，不含 self，参数名、种类与默认值照 CPython 3.12；有几种形式的，按书写的先后列出。

```json lang0.subset
{
  "statements": ["Module", "Expr", "Assign", "AugAssign", "FunctionDef", "Return", "If", "For", "While", "Break", "Continue", "Pass", "Try", "Raise", "Import", "ImportFrom"],
  "expressions": ["BoolOp", "BinOp", "UnaryOp", "IfExp", "Dict", "List", "Tuple", "ListComp", "DictComp", "Compare", "Call", "Constant", "JoinedStr", "FormattedValue", "Attribute", "Subscript", "Name", "Starred"],
  "operators": ["And", "Or", "Add", "Sub", "Mult", "Div", "FloorDiv", "Mod", "Not", "USub", "Eq", "NotEq", "Lt", "LtE", "Gt", "GtE", "Is", "IsNot", "In", "NotIn"],
  "annotation_only": ["BitOr"],
  "builtins": {
    "len": ["(obj, /)"],
    "range": ["(stop, /)", "(start, stop, step=1, /)"],
    "str": ["(object='')"],
    "int": ["(x=0, /)"],
    "float": ["(x=0.0, /)"],
    "bool": ["(x=False, /)"],
    "list": ["(iterable=(), /)"],
    "dict": ["(mapping_or_iterable=(), /, **kwargs)"],
    "isinstance": ["(obj, class_or_tuple, /)"],
    "min": ["(iterable, /, *, key=None)", "(iterable, /, *, default, key=None)", "(arg1, arg2, /, *args, key=None)"],
    "max": ["(iterable, /, *, key=None)", "(iterable, /, *, default, key=None)", "(arg1, arg2, /, *args, key=None)"],
    "sorted": ["(iterable, /, *, key=None, reverse=False)"],
    "enumerate": ["(iterable, start=0)"],
    "zip": ["(*iterables, strict=False)"],
    "any": ["(iterable, /)"],
    "all": ["(iterable, /)"],
    "sum": ["(iterable, /, start=0)"],
    "abs": ["(x, /)"]
  },
  "croon_calls": {
    "send": ["(ref, value, dependencies=[], attach={})"],
    "deliver": ["(value, dependencies)"],
    "read_json": ["(id, /)"],
    "read_text": ["(id, /)"]
  },
  "exceptions": ["BaseException", "Exception", "ArithmeticError", "LookupError", "KeyError", "IndexError", "ValueError", "TypeError", "RuntimeError", "OSError", "ZeroDivisionError", "OverflowError", "AttributeError", "NameError", "UnboundLocalError"],
  "methods": {
    "dict": {
      "get": ["(key, default=None, /)"], "setdefault": ["(key, default=None, /)"],
      "keys": ["()"], "values": ["()"], "items": ["()"],
      "pop": ["(key, /)", "(key, default, /)"], "update": ["(other=(), /, **kwargs)"], "clear": ["()"]
    },
    "list": {
      "append": ["(object, /)"], "extend": ["(iterable, /)"], "pop": ["(index=-1, /)"], "clear": ["()"],
      "index": ["(value, start=0, stop=9223372036854775807, /)"]
    },
    "str": {
      "join": ["(iterable, /)"], "split": ["(sep=None, maxsplit=-1)"], "strip": ["(chars=None, /)"],
      "startswith": ["(prefix, start=None, end=None, /)"], "endswith": ["(suffix, start=None, end=None, /)"],
      "replace": ["(old, new, count=-1, /)"], "format": ["(*args, **kwargs)"], "index": ["(sub, start=None, end=None, /)"]
    },
    "tuple": { "index": ["(value, start=0, stop=9223372036854775807, /)"] }
  },
  "modules": {
    "typing": ["TypedDict", "NotRequired", "Literal", "Any", "Annotated", "Callable"],
    "croon": ["Port", "Inbox", "Schema", "Failure", "Limit", "receive", "tool", "send", "deliver", "read_json", "read_text", "definition", "seat", "roster", "kind", "data"]
  }
}
```

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

```json lang0.messages
{
  "key_missing": { "class": "KeyError", "args": ["{key}"], "note": "args 是那个键本身，所以 str(e) 是它的 repr；dict 的下标、pop 与 str.format 的名字字段都是它" },
  "index_out_of_range": { "class": "IndexError", "message": { "list": "list index out of range", "tuple": "tuple index out of range", "str": "string index out of range", "range": "range object index out of range" } },
  "assign_out_of_range": { "class": "IndexError", "message": "list assignment index out of range" },
  "pop_empty": { "class": "IndexError", "message": "pop from empty list" },
  "pop_out_of_range": { "class": "IndexError", "message": "pop index out of range" },
  "index_size": { "class": "IndexError", "message": "cannot fit 'int' into an index-sized integer", "note": "重复次数超出时类是 OverflowError，消息相同" },
  "ssize_t": { "class": "OverflowError", "message": "Python int too large to convert to C ssize_t" },
  "div_by_zero": { "class": "ZeroDivisionError", "message": { "int": "division by zero", "float": "float division by zero" } },
  "floordiv_by_zero": { "class": "ZeroDivisionError", "message": { "int": "integer division or modulo by zero", "float": "float floor division by zero" } },
  "mod_by_zero": { "class": "ZeroDivisionError", "message": { "int": "integer modulo by zero", "float": "float modulo" } },
  "int_div_too_large": { "class": "OverflowError", "message": "integer division result too large for a float" },
  "int_to_float": { "class": "OverflowError", "message": "int too large to convert to float" },
  "unsupported_operand": { "class": "TypeError", "message": "unsupported operand type(s) for {op}: '{left}' and '{right}'" },
  "concat": { "class": "TypeError", "message": "can only concatenate {left} (not \"{right}\") to {left}" },
  "mul_non_int": { "class": "TypeError", "message": "can't multiply sequence by non-int of type '{type}'" },
  "unorderable": { "class": "TypeError", "message": "'{op}' not supported between instances of '{left}' and '{right}'" },
  "bad_abs": { "class": "TypeError", "message": "bad operand type for abs(): '{type}'" },
  "catch_non_exception": { "class": "TypeError", "message": "catching classes that do not inherit from BaseException is not allowed" },
  "descriptor_receiver": { "class": "TypeError", "message": "descriptor '{name}' for '{owner}' objects doesn't apply to a '{type}' object" },
  "float_nan_to_int": { "class": "ValueError", "message": "cannot convert float NaN to integer" },
  "star_value_not_iterable": { "class": "TypeError", "message": "Value after * must be an iterable, not {type}" },
  "croon_argument": { "own": true, "class": "TypeError", "message": { "receive": "croon.receive 的 ports 用 Inbox 或 Inbox 的 list", "tool": "croon.tool 的参数用函数", "exception": "croon.{name} 的参数用异常", "attach": "附带用引用值；数据放进交出的 Value" } },
  "declaration_only": { "own": true, "class": "TypeError", "message": "{name} 用于类型声明；运行时用声明的类型对应的值" },
  "bad_unary": { "class": "TypeError", "message": "bad operand type for unary {op}: '{type}'" },
  "in_string": { "class": "TypeError", "message": "'in <string>' requires string as left operand, not {type}" },
  "in_not_iterable": { "class": "TypeError", "message": "argument of type '{type}' is not iterable" },
  "not_callable": { "class": "TypeError", "message": "'{type}' object is not callable" },
  "not_subscriptable": { "class": "TypeError", "message": "'{type}' object is not subscriptable" },
  "no_item_assignment": { "class": "TypeError", "message": "'{type}' object does not support item assignment" },
  "not_iterable": { "class": "TypeError", "message": "'{type}' object is not iterable" },
  "not_mapping": { "class": "TypeError", "message": "'{type}' object is not a mapping", "note": "dict 字面量里的 **" },
  "not_integer": { "class": "TypeError", "message": "'{type}' object cannot be interpreted as an integer", "note": "range 的参数、list.pop、enumerate 的 start 这类要整数的位置" },
  "bad_slice_index": { "class": "TypeError", "message": { "list": "slice indices must be integers or have an __index__ method", "str": "slice indices must be integers or None or have an __index__ method" }, "note": "index 与 startswith 这类方法的 start、end；list 与 tuple 的 index 用 list" },
  "no_len": { "class": "TypeError", "message": "object of type '{type}' has no len()" },
  "unhashable": { "class": "TypeError", "message": "unhashable type: '{type}'" },
  "bad_index": { "class": "TypeError", "message": { "list": "list indices must be integers or slices, not {type}", "tuple": "tuple indices must be integers or slices, not {type}", "str": "string indices must be integers, not '{type}'", "range": "range indices must be integers or slices, not {type}" } },
  "no_attribute": { "class": "AttributeError", "message": "'{type}' object has no attribute '{name}'" },
  "missing_args": { "class": "TypeError", "message": "{name}() missing {n} required positional argument{s}: {names}", "note": "names 是 'a'、'a' and 'b'、'a', 'b', and 'c'；n 为 1 时 s 为空" },
  "missing_kwonly": { "class": "TypeError", "message": "{name}() missing {n} required keyword-only argument{s}: {names}" },
  "too_many_args": { "class": "TypeError", "message": "{name}() takes {n} positional argument{s} but {given} given", "note": "有默认值时 n 写成 from {min} to {max}，s 照 max；given 是 1 was、2 were，调用里另有绑上的仅关键字参数时写成 2 positional arguments (and 1 keyword-only argument) were" },
  "unexpected_kwarg": { "class": "TypeError", "message": "{name}() got an unexpected keyword argument '{key}'" },
  "multiple_values": { "class": "TypeError", "message": "{name}() got multiple values for argument '{key}'" },
  "multiple_kw_values": { "class": "TypeError", "message": "{name}() got multiple values for keyword argument '{key}'", "note": "两个 ** 给了同一个名字" },
  "positional_as_kw": { "class": "TypeError", "message": "{name}() got some positional-only arguments passed as keyword arguments: '{names}'", "note": "names 用 ', ' 连起来，外面只有一对引号" },
  "expected_count": { "class": "TypeError", "message": "{name} expected at least {n} argument{s}, got {got}", "note": "多于上限时 at least 换成 at most；name 后没有括号" },
  "keywords_not_str": { "class": "TypeError", "message": "keywords must be strings" },
  "star_star_not_mapping": { "class": "TypeError", "message": "{name}() argument after ** must be a mapping, not {type}" },
  "star_not_iterable": { "class": "TypeError", "message": "{name}() argument after * must be an iterable, not {type}" },
  "unpack_non_iterable": { "class": "TypeError", "message": "cannot unpack non-iterable {type} object" },
  "unpack_few": { "class": "ValueError", "message": "not enough values to unpack (expected {n}, got {got})", "note": "目标里有星号时 expected at least {n}" },
  "unpack_many": { "class": "ValueError", "message": "too many values to unpack (expected {n})" },
  "unbound_local": { "class": "UnboundLocalError", "message": "cannot access local variable '{name}' where it is not associated with a value" },
  "unbound_free": { "class": "NameError", "message": "cannot access free variable '{name}' where it is not associated with a value in enclosing scope" },
  "undefined_name": { "class": "NameError", "message": "name '{name}' is not defined" },
  "raise_non_exception": { "class": "TypeError", "message": "exceptions must derive from BaseException" },
  "reraise_none": { "class": "RuntimeError", "message": "No active exception to reraise" },
  "dict_changed": { "class": "RuntimeError", "message": { "size": "dictionary changed size during iteration", "keys": "dictionary keys changed during iteration" } },
  "int_literal": { "class": "ValueError", "message": "invalid literal for int() with base 10: {repr}" },
  "float_literal": { "class": "ValueError", "message": "could not convert string to float: {repr}" },
  "int_argument": { "class": "TypeError", "message": "int() argument must be a string, a bytes-like object or a real number, not '{type}'" },
  "float_argument": { "class": "TypeError", "message": "float() argument must be a string or a real number, not '{type}'" },
  "float_to_int": { "class": "OverflowError", "message": "cannot convert float infinity to integer" },
  "empty_min_max": { "class": "ValueError", "message": "{name}() iterable argument is empty" },
  "sum_strings": { "class": "TypeError", "message": "sum() can't sum strings [use ''.join(seq) instead]" },
  "range_step_zero": { "class": "ValueError", "message": "range() arg 3 must not be zero" },
  "isinstance_arg": { "class": "TypeError", "message": "isinstance() arg 2 must be a type, a tuple of types, or a union" },
  "zip_strict": { "class": "ValueError", "message": "zip() argument {i} is {longer} than argument{s} {range}", "note": "longer 是 longer 或 shorter；前面只有一个参数时 range 是 1、s 为空，否则是 1-{i-1}" },
  "dict_sequence": { "class": "TypeError", "message": "cannot convert dictionary update sequence element #{i} to a sequence" },
  "dict_sequence_length": { "class": "ValueError", "message": "dictionary update sequence element #{i} has length {n}; 2 is required" },
  "index_not_found": { "class": "ValueError", "message": { "list": "{repr} is not in list", "tuple": "tuple.index(x): x not in tuple", "str": "substring not found" } },
  "join_item": { "class": "TypeError", "message": "sequence item {i}: expected str instance, {type} found" },
  "join_not_iterable": { "class": "TypeError", "message": "can only join an iterable" },
  "empty_separator": { "class": "ValueError", "message": "empty separator" },
  "str_argument": { "class": "TypeError", "message": { "split": "must be str or None, not {type}", "strip": "strip arg must be None or str", "startswith": "startswith first arg must be str or a tuple of str, not {type}", "endswith": "endswith first arg must be str or a tuple of str, not {type}", "replace": "replace() argument {i} must be str, not {type}", "index": "must be str, not {type}" } },
  "format_index": { "class": "IndexError", "message": "Replacement index {i} out of range for positional args tuple" },
  "format_brace": { "class": "ValueError", "message": { "open": "Single '{{' encountered in format string", "close": "Single '}}' encountered in format string", "end": "expected '}}' before end of string" } },
  "format_field": { "own": true, "class": "ValueError", "message": "str.format 首版只收 {{}} 与 {{名字}}，不收 {{{field}}}" },
  "identity_of_value": { "own": true, "class": "TypeError", "message": "is 只用于 None、True、False 与 list、dict；比较 {left} 与 {right} 的值用 ==" },
  "percent_format": { "own": true, "class": "TypeError", "message": "用 f-string 或 str.format 代替 %" },
  "int_digits": { "own": true, "class": "ValueError", "message": { "to_str": "Exceeds the limit (4300 digits) for integer string conversion", "from_str": "Exceeds the limit (4300 digits) for integer string conversion: value has {n} digits" }, "note": "CPython 后面还有一句 sys.set_int_max_str_digits() 的建议，Lang0 没有 sys，所以去掉" },
  "limit": { "own": true, "class": "croon.Limit", "kind": "limit", "message": { "steps": "超出步数上限", "depth": "超出深度上限", "live": "超出活数据上限" } },
  "ref_text": { "own": true, "class": "croon.Failure", "kind": "type", "message": "引用值不能转成文本；给模型看的用 Definition 建的名字代替" },
  "content_read": { "own": true, "class": "croon.Failure", "kind": "use", "message": "croon.{name}: 读不到 {id} 或内容类型不符" },
  "delivery_argument": { "own": true, "class": "TypeError", "message": "croon.{name}: {reason}" },
  "read_at_top": { "own": true, "class": "RuntimeError", "message": "在 main 里读 croon.{name}()" },
  "schema_violation": { "own": true, "class": "croon.Failure", "kind": "type", "message": "{where}{pointer} 违反 {rule}", "note": "where 是 input 或 output；pointer 是 JSON Pointer，根是空串，所以根上的违反写成 input 违反 type；rule 是违反的 value.schema 关键字。pointer 与 rule 取自校验器报出的第一处违反；不用校验器的说明文字，它不随 lang0@1 冻结" },
  "not_value": { "own": true, "class": "croon.Failure", "kind": "type", "message": "{where}{pointer} 不是 Value：{reason}", "note": "where 是 value（交出的值）或 output（main 与工具的返回值）；pointer 是深度优先最先碰到的那一处", "reason": { "tuple": "用 list 代替 tuple", "引用值": "引用值只能经附带交出", "回执": "用 .value 取出数据", "到达": "用 .value 取出数据", "派生出的那一份": "用 .handle 取出数据", "number": "数要有限，绝对值不超过 9007199254740991", "key": "对象的键要是字符串", "limits": "超出 value.limits 的 {limit}", "其余": "{type} 不进 Value" } },
  "type_names": { "None": "NoneType", "bool": "bool", "int": "int", "float": "float", "str": "str", "list": "list", "dict": "dict", "tuple": "tuple", "range": "range", "dict 的 keys、values、items": "dict_keys、dict_values、dict_items", "enumerate": "enumerate", "zip": "zip", "函数": "function", "builtin 与方法": "builtin_function_or_method", "类": "type", "模块": "module", "异常": "它的类名", "引用值": "Ref", "回执": "Receipt", "到达": "Arrival", "派生出的那一份": "Derived", "工具": "Tool", "Port spec": "Port" }
}
```

message 里的 `{{`、`}}` 是字面的花括号。列表外的情形在实现时补进这里，没标 `own` 的先在 messages.py 里加一条对照；`lang0@1` 发布之前这张表算待定，发布之后只增加。

参考：README.md（拦什么、各构造、Interface、失败）；Evaluator.md（引擎、文本）；Engineering.md（降）。
