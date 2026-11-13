"""在 CPython 上对照 docs/Lang0/Subset.md 的 lang0.messages 与 lang0.subset。

用法：python3 -B crates/lang0/cpython/messages.py；要 CPython 3.12。

- 没标 own 的消息：每一条（按键分的，每个键）有一段程序，在 CPython 上跑出的异常的类名与 str(e)
  要等于按模板与给出的占位渲染出来的那一份。builtin 的绑定失败按 Subset.md 是列出的差异，
  所以 missing_args 这一组只拿用户函数对照，expected_count 拿 CPython 自己用这种措辞的 builtin。
- 清单里的调用形式：能解析成 Python 的形参，参数名与 CPython 的一致；方法清单对 Python 封闭。
  参考：docs/Lang0/Subset.md（清单、异常的消息）。
"""
import ast
import inspect
import json
import pathlib
import re
import sys
import warnings

warnings.simplefilter("ignore", SyntaxWarning)

ROOT = pathlib.Path(__file__).resolve().parents[3]
TEXT = (ROOT / "docs/Lang0/Subset.md").read_text()


def block(name):
    return json.loads(re.search(r"```json " + re.escape(name) + r"\n(.*?)```", TEXT, re.S).group(1))


MESSAGES = block("lang0.messages")
SUBSET = block("lang0.subset")

# 条目 → [(按键分的键或 None, 程序, 占位)]。程序的最后一句抛出要对照的异常。
PROBES = {
    "key_missing": [(None, "{}['k']", {"key": "k"})],
    "index_out_of_range": [("list", "[][0]", {}), ("tuple", "()[0]", {}), ("str", "''[0]", {}), ("range", "range(1)[5]", {})],
    "assign_out_of_range": [(None, "x = []\nx[0] = 1", {})],
    "pop_empty": [(None, "[].pop()", {})],
    "pop_out_of_range": [(None, "[1].pop(5)", {})],
    "index_size": [(None, "[1][2**63]", {})],
    "ssize_t": [(None, "[1].pop(2**63)", {})],
    "div_by_zero": [("int", "1 / 0", {}), ("float", "1.0 / 0", {})],
    "floordiv_by_zero": [("int", "1 // 0", {}), ("float", "1.0 // 0", {})],
    "mod_by_zero": [("int", "1 % 0", {}), ("float", "1.0 % 0", {})],
    "int_div_too_large": [(None, "10**400 / 1", {})],
    "int_to_float": [(None, "1.0 + 10**400", {})],
    "unsupported_operand": [(None, "1 - 'a'", {"op": "-", "left": "int", "right": "str"})],
    "concat": [(None, "'a' + 1", {"left": "str", "right": "int"}), (None, "[] + 1", {"left": "list", "right": "int"})],
    "mul_non_int": [(None, "'a' * 'b'", {"type": "str"})],
    "unorderable": [(None, "'a' < 1", {"op": "<", "left": "str", "right": "int"})],
    "bad_abs": [(None, "abs('a')", {"type": "str"})],
    "catch_non_exception": [(None, "try:\n    1/0\nexcept 1:\n    pass", {})],
    "descriptor_receiver": [(None, "dict.pop([], 'x')", {"name": "pop", "owner": "dict", "type": "list"})],
    "float_nan_to_int": [(None, "int(float('nan'))", {})],
    "star_value_not_iterable": [(None, "[*1]", {"type": "int"})],
    "bad_unary": [(None, "-'a'", {"op": "-", "type": "str"})],
    "in_string": [(None, "1 in 'a'", {"type": "int"})],
    "in_not_iterable": [(None, "1 in 1", {"type": "int"})],
    "not_callable": [(None, "x = 1\nx()", {"type": "int"})],
    "not_subscriptable": [(None, "x = 1\nx[0]", {"type": "int"}), (None, "{}.keys()[0]", {"type": "dict_keys"})],
    "no_item_assignment": [(None, "x = 'a'\nx[0] = 1", {"type": "str"})],
    "not_iterable": [(None, "list(1)", {"type": "int"})],
    "not_mapping": [(None, "{**1}", {"type": "int"})],
    "not_integer": [(None, "range(1.5)", {"type": "float"}), (None, "[1].pop('a')", {"type": "str"})],
    "bad_slice_index": [("list", "[1].index(1, 'a')", {}), ("list", "(1,).index(1, None)", {}),
                        ("str", "'a'.index('a', 'b')", {}), ("str", "'a'.endswith('a', 0, 'b')", {})],
    "no_len": [(None, "len(1)", {"type": "int"})],
    "unhashable": [(None, "{[]: 1}", {"type": "list"})],
    "bad_index": [("list", "[1]['a']", {"type": "str"}), ("tuple", "(1,)['a']", {"type": "str"}),
                  ("str", "'a'['a']", {"type": "str"}), ("range", "range(1)['a']", {"type": "str"})],
    "no_attribute": [(None, "'a'.foo", {"type": "str", "name": "foo"})],
    "missing_args": [(None, "def f(a, b): pass\nf()", {"name": "f", "n": 2, "s": "s", "names": "'a' and 'b'"}),
                     (None, "def f(a, b, c): pass\nf()", {"name": "f", "n": 3, "s": "s", "names": "'a', 'b', and 'c'"}),
                     (None, "def o():\n    def i(a): pass\n    return i\no()()", {"name": "o.<locals>.i", "n": 1, "s": "", "names": "'a'"})],
    "missing_kwonly": [(None, "def f(*, k): pass\nf()", {"name": "f", "n": 1, "s": "", "names": "'k'"})],
    "too_many_args": [(None, "def f(): pass\nf(1)", {"name": "f", "n": 0, "s": "s", "given": "1 was"}),
                      (None, "def f(a, b=1): pass\nf(1, 2, 3)", {"name": "f", "n": "from 1 to 2", "s": "s", "given": "3 were"}),
                      (None, "def f(a, *, k=1): pass\nf(1, 2, k=3)", {"name": "f", "n": 1, "s": "", "given": "2 positional arguments (and 1 keyword-only argument) were"})],
    "unexpected_kwarg": [(None, "def f(): pass\nf(x=1)", {"name": "f", "key": "x"})],
    "multiple_values": [(None, "def f(a): pass\nf(1, a=2)", {"name": "f", "key": "a"})],
    "multiple_kw_values": [(None, "def f(a): pass\nf(**{'a': 1}, **{'a': 2})", {"name": "f", "key": "a"})],
    "positional_as_kw": [(None, "def f(a, b, /): pass\nf(a=1, b=2)", {"name": "f", "names": "a, b"})],
    "expected_count": [(None, "min()", {"name": "min", "n": 1, "s": "", "got": 0}),
                       (None, "{}.pop()", {"name": "pop", "n": 1, "s": "", "got": 0})],
    "keywords_not_str": [(None, "def f(): pass\nf(**{1: 2})", {})],
    "star_star_not_mapping": [(None, "def f(): pass\nf(**[1])", {"name": "f", "type": "list"})],
    "star_not_iterable": [(None, "def f(): pass\nf(*1)", {"name": "f", "type": "int"})],
    "unpack_non_iterable": [(None, "a, b = 1", {"type": "int"})],
    "unpack_few": [(None, "a, b = [1]", {"n": 2, "got": 1})],
    "unpack_many": [(None, "a, b = [1, 2, 3]", {"n": 2})],
    "unbound_local": [(None, "def f():\n    y = y\nf()", {"name": "y"})],
    "unbound_free": [(None, "def f():\n    def g(): return x\n    g()\n    x = 1\nf()", {"name": "x"})],
    "undefined_name": [(None, "nope", {"name": "nope"})],
    "raise_non_exception": [(None, "raise 1", {})],
    "reraise_none": [(None, "raise", {})],
    "dict_changed": [("size", "d = {1: 1}\nfor k in d:\n    d[2] = 1", {}),
                     ("keys", "d = {1: 1, 2: 2}\nfor k in d:\n    d.pop(k)\n    d[k + 10] = 1", {})],
    "int_literal": [(None, "int('x')", {"repr": "'x'"})],
    "float_literal": [(None, "float('x')", {"repr": "'x'"})],
    "int_argument": [(None, "int(None)", {"type": "NoneType"})],
    "float_argument": [(None, "float([])", {"type": "list"})],
    "float_to_int": [(None, "int(float('inf'))", {})],
    "empty_min_max": [(None, "min([])", {"name": "min"}), (None, "max([])", {"name": "max"})],
    "sum_strings": [(None, "sum(['a'], '')", {})],
    "range_step_zero": [(None, "range(1, 2, 0)", {})],
    "isinstance_arg": [(None, "isinstance(1, 1)", {})],
    "zip_strict": [(None, "list(zip([1], [2, 3], strict=True))", {"i": 2, "longer": "longer", "s": "", "range": "1"}),
                   (None, "list(zip([1], [2], [3, 4], strict=True))", {"i": 3, "longer": "longer", "s": "s", "range": "1-2"})],
    "dict_sequence": [(None, "dict([1])", {"i": 0})],
    "dict_sequence_length": [(None, "dict([[1]])", {"i": 0, "n": 1})],
    "index_not_found": [("list", "[].index(1)", {"repr": "1"}), ("tuple", "().index(1)", {}), ("str", "'a'.index('z')", {})],
    "join_item": [(None, "''.join(['a', 1])", {"i": 1, "type": "int"})],
    "join_not_iterable": [(None, "''.join(1)", {})],
    "empty_separator": [(None, "'a'.split('')", {})],
    "str_argument": [("split", "'a'.split(1)", {"type": "int"}), ("strip", "'a'.strip(1)", {}),
                     ("startswith", "'a'.startswith(1)", {"type": "int"}), ("endswith", "'a'.endswith(1)", {"type": "int"}),
                     ("replace", "'a'.replace('a', 1)", {"i": 2, "type": "int"}), ("index", "'a'.index(1)", {"type": "int"})],
    "format_index": [(None, "'{}{}'.format(1)", {"i": 1})],
    "format_brace": [("open", "'{'.format()", {}), ("close", "'}'.format()", {}), ("end", "'{a'.format(a=1)", {})],
}

# 去掉的只是 CPython 消息末尾的一句建议，前面要逐字相同。
PREFIX = {
    "int_digits": [("to_str", "str(10**5000)", {}), ("from_str", "int('1' * 5000)", {"n": 5000})],
}


def render(template, values):
    return template.format(**values)


def run(source):
    try:
        exec(compile(source, "<probe>", "exec"), {})
    except BaseException as e:
        return type(e).__name__, str(e)
    return None, None


def main():
    errors = []
    for name, entry in MESSAGES.items():
        if name == "type_names":
            continue
        if entry.get("own") and name not in PREFIX:
            continue
        probes = PREFIX.get(name) or PROBES.get(name)
        if probes is None:
            errors.append(f"{name}：没有对照的程序")
            continue
        for key, source, values in probes:
            message = entry.get("message")
            template = message[key] if isinstance(message, dict) else message
            cls, text = run(source)
            if cls != entry["class"]:
                errors.append(f"{name}：类是 {cls}，不是 {entry['class']}（{source!r}）")
                continue
            if name == "key_missing":
                expected = repr(values["key"])
            else:
                expected = render(template, values)
            ok = text.startswith(expected) if name in PREFIX else text == expected
            if not ok:
                errors.append(f"{name}：CPython 给 {text!r}，表里是 {expected!r}")
    for name in PROBES:
        if name not in MESSAGES:
            errors.append(f"{name}：表里没有这一条")

    def params(form):
        node = ast.parse(f"def f{form}: pass").body[0].args
        return [a.arg for a in node.posonlyargs + node.args + node.kwonlyargs]

    builtins = __builtins__.__dict__ if hasattr(__builtins__, "__dict__") else __builtins__
    for name, forms in SUBSET["builtins"].items():
        for form in forms:
            params(form)
        try:
            real = [p for p in inspect.signature(builtins[name]).parameters]
        except ValueError:
            continue
        if real not in [params(f) for f in forms]:
            errors.append(f"{name}：CPython 的形参是 {real}")
    union = set()
    for typ, methods in SUBSET["methods"].items():
        union |= set(methods)
        for method, forms in methods.items():
            for form in forms:
                params(form)
            try:
                real = [p for p in inspect.signature(getattr(eval(typ), method)).parameters][1:]
            except ValueError:
                continue
            if real not in [params(f) for f in forms]:
                errors.append(f"{typ}.{method}：CPython 的形参是 {real}")
    for typ in SUBSET["methods"]:
        for method in sorted(union):
            if hasattr(eval(typ), method) and method not in SUBSET["methods"][typ]:
                errors.append(f"{typ}：Python 有 {method}，清单没收")
    for name in SUBSET["exceptions"]:
        if not isinstance(builtins.get(name), type):
            errors.append(f"{name}：不是 Python 的异常类")

    for e in errors:
        print(e)
    print(f"{len(errors)} 处不符" if errors else "全部相符")
    return 1 if errors else 0


if __name__ == "__main__":
    if sys.version_info[:2] != (3, 12):
        sys.exit(f"要 CPython 3.12，这是 {sys.version.split()[0]}")
    sys.exit(main())
