//! import 的检查经 Read，不从宿主路径旁路读内容。
//! 参考：docs/README.md（模块）；croon:docs/Kernel/Language.md「实现」。

use super::support::*;
use croon_eval::host::Language;
use lang0::Lang0;
use serde_json::json;

fn check_with_code(body: &str, code: String, contents: &Contents) -> Result<(), String> {
    let mut d = definition(body);
    d["body"]["modules"] = json!(code);
    Lang0.check(&d, contents).map_err(|error| error.to_string())
}

#[test]
fn imports_check_namespace_packages_regular_packages_and_relative_modules() {
    for init in [
        None,
        Some("from . import leaf\n"),
        Some("from .leaf import N\n"),
    ] {
        let mut read = Contents::default();
        let leaf = read.put_source("from croon import Schema\nN = Schema({'type': 'null'})\n");
        let types = read.put_source("from .leaf import N as Result\n");
        let mut package = json!({"leaf.py": leaf, "types.py": types});
        if let Some(init) = init {
            package["__init__.py"] = read.put_source(init);
        }
        let pkg = read.put_json(package);
        let root = read.put_json(json!({"pkg": {"dir": pkg}}));
        for body in [
            "from pkg.types import Result\ndef main() -> Result:\n    pass\n",
            "import pkg.types\ndef main() -> pkg.types.Result:\n    pass\n",
            "import pkg.types as t\ndef main() -> t.Result:\n    pass\n",
            "from pkg import types as t\ndef main() -> t.Result:\n    pass\n",
        ] {
            check_with_code(body, root.clone(), &read).unwrap();
        }
    }
}

#[test]
fn relative_imports_can_walk_to_parent_packages_but_not_outside() {
    let mut read = Contents::default();
    let sub = read.code(&[("types.py", "from ..leaf import N\n")]);
    let leaf = read.put_source("from croon import Schema\nN = Schema({'type': 'null'})\n");
    let pkg = read.put_json(json!({"sub": {"dir": sub}, "leaf.py": leaf}));
    let root = read.put_json(json!({"pkg": {"dir": pkg}}));
    check_with_code(
        "from pkg.sub.types import N\ndef main() -> N:\n    pass\n",
        root.clone(),
        &read,
    )
    .unwrap();
    let error = check_with_code(
        "from .pkg import leaf\ndef main() -> None:\n    pass\n",
        root,
        &read,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("相对 import"), "{error}");
}

#[test]
fn imported_code_and_package_initializers_receive_the_same_checks() {
    for source in [
        "print('bad')\n",
        "class X:\n    pass\n",
        "def f(e):\n    return type(e).__name__\n",
    ] {
        let mut read = Contents::default();
        let root = read.code(&[("bad.py", source)]);
        let error = check_with_code("import bad\ndef main() -> None:\n    pass\n", root, &read)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("/body/modules/bad.py:"), "{error}");
        let init = read.put_source(source);
        let child = read.put_source("value = 1\n");
        let package = read.put_json(json!({"__init__.py": init, "child.py": child}));
        let root = read.put_json(json!({"bad": {"dir": package}}));
        let error = check_with_code(
            "import bad.child\ndef main() -> None:\n    pass\n",
            root,
            &read,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.starts_with("/body/modules/bad/__init__.py:"),
            "{error}"
        );
    }
}

#[test]
fn content_read_capability_and_import_whitelist_are_enforced() {
    let mut read = Contents::default();
    let root = read.code(&[("secret.py", "x = 1\n")]);
    read.bytes.clear();
    let error = check_with_code(
        "import secret\ndef main() -> None:\n    pass\n",
        root,
        &read,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("读不到源码"), "{error}");
    for (body, want) in [
        (
            "import os\ndef main() -> None:\n    pass\n",
            "/body:1:1: 模块 os 不在 croon、白名单或模块目录里",
        ),
        (
            "from typing import cast\ndef main() -> None:\n    pass\n",
            "/body:1:1: lang0@1 首版不收 typing.cast",
        ),
        (
            "from croon import Llm\ndef main() -> None:\n    pass\n",
            "/body:1:1: lang0@1 首版不收 croon.Llm",
        ),
        (
            "from typing import *\ndef main() -> None:\n    pass\n",
            "/body:1:1: lang0@1 首版不收 import *",
        ),
        (
            "import typing\ndef main() -> None:\n    typing.cast(None)\n",
            "/body:3:5: lang0@1 首版不收 .cast",
        ),
    ] {
        assert_eq!(
            Lang0
                .check(&definition(body), &read)
                .unwrap_err()
                .to_string(),
            want,
            "{body}"
        );
    }
}

#[test]
fn code_roots_cannot_shadow_reserved_modules_even_when_unused() {
    for name in ["croon.py", "typing.py", "croon", "typing"] {
        let mut read = Contents::default();
        let code = read.code(&[(name, "x = 1\n")]);
        let error = check_with_code("def main() -> None:\n    pass\n", code, &read)
            .unwrap_err()
            .to_string();
        assert!(error.contains("改名"), "{error}");
    }
}

#[test]
fn symlink_imports_are_rejected_without_following_the_target() {
    let mut read = Contents::default();
    for tree in [
        json!({"a.py": {"link": "b.py"}}),
        json!({"a": {"link": "b"}}),
    ] {
        let code = read.put_json(tree);
        let error = check_with_code("import a\ndef main() -> None:\n    pass\n", code, &read)
            .unwrap_err()
            .to_string();
        assert!(error.contains("首版不收 import 经符号链接"), "{error}");
    }
}

#[test]
fn recursive_import_graphs_terminate_but_cyclic_annotations_fail() {
    let mut read = Contents::default();
    let code = read.code(&[
        (
            "a.py",
            "import b\nfrom croon import Schema\nN = Schema({'type': 'null'})\n",
        ),
        ("b.py", "import a\n"),
    ]);
    check_with_code("from a import N\ndef main() -> N:\n    pass\n", code, &read).unwrap();
    let code = read.code(&[("a.py", "from b import N\n"), ("b.py", "from a import N\n")]);
    let error = check_with_code("from a import N\ndef main() -> N:\n    pass\n", code, &read)
        .unwrap_err()
        .to_string();
    assert!(error.contains("成环"), "{error}");
}
