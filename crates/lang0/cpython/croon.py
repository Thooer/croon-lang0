"""CPython 上的 `croon` 参考实现：只用来产出预期与作差分的参照，不是 Lang0。

推导按 docs/README.md 的「Interface」与「推导」，在 AST 上做，不对标注求值。
Atom 不真的交给 Kernel：交出与取由 `Driver` 按脚本给结果，并按 Lang0 的键记下。
"""
import ast
import copy
import inspect
import json
import sys

# ---- 运行时 ----


class Failure(Exception):
    def __init__(self, kind, message, data=None, dependencies=[]):
        super().__init__(message)
        self.kind_, self.data_ = kind, {"error": data, "dependencies": copy.deepcopy(dependencies)}


class Limit(BaseException):
    pass


def kind(e):
    if isinstance(e, Failure):
        return e.kind_
    if isinstance(e, Limit):
        return "limit"
    return type(e).__name__


def data(e):
    return e.data_ if isinstance(e, Failure) else None


class Driver:
    """一个 Instance 的会话：按键给结果，记下报出的 Atom。"""

    def __init__(self, results, contents=None):
        self.results, self.contents, self.n, self.atoms = results, contents or {}, 0, []
        self.consumed_keys = []

    def atom(self, body):
        key = str(self.n)
        self.n += 1
        self.atoms.append({"key": key, **json.loads(json.dumps(body))})
        if key not in self.results:
            raise SystemExit(f"键 {key} 没有脚本里的结果：{body}")
        r = self.results[key]
        self.consumed_keys.append(key)
        return r


DRIVER = None


class Ref:
    def __init__(self, target, spec=None):
        self.target, self.spec = target, spec


def send(ref, value, dependencies=[], attach={}):
    r = DRIVER.atom({"send": {"to": ref.target, "delivery": {"value": value, "dependencies": dependencies},
                              "attach": {k: a.target for k, a in attach.items()}}})
    if not r["outcome"]["ok"]:
        e = r["outcome"]["error"]
        raise Failure(e["kind"], e["message"], e["data"], r["outcome"]["dependencies"])
    return Receipt(r)


class Delivery:
    def __init__(self, value, dependencies):
        self.value, self.dependencies = value, dependencies


def deliver(value, dependencies):
    return Delivery(value, dependencies)


def read_json(id):
    return _read(id, "read_json")


def read_text(id):
    return _read(id, "read_text")


def _read(id, name):
    entry = DRIVER.contents.get(id)
    if entry is None or (entry.get("key") is not None and entry["key"] not in DRIVER.consumed_keys):
        raise Failure("use", f"croon.{name}: 读不到 {id} 或内容类型不符")
    return copy.deepcopy(entry["value"])


class Derived:
    def __init__(self, d):
        self.handle, self.revoker = d["handle"], Ref({"handle": d["revoker"]["handle"]})


class Receipt:
    def __init__(self, r):
        self.value = r["outcome"]["value"]
        self.dependencies = copy.deepcopy(r["outcome"]["dependencies"])
        self.attach = {k: Ref({"handle": a["handle"]}) for k, a in r["attach"].items()}
        self.derived = {k: Derived(d) for k, d in r["derived"].items()}


class Arrival:
    def __init__(self, a):
        self.port, self.sender, self.via, self.value = a["port"], a["sender"], a["via"], a["delivery"]["value"]
        self.dependencies = copy.deepcopy(a["delivery"]["dependencies"])
        self.attach = {k: Ref({"handle": x["handle"]}) for k, x in a["attach"].items()}


class InboxRef:
    def __init__(self, name):
        self.name = name


def receive(ports, via=None, sender=None, timeout=None):
    ports = ports if isinstance(ports, list) else [ports]
    filt = {"ports": [p.name for p in ports]}
    for k, v in (("sender", sender), ("via", via)):
        if v is not None:
            filt[k] = v if isinstance(v, list) else [v]
    if timeout is not None:
        filt["timeout"] = timeout
    r = DRIVER.atom({"receive": filt})
    return None if r["arrival"] is None else Arrival(r["arrival"])


# 只在标注里出现；运行时 main 拿到的是 Ref 与 InboxRef。
def Port(spec, attach=None, doc=None):
    return ("Port", spec, attach, doc)


class Inbox:
    """只在标注里出现；是类，`Inbox | None` 在 CPython 上才求值得了。"""

    def __init__(self, p):
        self.p = p


def Schema(s):
    return ("Schema", s)


class Tool:
    def __init__(self, f):
        self.f, self.name = f, f.__name__
        node = _function_node(f)
        desc, args = _docstring(ast.get_docstring(node, clean=True))
        self.spec = {"name": self.name, "description": desc, "input": _input(node, args, _module_of(f))}

    def __call__(self, input):
        return self.f(**input)


def tool(f):
    return Tool(f)


# ---- 推导 ----


def _docstring(doc):
    if not doc:
        return "", {}
    lines = doc.split("\n")
    if "Args:" not in [l.strip() for l in lines]:
        return doc.strip(), {}
    i = [l.strip() for l in lines].index("Args:")
    desc, args, cur, ind = "\n".join(lines[:i]).strip(), {}, None, None
    for l in lines[i + 1:]:
        if not l.strip():
            continue
        depth = len(l) - len(l.lstrip())
        if cur is None or depth <= ind:
            name, _, text = l.strip().partition(":")
            cur, ind = name.strip(), depth
            args[cur] = text.strip()
        else:
            args[cur] = (args[cur] + " " + l.strip()).strip()
    return desc, args


class _Module:
    """一个模块的顶层绑定：名字 → 赋值的 AST，或 import 来源。"""

    def __init__(self, source, name, loader):
        self.tree, self.name, self.loader, self.bind = ast.parse(source), name, loader, {}
        for s in self.tree.body:
            if isinstance(s, ast.Assign) and len(s.targets) == 1 and isinstance(s.targets[0], ast.Name):
                self.bind[s.targets[0].id] = ("expr", s.value)
            elif isinstance(s, ast.ImportFrom):
                for a in s.names:
                    self.bind[a.asname or a.name] = ("from", s.module, a.name)
            elif isinstance(s, ast.FunctionDef):
                self.bind[s.name] = ("def", s)

    def lookup(self, name):
        b = self.bind.get(name)
        if b and b[0] == "from":
            if b[1] in ("typing", "croon"):
                return ("builtin", b[2])
            return self.loader(b[1]).lookup(b[2])
        if b and b[0] == "expr":
            return ("expr", b[1], self)
        return ("builtin", name)


def _literal(node):
    return ast.literal_eval(node)


def _schema(node, mod):
    if isinstance(node, ast.Constant) and node.value is None:
        return {"type": "null"}
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.BitOr):
        return {"anyOf": _flat_or(node, mod)}
    if isinstance(node, ast.Subscript):
        head = _resolve(node.value, mod)
        args = node.slice.elts if isinstance(node.slice, ast.Tuple) else [node.slice]
        if head == "list":
            return {"type": "array", "items": _schema(args[0], mod)}
        if head == "dict":
            return {"type": "object", "additionalProperties": _schema(args[1], mod)}
        if head == "Literal":
            return {"enum": [_literal(a) for a in args]}
        if head == "Annotated":
            return {**_schema(args[0], mod), "description": _literal(args[1])}
    if isinstance(node, ast.Name):
        r = mod.lookup(node.id)
        if r[0] == "builtin":
            return {"str": {"type": "string"}, "int": {"type": "integer"}, "float": {"type": "number"},
                    "bool": {"type": "boolean"}, "Any": {}}[r[1]]
        return _value_schema(r[1], r[2])
    raise SystemExit(f"推不出 Schema：{ast.dump(node)}")


def _flat_or(node, mod):
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.BitOr):
        return _flat_or(node.left, mod) + _flat_or(node.right, mod)
    return [_schema(node, mod)]


def _resolve(node, mod):
    return mod.lookup(node.id)[1] if isinstance(node, ast.Name) else None


def _value_schema(expr, mod):
    """顶层赋值右边是 TypedDict(...) 或 Schema(...)。"""
    f = _resolve(expr.func, mod)
    if f == "TypedDict":
        props, req = {}, []
        for k, v in zip(expr.args[1].keys, expr.args[1].values):
            name = _literal(k)
            if isinstance(v, ast.Subscript) and _resolve(v.value, mod) == "NotRequired":
                props[name] = _schema(v.slice, mod)
            else:
                props[name] = _schema(v, mod)
                req.append(name)
        return {"type": "object", "properties": props, "required": req, "additionalProperties": False}
    if f == "Schema":
        return _literal(expr.args[0])
    raise SystemExit(f"{f} 不是值的类型")


def _port(node, mod):
    """Port 类型的标注 → ("requires" | "provides", Port spec)；不是 Port 就是 None。"""
    kind_ = "requires"
    if isinstance(node, ast.Call) and _resolve(node.func, mod) == "Inbox":
        kind_, node = "provides", node.args[0]
    if not isinstance(node, ast.Name):
        return None
    r = mod.lookup(node.id)
    if r[0] != "expr" or not isinstance(r[1], ast.Call) or _resolve(r[1].func, r[2]) != "Port":
        return None
    call, m = r[1], r[2]
    kw = {k.arg: k.value for k in call.keywords}
    if isinstance(call.args[0], ast.Dict):
        return kind_, _literal(call.args[0])
    spec = {"description": _literal(kw["doc"]) if "doc" in kw else "", "in": _schema(call.args[0], m),
            "attach": {"additionalProperties": False}}
    return kind_, spec


def _input(node, args, mod):
    props, req = {}, []
    params = node.args.args
    defaults = [None] * (len(params) - len(node.args.defaults)) + node.args.defaults
    for p, d in zip(params, defaults):
        if _port(p.annotation, mod):
            continue
        s = _schema(p.annotation, mod)
        if p.arg in args:
            s = {**s, "description": args[p.arg]}
        props[p.arg] = s
        if d is None:
            req.append(p.arg)
    return {"type": "object", "properties": props, "required": req, "additionalProperties": False}


def interface(source, loader):
    mod = _Module(source, "__main__", loader)
    main = mod.bind["main"][1]
    desc, args = _docstring(ast.get_docstring(main, clean=True))
    provides, requires = {}, {}
    for p in main.args.args:
        r = _port(p.annotation, mod)
        if r:
            (provides if r[0] == "provides" else requires)[p.arg] = r[1]
    return {"description": desc, "signature": {"in": _input(main, args, mod), "out": _schema(main.returns, mod)},
            "provides": provides, "requires": requires}


_SOURCES = {}


def _function_node(f):
    src = inspect.getsource(f)
    lines = src.split("\n")
    pad = len(lines[0]) - len(lines[0].lstrip())
    return ast.parse("\n".join(l[pad:] for l in lines)).body[0]


def _module_of(f):
    return LOADER(f.__module__)


LOADER = None


_LOADED = {}
