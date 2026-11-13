"""生成 crates/lang0/src/eval/unicode.rs：Unicode 15.0 的三张表，取自 CPython 3.12。

用法：python3 -B crates/lang0/cpython/unicode_tables.py > crates/lang0/src/eval/unicode.rs
参考：docs/Lang0/Evaluator.md（文本）。
"""
import sys
import unicodedata

assert unicodedata.unidata_version == "15.0.0", unicodedata.unidata_version


def ranges(pred):
    out, start = [], None
    for c in range(sys.maxunicode + 2):
        ok = c <= sys.maxunicode and pred(chr(c))
        if ok and start is None:
            start = c
        if not ok and start is not None:
            out.append((start, c - 1))
            start = None
    return out


def table(name, doc, rs):
    body = ", ".join(f"(0x{a:x}, 0x{b:x})" for a, b in rs)
    return f"/// {doc}\npub static {name}: &[(u32, u32)] = &[{body}];\n"


print("//! 由 crates/lang0/cpython/unicode_tables.py 从 CPython 3.12（Unicode 15.0）生成，不要手改。")
print("//! 参考：docs/Lang0/Evaluator.md（文本）。\n")
print(table("NOT_PRINTABLE", "`str.isprintable()` 为假的字符：repr 要转义。", ranges(lambda c: not c.isprintable())))
print(table("SPACE", "`str.isspace()` 为真的字符：split、strip 与转数时认的空白。", ranges(str.isspace)))
print(table("DECIMAL", "`str.isdecimal()` 为真的字符，每段是连续的 0 到 9。", ranges(str.isdecimal)))
