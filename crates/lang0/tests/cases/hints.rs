//! 拦下时说用什么代替：模型常写、子集里没有的几类，报错要带上替代的写法。
//! 参考：docs/Subset.md「拦下时说什么」。

use super::support::*;
use croon_eval::host::Language;
use lang0::Lang0;

fn rejected(body: &str) -> String {
    Lang0
        .check(&definition(body), &Contents::default())
        .unwrap_err()
        .to_string()
}

#[test]
fn string_methods_outside_the_subset_say_what_to_use_instead() {
    for method in ["lower", "upper", "casefold", "isdigit", "zfill", "splitlines", "rstrip"] {
        let error = rejected(&format!(
            "def main(text: str) -> str:\n    return text.{method}()\n"
        ));
        assert!(error.contains(&format!("首版不收 .{method}")), "{error}");
        assert!(error.contains("逐字符查"), "{error}");
        assert!(error.contains("lower[upper.index(c)]"), "{error}");
    }
}

#[test]
fn a_typed_local_assignment_says_to_drop_the_type() {
    let error = rejected("def main() -> list[int]:\n    xs: list[int] = []\n    return xs\n");
    assert!(error.contains("首版不收 AnnAssign"), "{error}");
    assert!(error.contains("局部变量不写类型"), "{error}");
}

#[test]
fn set_says_to_use_dict_keys() {
    let error = rejected("def main(xs: list[int]) -> int:\n    return len(set(xs))\n");
    assert!(error.contains("首版不收 set"), "{error}");
    assert!(error.contains("用 dict 的键代替"), "{error}");
}

#[test]
fn an_invented_helper_function_lists_the_real_builtins() {
    let error = rejected("def main(text: str) -> list[str]:\n    return str_split(text)\n");
    assert!(error.contains("str_split"), "{error}");
    assert!(error.contains("内建函数只有"), "{error}");
}

#[test]
fn a_name_without_a_hint_keeps_the_plain_message() {
    let error = rejected("def main(text: str) -> str:\n    return text.encode()\n");
    assert!(error.ends_with("lang0@1 首版不收 .encode"), "{error}");
}
