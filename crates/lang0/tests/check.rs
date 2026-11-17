//! 构造检查：用最小源码检验子集、绑定与 Interface 的规则。
//! 参考：docs/Subset.md（拦下时说什么）；docs/README.md（Interface）。

#[path = "cases/imports.rs"]
mod imports;
#[path = "cases/inference.rs"]
mod inference;
#[path = "cases/surface.rs"]
mod surface;
mod support;

use croon_eval::host::Language;
use lang0::Lang0;
use serde_json::json;
use support::*;

#[test]
fn subset_print_class_and_dunder_use_documented_messages() {
    for (extra, message) in [
        ("\nprint('debug')\n", "用 return 或交出代替；调试看 Journal"),
        (
            "\nclass X:\n    pass\n",
            "用 `X = TypedDict(\"X\", {...})` 与函数",
        ),
        (
            "\ndef inspect_error(e):\n    return type(e).__name__\n",
            "失败的种类用 `croon.kind(e)`",
        ),
    ] {
        let body = format!("def main() -> None:\n    pass\n{extra}");
        let error = Lang0
            .check(&definition(&body), &Contents::default())
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("/body:") && error.ends_with(message),
            "{error}"
        );
    }
}

#[test]
fn parameter_and_local_bindings_shadow_blocked_builtins() {
    for body in [
        "def helper(exec, print):\n    exec()\n    print()\ndef main() -> None:\n    return None\n",
        "def main() -> None:\n    print = str\n    print(1)\n    exec = str\n    exec(2)\n",
        "def main() -> None:\n    print(1)\n    print = str\n",
        "def main() -> None:\n    print = str\n    def nested():\n        print(1)\n",
    ] {
        Lang0
            .check(&definition(body), &Contents::default())
            .unwrap();
    }
}

#[test]
fn undefined_and_comprehension_names_follow_python_scopes() {
    Lang0
        .check(
            &definition("def main() -> None:\n    xs = [(x, y) for x in [1] for y in [x]]\n"),
            &Contents::default(),
        )
        .unwrap();
    for body in [
        "def main() -> None:\n    return missing\n",
        "def main() -> None:\n    xs = [x for x in [1]]\n    x\n",
        "def main() -> None:\n    xs = [x for x in x]\n",
    ] {
        let e = Lang0
            .check(&definition(body), &Contents::default())
            .unwrap_err()
            .to_string();
        assert!(e.contains("没有绑定"), "{e}");
    }
}

#[test]
fn unsupported_constructs_and_expression_details_are_rejected() {
    for (body, expected) in [
        ("def main(*args) -> None:\n    pass\n", "函数参数"),
        ("def main(x, /) -> None:\n    pass\n", "函数参数"),
        (
            "def main() -> None:\n    try:\n        pass\n    except Exception:\n        pass\n    else:\n        pass\n",
            "try else",
        ),
        (
            "def main() -> None:\n    raise ValueError() from None\n",
            "raise from",
        ),
        ("def main() -> None:\n    f'{1!r}'\n", "f-string"),
        ("def main() -> None:\n    f'{1:>8}'\n", "f-string"),
        (
            "def main() -> None:\n    '{x.__class__}'.format(x=1)\n",
            "str.format",
        ),
        ("def main() -> None:\n    'x'.upper()\n", ".upper"),
        ("def main() -> None:\n    [].get(0)\n", ".get"),
        ("def main() -> None:\n    1 | 2\n", "BitOr"),
        ("X = str | None\ndef main() -> None:\n    pass\n", "BitOr"),
        ("def main() -> None:\n    b'x'\n", "bytes"),
        ("def main() -> None:\n    1j\n", "complex"),
        ("def main() -> None:\n    ...\n", "用 pass 代替"),
        ("def main(x: int) -> None:\n    x is 1\n", "用 == 比较值"),
        (
            "def main(x: str) -> None:\n    'a' is not x\n",
            "用 == 比较值",
        ),
        ("def main(x: int) -> None:\n    '%d' % x\n", "代替 %"),
        ("def main(x: int) -> None:\n    f'{x}%d' % x\n", "代替 %"),
        (
            "@str\ndef helper():\n    pass\ndef main() -> None:\n    pass\n",
            "装饰器",
        ),
    ] {
        let error = Lang0
            .check(&definition(body), &Contents::default())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(expected) && error.starts_with("/body:"),
            "expected {expected}: {error}"
        );
    }
}

#[test]
fn python312_fstrings_parse_and_newer_syntax_fails() {
    Lang0
        .check(
            &definition("def main() -> None:\n    x = {'a': 1}\n    f'{x['a']}'\n"),
            &Contents::default(),
        )
        .unwrap();
    let body = "def main[T = int]() -> None:\n    pass\n";
    let error = Lang0
        .check(&definition(body), &Contents::default())
        .unwrap_err()
        .to_string();
    assert!(error.contains("3.13"), "{error}");
}

#[test]
fn dunder_identifiers_are_rejected_but_strings_are_data() {
    for body in [
        "def main() -> None:\n    __file__\n",
        "def helper(__name__):\n    pass\ndef main() -> None:\n    pass\n",
        "def main() -> None:\n    __import__ = str\n",
        "def main() -> None:\n    dict(__x__=1)\n",
    ] {
        let error = Lang0
            .check(&definition(body), &Contents::default())
            .unwrap_err()
            .to_string();
        assert!(error.ends_with("首版不收 dunder 名字"), "{error}");
    }
    Lang0
        .check(
            &definition("def main() -> None:\n    x = '__name__'\n"),
            &Contents::default(),
        )
        .unwrap();
}

#[test]
fn accepted_statement_and_expression_forms_remain_available() {
    let source = r#"def helper(a=1):
    return a
def main() -> None:
    a, b = [1, 2]
    a += b
    xs = [a, *[b]]
    d = {"a": a, **{"b": b}}
    helper(*[a], **{})
    for i in xs:
        if i < 0:
            continue
        elif i >= 2:
            break
    else:
        pass
    while False:
        pass
    else:
        pass
    try:
        if not (a is None) and 0 < a <= b or a not in xs:
            raise ValueError('bad')
    except (ValueError, TypeError) as e:
        str(e)
    finally:
        xs.clear()
    '{} {name} {{}}'.format(a, name='ok')
    a is None or xs is not None or xs is xs
    a % 3
    {'k': a}.clear()
    'ab'.index('b')
    (a, b).index(b)
    x = a if a != b else -b
"#;
    Lang0
        .check(&definition(source), &Contents::default())
        .unwrap();
}

#[test]
fn syntax_errors_and_unicode_source_locations_are_reported() {
    let error = Lang0
        .check(
            &definition("def main( -> None:\n    pass\n"),
            &Contents::default(),
        )
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("/body:1:"), "{error}");
    let error = Lang0
        .check(
            &definition("def main() -> None:\n    '中文'; print(1)\n"),
            &Contents::default(),
        )
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("/body:2:11:"), "{error}");
}

#[test]
fn format_fields_use_python_identifiers_including_unicode() {
    for text in ["{} {name} {{literal}}", "{名字}", "{a\u{0301}}", "{class}"] {
        let body = format!("def main() -> None:\n    '{text}'.format()\n");
        Lang0
            .check(&definition(&body), &Contents::default())
            .unwrap();
    }
    for text in [
        "{0}",
        "{a.b}",
        "{a[0]}",
        "{²}",
        "{name!r}",
        "{name:>8}",
        "{ x }",
    ] {
        let body = format!("def main() -> None:\n    '{text}'.format()\n");
        let error = Lang0
            .check(&definition(&body), &Contents::default())
            .unwrap_err()
            .to_string();
        assert!(error.contains("str.format"), "{error}");
    }
}

#[test]
fn python_semantic_syntax_errors_are_not_accepted_as_programs() {
    for (body, want) in [
        (
            "def helper(a, a):\n    pass\ndef main() -> None:\n    pass\n",
            "/body:1:15: 函数参数名重复",
        ),
        (
            "def main() -> None:\n    dict(a=1, a=2)\n",
            "/body:2:17: 调用的关键字参数名重复",
        ),
        (
            "def main() -> None:\n    *a = [1]\n",
            "/body:2:5: 赋值目标须是名字",
        ),
        (
            "def main() -> None:\n    *a, *b = [1, 2]\n",
            "/body:2:9: 同一层解包只能有一个星号目标",
        ),
        (
            "def main() -> None:\n    a, b = [1, 2]\n    (a, b) += (1, 2)\n",
            "/body:3:5: Invalid augmented assignment target",
        ),
    ] {
        let error = Lang0
            .check(&definition(body), &Contents::default())
            .unwrap_err()
            .to_string();
        assert!(error.starts_with(want), "{body}: {error}");
    }
}

/// 类型位置只收类型的写法；运行时的下标也按类型位置查。
#[test]
fn type_positions_accept_only_type_forms() {
    for body in [
        "def main() -> None:\n    x = list[1 | 2]\n",
        "def main() -> None:\n    x = list[len('a') | 1]\n",
        "def main() -> list[1]:\n    return []\n",
        "def main() -> list[len('a')]:\n    return []\n",
        "from typing import TypedDict\nT = TypedDict('T', {'a': 1 | 2})\ndef main() -> None:\n    pass\n",
    ] {
        let error = Lang0
            .check(&definition(body), &Contents::default())
            .unwrap_err()
            .to_string();
        assert!(error.contains("首版不收 类型位置上的"), "{body}: {error}");
    }
    let error = Lang0
        .check(
            &definition(
                "from typing import Literal\ndef main() -> Literal[len('a')]:\n    return None\n",
            ),
            &Contents::default(),
        )
        .unwrap_err()
        .to_string();
    assert!(error.ends_with("Literal 的参数须是字面量"), "{error}");
}

/// 没有返回类型、Interface 不等，都指到 def main 那一行。
#[test]
fn main_errors_point_at_def_main() {
    let error = Lang0
        .check(
            &definition("x = 1\ndef main():\n    pass\n"),
            &Contents::default(),
        )
        .unwrap_err()
        .to_string();
    assert!(
        error.starts_with("/body:2:1: main 的返回值须标类型"),
        "{error}"
    );
    let mut d = definition("x = 1\ndef main() -> None:\n    pass\n");
    d["interface"]["description"] = json!("不一样");
    let error = Lang0
        .check(&d, &Contents::default())
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("/body:2:1: /interface:"), "{error}");
}
