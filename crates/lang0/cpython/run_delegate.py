"""在 CPython 上跑 Delegate 的 S 与 W0，按脚本给结果，打印推出的 Interface、报出的 Atom 与结束值。

用法：python3 -B run_delegate.py <生成模块目录>；kernel.py 由 `cargo run -p lang0 --example kernel_module` 生成。
"""
import importlib
import json
import pathlib
import shutil
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[2]

work = pathlib.Path(tempfile.mkdtemp())
shutil.copytree(ROOT / "library", work / "library")
for module in ["kernel", "model", "directory"]:
    shutil.copy(pathlib.Path(sys.argv[1]) / f"{module}.py", work / "library" / f"{module}.py")
sys.path[:0] = [str(work), str(HERE)]
import croon  # noqa: E402


def loader(name):
    if name not in croon._LOADED:
        croon._LOADED[name] = croon._Module((work / (name.replace(".", "/") + ".py")).read_text(), name, loader)
    return croon._LOADED[name]


croon.LOADER = loader


def ok(value, attach=None, derived=None, dependencies=[]):
    return {"outcome": {"ok": True, "value": value, "dependencies": dependencies}, "attach": attach or {}, "derived": derived or {}, "accepted": True}


def llm(text, calls):
    return ok({"text": text, "reasoning": None, "calls": calls, "finish": "tool_calls" if calls else "stop",
               "usage": {"input": 1, "output": 1}})


def done_exec(stdout="", snapshot="sha256:lrepo"):
    return ok({"exit": 0, "stdout": stdout, "stderr": "", "timed_out": False, "truncated": False, "snapshot": snapshot}, dependencies=[snapshot])


def rev(n):
    return {"handle": f"h-rev{n}", "spec": "sha256:psrevoke"}


S_OUT = {"summary": "改好了 a.py", "snapshot": "sha256:lrepo2"}
SCRIPTS = {
    "s": ({"0": llm("", [{"id": "c1", "name": "write_file", "input": {"path": "a.py", "content": "fixed\n"}}]),
           "1": done_exec(snapshot="sha256:lrepo2"),
           "2": llm("改好了 a.py", [])},
          {"task": "修好 a.py", "snapshot": "sha256:lrepo"}, {"engine": "port", "exec": "port"}, {}),
    "w0": ({"0": llm("", [{"id": "c1", "name": "delegate", "input": {"name": "fix", "task": "修好 a.py"}}]),
            "1": ok(None, attach={"look": {"handle": "h-slook", "spec": "sha256:pslook"},
                                  "notify": {"handle": "h-snotify", "spec": "sha256:psnotify"},
                                  "correct": {"handle": "h-scorrect", "spec": "sha256:pscorrect"}},
                    derived={"engine": {"handle": "h-sengine", "revoker": rev(5)}, "exec": {"handle": "h-sexec", "revoker": rev(6)}}),
            "2": ok(None, derived={"to": {"handle": "h-sdone", "revoker": rev(7)}}),
            "3": {"arrival": {"port": "done", "sender": "i-s", "via": "h-sdone", "delivery": {"value": {"ok": True, "value": S_OUT, "dependencies": ["sha256:lrepo2"]}, "dependencies": ["sha256:lrepo2"]}, "attach": {}}},
            "4": done_exec(" a.py | 2 +-\n 1 file changed, 1 insertion(+), 1 deletion(-)\n"),
            "5": llm("", [{"id": "c2", "name": "merge", "input": {"name": "fix"}}]),
            "6": llm("已修好 a.py", [])},
           {"task": "修好 a.py", "worker": "sha256:ls", "snapshot": "sha256:lrepo"},
           {"engine": "port", "exec": "port", "instantiate": "port", "back": "port"}, {"done": "inbox"}),
}

out = {}
for name, (results, inp, refs, inboxes) in SCRIPTS.items():
    source = (work / "library" / f"{name}.py").read_text()
    iface = croon.interface(source, loader)
    croon.DRIVER = croon.Driver(results)
    mod = importlib.import_module(f"library.{name}")
    args = dict(inp)
    for k in refs:
        args[k] = croon.Ref({"port": k}, iface["requires"][k])
    for k in inboxes:
        args[k] = croon.InboxRef(k)
    try:
        delivery = mod.main(**args)
        end = {"ok": True, "value": delivery.value, "dependencies": delivery.dependencies}
    except croon.Failure as e:
        end = {"ok": False, "error": {"kind": e.kind_, "message": str(e), "data": e.data_}, "dependencies": []}
    out[name] = {"interface": iface, "atoms": croon.DRIVER.atoms, "end": end}

json.dump(out, sys.stdout, ensure_ascii=False, indent=1)
print()
