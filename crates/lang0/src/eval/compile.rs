//! 源码编译成栈式 IR：每个函数、模块与推导式一份 Code，帧与续体是求值器的数据。
//! 求值顺序照 Python：键的序号就是效果的先后。
//! 参考：docs/Engineering.md（选择、降）；docs/Evaluator.md（键）；docs/Subset.md（清单）。

use std::collections::BTreeMap;
use std::rc::Rc;

use num_bigint::BigInt;
use num_traits::Num;

use crate::ast::*;
use crate::modules::Program;
use crate::symbols::{self, Binding, Imports};

use super::value::{Form, Value};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bin {
    Add,
    Sub,
    Mult,
    Div,
    FloorDiv,
    Mod,
    BitOr,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmp {
    Eq,
    NotEq,
    Lt,
    LtE,
    Gt,
    GtE,
    In,
    NotIn,
    Is,
    IsNot,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Un {
    Not,
    Neg,
}

/// import 之后推什么：模块本身，或它的一个名字。
#[derive(Clone, Debug)]
pub enum Push {
    Module(String),
    Attr(String, String),
}

#[derive(Clone, Debug)]
pub struct ImportOp {
    /// 要先执行完的模块，按先后。
    pub chain: Vec<String>,
    pub push: Push,
}

#[derive(Clone, Debug)]
pub enum Op {
    Const(usize),
    Load(usize),
    Store(usize),
    Unbind(usize),
    LoadGlobal(usize),
    StoreGlobal(usize),
    UnbindGlobal(usize),
    Pop,
    Dup,
    Dup2,
    Rot2,
    Rot3,
    BuildList(usize),
    BuildTuple(usize),
    ListAppend,
    /// 栈顶的可迭代对象并进下面的 list；`call` 是调用里的 `*`。
    ListExtend {
        call: bool,
    },
    ListToTuple,
    BuildDict,
    /// 栈顶的键与值放进下面的 dict；`call` 是调用里的关键字参数，键已有就报 multiple_kw_values。
    DictInsert {
        call: bool,
    },
    /// 栈顶的映射并进下面的 dict；`call` 是调用里的 `**`。
    DictMerge {
        call: bool,
    },
    /// 推导式：栈顶的值放进下面第 n 个位置上的 list 或 dict。
    CompAppend(usize),
    CompInsert(usize),
    Binary(Bin),
    InPlace(Bin),
    Unary(Un),
    Compare(Cmp),
    GetAttr(usize),
    StoreAttr(usize),
    GetItem,
    StoreItem,
    Call {
        pos: usize,
        kw: Vec<usize>,
    },
    /// 栈上是函数、位置参数的 list、关键字参数的 dict。
    CallEx,
    MakeFunction {
        code: usize,
        defaults: usize,
    },
    GetIter,
    ForIter(usize),
    Jump(usize),
    JumpIfFalse(usize),
    JumpIfFalseOrPop(usize),
    JumpIfTrueOrPop(usize),
    Return,
    Raise,
    Reraise,
    SetupExcept(usize),
    SetupFinally(usize),
    PopBlock,
    EnterHandler,
    ExitHandler,
    LoadExc,
    CheckMatch,
    EndFinally,
    Unpack {
        n: usize,
        star: Option<usize>,
    },
    FormatValue,
    BuildString(usize),
    Import(usize),
    /// 每一轮循环计一步。
    Tick,
}

pub struct Code {
    pub name: String,
    pub qualname: String,
    /// 所在模块：import 名，body 是空串。
    pub module: String,
    pub params: Vec<String>,
    pub nlocals: usize,
    pub slots: Vec<String>,
    pub ops: Vec<Op>,
    pub spans: Vec<Span>,
    pub consts: Vec<Value>,
    pub names: Vec<String>,
    pub children: Vec<Rc<Code>>,
    /// 子 Code 的 free 各取自本帧的哪个槽位。
    pub cellmaps: Vec<Vec<usize>>,
    pub imports: Vec<ImportOp>,
    /// 推导式：不计调用层数，与 CPython 3.12 内联推导式一致。
    pub comprehension: bool,
    /// 函数的定义：croon.tool 从它推出说明。
    pub def: Option<FunctionDef>,
    pub is_module: bool,
}

#[derive(PartialEq)]
enum Kind {
    Module,
    Function,
    Comprehension,
}

struct Scope {
    kind: Kind,
    locals: Vec<String>,
    frees: Vec<String>,
    code: Code,
}

enum Ctx {
    Loop {
        for_: bool,
        top: usize,
        breaks: Vec<usize>,
    },
    Except,
    Finally(Vec<Stmt>),
    Handler,
}

enum Name {
    Slot(usize),
    Global(usize),
}

struct Compiler<'a> {
    program: &'a Program,
    module: String,
    imports: &'a Imports,
    scopes: Vec<Scope>,
    ctx: Vec<Ctx>,
}

pub fn module(program: &Program, name: &str) -> Rc<Code> {
    let m = &program.modules[name];
    let mut c = Compiler {
        program,
        module: name.to_owned(),
        imports: &m.imports,
        scopes: vec![],
        ctx: vec![],
    };
    c.scopes.push(Scope {
        kind: Kind::Module,
        locals: vec![],
        frees: vec![],
        code: new_code(name, "<module>", "<module>"),
    });
    c.scopes.last_mut().unwrap().code.is_module = true;
    for s in &m.ast.body {
        c.stmt(s);
    }
    let none = c.konst(Value::None);
    c.emit(Op::Const(none), 0..0);
    c.emit(Op::Return, 0..0);
    Rc::new(c.scopes.pop().unwrap().code)
}

fn new_code(module: &str, name: &str, qualname: &str) -> Code {
    Code {
        name: name.into(),
        qualname: qualname.into(),
        module: module.into(),
        params: vec![],
        nlocals: 0,
        slots: vec![],
        ops: vec![],
        spans: vec![],
        consts: vec![],
        names: vec![],
        children: vec![],
        cellmaps: vec![],
        imports: vec![],
        comprehension: false,
        def: None,
        is_module: false,
    }
}

fn bin_op(op: &str) -> Bin {
    match op {
        "Add" => Bin::Add,
        "Sub" => Bin::Sub,
        "Mult" => Bin::Mult,
        "Div" => Bin::Div,
        "FloorDiv" => Bin::FloorDiv,
        "Mod" => Bin::Mod,
        "BitOr" => Bin::BitOr,
        other => panic!("构造检查放过了运算 {other}"),
    }
}

fn cmp_op(op: &str) -> Cmp {
    match op {
        "Eq" => Cmp::Eq,
        "NotEq" => Cmp::NotEq,
        "Lt" => Cmp::Lt,
        "LtE" => Cmp::LtE,
        "Gt" => Cmp::Gt,
        "GtE" => Cmp::GtE,
        "In" => Cmp::In,
        "NotIn" => Cmp::NotIn,
        "Is" => Cmp::Is,
        "IsNot" => Cmp::IsNot,
        other => panic!("构造检查放过了比较 {other}"),
    }
}

/// 整数字面量的文本：十进制，或 0x、0o、0b 开头，可带下划线。
pub fn int_literal(text: &str) -> BigInt {
    let t: String = text.chars().filter(|c| *c != '_').collect();
    let lower = t.to_ascii_lowercase();
    let (radix, digits) = match lower.get(..2) {
        Some("0x") => (16, &lower[2..]),
        Some("0o") => (8, &lower[2..]),
        Some("0b") => (2, &lower[2..]),
        _ => (10, &lower[..]),
    };
    BigInt::from_str_radix(digits, radix).expect("解析器给出的整数字面量")
}

impl Compiler<'_> {
    fn scope(&mut self) -> &mut Scope {
        self.scopes.last_mut().unwrap()
    }
    fn code(&mut self) -> &mut Code {
        &mut self.scope().code
    }
    fn emit(&mut self, op: Op, span: Span) -> usize {
        let c = self.code();
        c.ops.push(op);
        c.spans.push(span);
        c.ops.len() - 1
    }
    fn here(&mut self) -> usize {
        self.code().ops.len()
    }
    fn patch(&mut self, at: usize) {
        let to = self.here();
        match &mut self.code().ops[at] {
            Op::Jump(t)
            | Op::JumpIfFalse(t)
            | Op::JumpIfFalseOrPop(t)
            | Op::JumpIfTrueOrPop(t)
            | Op::ForIter(t)
            | Op::SetupExcept(t)
            | Op::SetupFinally(t) => *t = to,
            other => panic!("不能补跳转的目标：{other:?}"),
        }
    }
    fn konst(&mut self, v: Value) -> usize {
        let c = self.code();
        c.consts.push(v);
        c.consts.len() - 1
    }
    fn name_index(&mut self, name: &str) -> usize {
        let c = self.code();
        match c.names.iter().position(|n| n == name) {
            Some(i) => i,
            None => {
                c.names.push(name.into());
                c.names.len() - 1
            }
        }
    }

    /// 名字在当前作用域里的位置：局部或 free 是槽位，其余是全局。
    fn resolve(&mut self, name: &str) -> Name {
        let top = self.scopes.len() - 1;
        if self.scopes[top].kind == Kind::Module {
            return Name::Global(self.name_index(name));
        }
        if let Some(i) = slot_of(&self.scopes[top], name) {
            return Name::Slot(i);
        }
        let found = (0..top)
            .rev()
            .take_while(|&i| self.scopes[i].kind != Kind::Module)
            .find(|&i| slot_of(&self.scopes[i], name).is_some());
        match found {
            Some(at) => {
                for s in &mut self.scopes[at + 1..] {
                    if !s.locals.contains(&name.to_owned()) && !s.frees.contains(&name.to_owned()) {
                        s.frees.push(name.into());
                    }
                }
                Name::Slot(slot_of(&self.scopes[top], name).unwrap())
            }
            None => Name::Global(self.name_index(name)),
        }
    }
    fn load(&mut self, name: &str, span: Span) {
        match self.resolve(name) {
            Name::Slot(i) => self.emit(Op::Load(i), span),
            Name::Global(i) => self.emit(Op::LoadGlobal(i), span),
        };
    }
    fn store(&mut self, name: &str, span: Span) {
        match self.resolve(name) {
            Name::Slot(i) => self.emit(Op::Store(i), span),
            Name::Global(i) => self.emit(Op::StoreGlobal(i), span),
        };
    }
    fn unbind(&mut self, name: &str, span: Span) {
        match self.resolve(name) {
            Name::Slot(i) => self.emit(Op::Unbind(i), span),
            Name::Global(i) => self.emit(Op::UnbindGlobal(i), span),
        };
    }

    /// 编完一个子作用域：它的 free 从本作用域的槽位取。
    fn finish(&mut self) -> (usize, usize) {
        let mut s = self.scopes.pop().unwrap();
        s.code.nlocals = s.locals.len();
        s.code.slots = s.locals.iter().chain(&s.frees).cloned().collect();
        let frees = s.frees.clone();
        let mut map = vec![];
        for f in &frees {
            match self.resolve(f) {
                Name::Slot(i) => map.push(i),
                Name::Global(_) => unreachable!("free 只来自外层函数"),
            }
        }
        let c = self.code();
        c.children.push(Rc::new(s.code));
        c.cellmaps.push(map);
        (c.children.len() - 1, frees.len())
    }

    fn qualname(&self, name: &str) -> String {
        match self.scopes.last() {
            Some(s) if s.kind != Kind::Module => format!("{}.<locals>.{name}", s.code.qualname),
            _ => name.into(),
        }
    }

    // ---- 语句 ----

    fn body(&mut self, body: &[Stmt]) {
        for s in body {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        let sp = s.span.clone();
        match &s.kind {
            StmtKind::Expr(e) => {
                self.expr(e);
                self.emit(Op::Pop, sp);
            }
            StmtKind::Assign { targets, value } => {
                self.expr(value);
                for (i, t) in targets.iter().enumerate() {
                    if i + 1 < targets.len() {
                        self.emit(Op::Dup, sp.clone());
                    }
                    self.target(t);
                }
            }
            StmtKind::AugAssign { target, op, value } => {
                let op = bin_op(op);
                match &target.kind {
                    ExprKind::Name(n) => {
                        self.load(n, target.span.clone());
                        self.expr(value);
                        self.emit(Op::InPlace(op), sp.clone());
                        self.store(n, sp);
                    }
                    ExprKind::Subscript { value: obj, slice } => {
                        self.expr(obj);
                        self.expr(slice);
                        self.emit(Op::Dup2, sp.clone());
                        self.emit(Op::GetItem, sp.clone());
                        self.expr(value);
                        self.emit(Op::InPlace(op), sp.clone());
                        self.emit(Op::Rot3, sp.clone());
                        self.emit(Op::StoreItem, sp);
                    }
                    ExprKind::Attribute { value: obj, attr } => {
                        self.expr(obj);
                        self.emit(Op::Dup, sp.clone());
                        let a = self.name_index(attr);
                        self.emit(Op::GetAttr(a), sp.clone());
                        self.expr(value);
                        self.emit(Op::InPlace(op), sp.clone());
                        self.emit(Op::Rot2, sp.clone());
                        self.emit(Op::StoreAttr(a), sp);
                    }
                    _ => unreachable!("构造检查放过了增量赋值的目标"),
                }
            }
            StmtKind::FunctionDef(f) => {
                let defaults: Vec<&Expr> =
                    f.args.iter().filter_map(|a| a.default.as_ref()).collect();
                for d in &defaults {
                    self.expr(d);
                }
                let qual = self.qualname(&f.name);
                let mut code = new_code(&self.module, &f.name, &qual);
                code.params = f.args.iter().map(|a| a.name.clone()).collect();
                code.def = Some(f.clone());
                let mut locals = code.params.clone();
                for name in symbols::collect(&f.body, &Imports::new(), false).keys() {
                    if !locals.contains(name) {
                        locals.push(name.clone());
                    }
                }
                let saved = std::mem::take(&mut self.ctx);
                self.scopes.push(Scope {
                    kind: Kind::Function,
                    locals,
                    frees: vec![],
                    code,
                });
                self.body(&f.body);
                let none = self.konst(Value::None);
                self.emit(Op::Const(none), sp.clone());
                self.emit(Op::Return, sp.clone());
                self.ctx = saved;
                let (code, _) = self.finish();
                self.emit(
                    Op::MakeFunction {
                        code,
                        defaults: defaults.len(),
                    },
                    sp.clone(),
                );
                self.store(&f.name, sp);
            }
            StmtKind::Return(e) => {
                match e {
                    Some(e) => self.expr(e),
                    None => {
                        let none = self.konst(Value::None);
                        self.emit(Op::Const(none), sp.clone());
                    }
                }
                self.unwind(None, sp.clone());
                self.emit(Op::Return, sp);
            }
            StmtKind::If { test, body, orelse } => {
                self.expr(test);
                let j = self.emit(Op::JumpIfFalse(0), sp.clone());
                self.body(body);
                if orelse.is_empty() {
                    self.patch(j);
                } else {
                    let end = self.emit(Op::Jump(0), sp);
                    self.patch(j);
                    self.body(orelse);
                    self.patch(end);
                }
            }
            StmtKind::While { test, body, orelse } => {
                let top = self.here();
                self.emit(Op::Tick, sp.clone());
                self.expr(test);
                let exit = self.emit(Op::JumpIfFalse(0), sp.clone());
                self.ctx.push(Ctx::Loop {
                    for_: false,
                    top,
                    breaks: vec![],
                });
                self.body(body);
                self.emit(Op::Jump(top), sp);
                let Some(Ctx::Loop { breaks, .. }) = self.ctx.pop() else {
                    unreachable!("循环体压入的 Ctx::Loop 由它自己弹出")
                };
                self.patch(exit);
                self.body(orelse);
                for b in breaks {
                    self.patch(b);
                }
            }
            StmtKind::For {
                target,
                iter,
                body,
                orelse,
            } => {
                self.expr(iter);
                self.emit(Op::GetIter, sp.clone());
                let top = self.here();
                let exit = self.emit(Op::ForIter(0), sp.clone());
                self.emit(Op::Tick, sp.clone());
                self.target(target);
                self.ctx.push(Ctx::Loop {
                    for_: true,
                    top,
                    breaks: vec![],
                });
                self.body(body);
                self.emit(Op::Jump(top), sp);
                let Some(Ctx::Loop { breaks, .. }) = self.ctx.pop() else {
                    unreachable!("循环体压入的 Ctx::Loop 由它自己弹出")
                };
                self.patch(exit);
                self.body(orelse);
                for b in breaks {
                    self.patch(b);
                }
            }
            StmtKind::Break => {
                let at = self.unwind(Some(true), sp.clone()).expect("break 在循环里");
                let j = self.emit(Op::Jump(0), sp);
                if let Ctx::Loop { breaks, .. } = &mut self.ctx[at] {
                    breaks.push(j);
                }
            }
            StmtKind::Continue => {
                let at = self
                    .unwind(Some(false), sp.clone())
                    .expect("continue 在循环里");
                let Ctx::Loop { top, .. } = self.ctx[at] else {
                    unreachable!("unwind 返回的是循环的 Ctx")
                };
                self.emit(Op::Jump(top), sp);
            }
            StmtKind::Pass => {}
            StmtKind::Try {
                body,
                handlers,
                finalbody,
                ..
            } => {
                if finalbody.is_empty() {
                    self.try_except(body, handlers, sp);
                } else {
                    let setup = self.emit(Op::SetupFinally(0), sp.clone());
                    self.ctx.push(Ctx::Finally(finalbody.clone()));
                    if handlers.is_empty() {
                        self.body(body);
                    } else {
                        self.try_except(body, handlers, sp.clone());
                    }
                    self.ctx.pop();
                    self.emit(Op::PopBlock, sp.clone());
                    self.body(finalbody);
                    let end = self.emit(Op::Jump(0), sp.clone());
                    self.patch(setup);
                    self.emit(Op::EnterHandler, sp.clone());
                    self.ctx.push(Ctx::Handler);
                    self.body(finalbody);
                    self.ctx.pop();
                    self.emit(Op::EndFinally, sp);
                    self.patch(end);
                }
            }
            StmtKind::Raise { exc, .. } => match exc {
                Some(e) => {
                    self.expr(e);
                    self.emit(Op::Raise, sp);
                }
                None => {
                    self.emit(Op::Reraise, sp);
                }
            },
            StmtKind::Import(_) | StmtKind::ImportFrom { .. } => {
                let bindings = self.imports[&s.span.start].clone();
                let full: Vec<Option<String>> = match &s.kind {
                    StmtKind::Import(names) => names.iter().map(|a| Some(a.name.clone())).collect(),
                    _ => vec![None; bindings.len()],
                };
                for ((name, binding), full) in bindings.into_iter().zip(full) {
                    let mut op = self.import_op(&binding);
                    if let Some(full) = full {
                        op.chain = self.import_op(&Binding::Module(full)).chain;
                    }
                    let code = self.code();
                    code.imports.push(op);
                    let i = code.imports.len() - 1;
                    self.emit(Op::Import(i), sp.clone());
                    self.store(&name, sp.clone());
                }
            }
            StmtKind::Unsupported(name) => panic!("构造检查放过了 {name}"),
        }
    }

    fn import_op(&self, binding: &Binding) -> ImportOp {
        let chain = |m: &str| -> Vec<String> {
            let parts: Vec<&str> = m.split('.').collect();
            (1..=parts.len())
                .map(|i| parts[..i].join("."))
                .filter(|p| self.program.modules.contains_key(p))
                .collect()
        };
        match binding {
            Binding::Module(m) => ImportOp {
                chain: chain(m),
                push: Push::Module(m.clone()),
            },
            Binding::From { module, name } => {
                let child = format!("{module}.{name}");
                let chain = chain(if self.program.modules.contains_key(&child) {
                    &child
                } else {
                    module
                });
                ImportOp {
                    chain,
                    push: Push::Attr(module.clone(), name.clone()),
                }
            }
            _ => unreachable!("import 的绑定是模块或模块的名字"),
        }
    }

    fn try_except(&mut self, body: &[Stmt], handlers: &[ExceptHandler], sp: Span) {
        let setup = self.emit(Op::SetupExcept(0), sp.clone());
        self.ctx.push(Ctx::Except);
        self.body(body);
        self.ctx.pop();
        self.emit(Op::PopBlock, sp.clone());
        let mut ends = vec![self.emit(Op::Jump(0), sp.clone())];
        self.patch(setup);
        self.emit(Op::EnterHandler, sp.clone());
        self.ctx.push(Ctx::Handler);
        for h in handlers {
            let next = h.typ.as_ref().map(|t| {
                self.emit(Op::LoadExc, sp.clone());
                self.expr(t);
                self.emit(Op::CheckMatch, sp.clone());
                self.emit(Op::JumpIfFalse(0), sp.clone())
            });
            if let Some(name) = &h.name {
                self.emit(Op::LoadExc, sp.clone());
                self.store(name, sp.clone());
            }
            self.body(&h.body);
            if let Some(name) = &h.name {
                self.unbind(name, sp.clone());
            }
            self.emit(Op::ExitHandler, sp.clone());
            ends.push(self.emit(Op::Jump(0), sp.clone()));
            match next {
                Some(j) => self.patch(j),
                None => break,
            }
        }
        self.ctx.pop();
        self.emit(Op::Reraise, sp);
        for e in ends {
            self.patch(e);
        }
    }

    /// return（`None`）、break（`Some(true)`）、continue（`Some(false)`）离开之前，退出途经的 try 与
    /// except，内联途经的 finally。返回目标循环在 ctx 里的位置。
    fn unwind(&mut self, to_loop: Option<bool>, sp: Span) -> Option<usize> {
        let mut i = self.ctx.len();
        while i > 0 {
            i -= 1;
            match &self.ctx[i] {
                Ctx::Loop { for_, .. } => {
                    if let Some(is_break) = to_loop {
                        if is_break && *for_ {
                            self.emit(Op::Pop, sp);
                        }
                        return Some(i);
                    }
                }
                Ctx::Except => {
                    self.emit(Op::PopBlock, sp.clone());
                }
                Ctx::Handler => {
                    self.emit(Op::ExitHandler, sp.clone());
                }
                Ctx::Finally(body) => {
                    let body = body.clone();
                    self.emit(Op::PopBlock, sp.clone());
                    let rest = self.ctx.split_off(i);
                    self.body(&body);
                    self.ctx.extend(rest);
                }
            }
        }
        None
    }

    fn target(&mut self, t: &Expr) {
        let sp = t.span.clone();
        match &t.kind {
            ExprKind::Name(n) => self.store(n, sp),
            ExprKind::Subscript { value, slice } => {
                self.expr(value);
                self.expr(slice);
                self.emit(Op::StoreItem, sp);
            }
            ExprKind::Attribute { value, attr } => {
                self.expr(value);
                let a = self.name_index(attr);
                self.emit(Op::StoreAttr(a), sp);
            }
            ExprKind::Tuple(es) | ExprKind::List(es) => {
                let star = es
                    .iter()
                    .position(|e| matches!(e.kind, ExprKind::Starred(_)));
                self.emit(Op::Unpack { n: es.len(), star }, sp);
                for e in es {
                    match &e.kind {
                        ExprKind::Starred(inner) => self.target(inner),
                        _ => self.target(e),
                    }
                }
            }
            _ => unreachable!("构造检查放过了赋值的目标"),
        }
    }

    // ---- 表达式 ----

    fn expr(&mut self, e: &Expr) {
        let sp = e.span.clone();
        if let ExprKind::Call { func, .. } = &e.kind
            && matches!(self.program.resolve_expr(&self.module, func), Ok(crate::modules::Resolved::Builtin(ref name)) if matches!(name.as_str(), "typing.TypedDict" | "croon.Port" | "croon.Schema"))
            && let Ok((port, _schema)) = crate::infer::declaration(self.program, &self.module, e)
        {
            let form = if port { Form::Port } else { Form::Schema };
            let k = self.konst(Value::Form(Rc::new(form)));
            self.emit(Op::Const(k), sp);
            return;
        }
        match &e.kind {
            ExprKind::BoolOp { op, values } => {
                let mut jumps = vec![];
                for (i, v) in values.iter().enumerate() {
                    self.expr(v);
                    if i + 1 < values.len() {
                        let j = if op == "And" {
                            Op::JumpIfFalseOrPop(0)
                        } else {
                            Op::JumpIfTrueOrPop(0)
                        };
                        jumps.push(self.emit(j, sp.clone()));
                    }
                }
                for j in jumps {
                    self.patch(j);
                }
            }
            ExprKind::BinOp { left, op, right } => {
                self.expr(left);
                self.expr(right);
                self.emit(Op::Binary(bin_op(op)), sp);
            }
            ExprKind::UnaryOp { op, operand } => {
                self.expr(operand);
                let op = match op.as_str() {
                    "Not" => Un::Not,
                    "USub" => Un::Neg,
                    other => panic!("构造检查放过了 {other}"),
                };
                self.emit(Op::Unary(op), sp);
            }
            ExprKind::IfExp { test, body, orelse } => {
                self.expr(test);
                let j = self.emit(Op::JumpIfFalse(0), sp.clone());
                self.expr(body);
                let end = self.emit(Op::Jump(0), sp);
                self.patch(j);
                self.expr(orelse);
                self.patch(end);
            }
            ExprKind::Dict(items) => {
                self.emit(Op::BuildDict, sp.clone());
                for (k, v) in items {
                    match k {
                        Some(k) => {
                            self.expr(k);
                            self.expr(v);
                            self.emit(Op::DictInsert { call: false }, sp.clone());
                        }
                        None => {
                            self.expr(v);
                            self.emit(Op::DictMerge { call: false }, sp.clone());
                        }
                    }
                }
            }
            ExprKind::List(es) | ExprKind::Tuple(es) => {
                let tuple = matches!(e.kind, ExprKind::Tuple(_));
                if es.iter().any(|x| matches!(x.kind, ExprKind::Starred(_))) {
                    self.emit(Op::BuildList(0), sp.clone());
                    for x in es {
                        match &x.kind {
                            ExprKind::Starred(inner) => {
                                self.expr(inner);
                                self.emit(Op::ListExtend { call: false }, sp.clone());
                            }
                            _ => {
                                self.expr(x);
                                self.emit(Op::ListAppend, sp.clone());
                            }
                        }
                    }
                    if tuple {
                        self.emit(Op::ListToTuple, sp);
                    }
                } else {
                    for x in es {
                        self.expr(x);
                    }
                    self.emit(
                        if tuple {
                            Op::BuildTuple(es.len())
                        } else {
                            Op::BuildList(es.len())
                        },
                        sp,
                    );
                }
            }
            ExprKind::ListComp { elt, generators } => self.comprehension(e, generators, |c| {
                c.expr(elt);
            }),
            ExprKind::DictComp {
                key,
                value,
                generators,
            } => self.comprehension(e, generators, |c| {
                c.expr(key);
                c.expr(value);
            }),
            ExprKind::Compare { operands, ops } => {
                self.expr(&operands[0]);
                if ops.len() == 1 {
                    self.expr(&operands[1]);
                    self.emit(Op::Compare(cmp_op(&ops[0])), sp);
                    return;
                }
                let mut cleanups = vec![];
                for (i, op) in ops.iter().enumerate() {
                    self.expr(&operands[i + 1]);
                    if i + 1 < ops.len() {
                        self.emit(Op::Dup, sp.clone());
                        self.emit(Op::Rot3, sp.clone());
                        self.emit(Op::Compare(cmp_op(op)), sp.clone());
                        cleanups.push(self.emit(Op::JumpIfFalseOrPop(0), sp.clone()));
                    } else {
                        self.emit(Op::Compare(cmp_op(op)), sp.clone());
                    }
                }
                let end = self.emit(Op::Jump(0), sp.clone());
                for c in cleanups {
                    self.patch(c);
                }
                self.emit(Op::Rot2, sp.clone());
                self.emit(Op::Pop, sp);
                self.patch(end);
            }
            ExprKind::Call {
                func,
                args,
                keywords,
            } => {
                self.expr(func);
                let simple = !args.iter().any(|a| matches!(a.kind, ExprKind::Starred(_)))
                    && keywords.iter().all(|k| k.arg.is_some());
                if simple {
                    for a in args {
                        self.expr(a);
                    }
                    let mut kw = vec![];
                    for k in keywords {
                        self.expr(&k.value);
                        kw.push(self.name_index(k.arg.as_ref().unwrap()));
                    }
                    self.emit(
                        Op::Call {
                            pos: args.len(),
                            kw,
                        },
                        sp,
                    );
                } else {
                    self.emit(Op::BuildList(0), sp.clone());
                    for a in args {
                        match &a.kind {
                            ExprKind::Starred(inner) => {
                                self.expr(inner);
                                self.emit(Op::ListExtend { call: true }, sp.clone());
                            }
                            _ => {
                                self.expr(a);
                                self.emit(Op::ListAppend, sp.clone());
                            }
                        }
                    }
                    self.emit(Op::BuildDict, sp.clone());
                    for k in keywords {
                        match &k.arg {
                            Some(name) => {
                                let key = self.konst(Value::Str(name.as_str().into()));
                                self.emit(Op::Const(key), sp.clone());
                                self.expr(&k.value);
                                self.emit(Op::DictInsert { call: true }, sp.clone());
                            }
                            None => {
                                self.expr(&k.value);
                                self.emit(Op::DictMerge { call: true }, sp.clone());
                            }
                        }
                    }
                    self.emit(Op::CallEx, sp);
                }
            }
            ExprKind::Constant(c) => {
                let v = match c {
                    Constant::None => Value::None,
                    Constant::Bool(b) => Value::Bool(*b),
                    Constant::Int(text) => Value::Int(Rc::new(int_literal(text))),
                    Constant::Float(x) => Value::Float(*x),
                    Constant::Str(s) => Value::Str(s.as_str().into()),
                };
                let k = self.konst(v);
                self.emit(Op::Const(k), sp);
            }
            ExprKind::JoinedStr(parts) => {
                for p in parts {
                    self.expr(p);
                }
                self.emit(Op::BuildString(parts.len()), sp);
            }
            ExprKind::FormattedValue { value, .. } => {
                self.expr(value);
                self.emit(Op::FormatValue, sp);
            }
            ExprKind::Attribute { value, attr } => {
                self.expr(value);
                let a = self.name_index(attr);
                self.emit(Op::GetAttr(a), sp);
            }
            ExprKind::Subscript { value, slice } => {
                self.expr(value);
                self.expr(slice);
                self.emit(Op::GetItem, sp);
            }
            ExprKind::Name(n) => self.load(n, sp),
            ExprKind::Starred(_) | ExprKind::Unsupported(_) => {
                panic!("构造检查放过了 {}", e.name())
            }
        }
    }

    /// 推导式是一个立即调用的作用域：最外层的迭代对象在外面求值，作为 `.0` 交进去。
    fn comprehension(
        &mut self,
        e: &Expr,
        generators: &[Comprehension],
        elt: impl FnOnce(&mut Self),
    ) {
        let sp = e.span.clone();
        let dict = matches!(e.kind, ExprKind::DictComp { .. });
        self.expr(&generators[0].iter);
        self.emit(Op::GetIter, sp.clone());
        let name = if dict { "<dictcomp>" } else { "<listcomp>" };
        let qual = self.qualname(name);
        let mut code = new_code(&self.module, name, &qual);
        code.params = vec![".0".into()];
        code.comprehension = true;
        let mut locals = vec![".0".to_owned()];
        for g in generators {
            let mut names = vec![];
            symbols::target_names(&g.target, &mut names);
            for n in names {
                if !locals.contains(&n) {
                    locals.push(n);
                }
            }
        }
        let saved = std::mem::take(&mut self.ctx);
        self.scopes.push(Scope {
            kind: Kind::Comprehension,
            locals,
            frees: vec![],
            code,
        });
        self.emit(
            if dict {
                Op::BuildDict
            } else {
                Op::BuildList(0)
            },
            sp.clone(),
        );
        let mut tops = vec![];
        let mut exits = vec![];
        for (i, g) in generators.iter().enumerate() {
            if i == 0 {
                self.emit(Op::Load(0), sp.clone());
            } else {
                self.expr(&g.iter);
                self.emit(Op::GetIter, sp.clone());
            }
            let top = self.here();
            exits.push(self.emit(Op::ForIter(0), sp.clone()));
            self.emit(Op::Tick, sp.clone());
            self.target(&g.target);
            for cond in &g.ifs {
                self.expr(cond);
                let j = self.emit(Op::JumpIfFalse(top), sp.clone());
                let _ = j;
            }
            tops.push(top);
        }
        elt(self);
        let depth = generators.len() + 1;
        self.emit(
            if dict {
                Op::CompInsert(depth)
            } else {
                Op::CompAppend(depth)
            },
            sp.clone(),
        );
        for i in (0..generators.len()).rev() {
            self.emit(Op::Jump(tops[i]), sp.clone());
            self.patch(exits[i]);
        }
        self.emit(Op::Return, sp.clone());
        self.ctx = saved;
        let (code, _) = self.finish();
        self.emit(Op::MakeFunction { code, defaults: 0 }, sp.clone());
        self.emit(Op::Rot2, sp.clone());
        self.emit(Op::Call { pos: 1, kw: vec![] }, sp);
    }
}

fn slot_of(s: &Scope, name: &str) -> Option<usize> {
    s.locals.iter().position(|n| n == name).or_else(|| {
        s.frees
            .iter()
            .position(|n| n == name)
            .map(|i| s.locals.len() + i)
    })
}

/// 一个模块的全部 Code，按 import 名。
pub fn program(program: &Program) -> BTreeMap<String, Rc<Code>> {
    program
        .modules
        .keys()
        .map(|name| (name.clone(), module(program, name)))
        .collect()
}
