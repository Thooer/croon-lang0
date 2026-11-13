//! Lang0 自己的 AST；Ruff 的类型只出现在 parser 模块。位置是源码的字节偏移。
//! 参考：docs/Lang0/Engineering.md（选择）；docs/Lang0/Subset.md（清单）。

use std::ops::Range;

pub type Span = Range<usize>;

#[derive(Clone, Debug)]
pub struct Module {
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub span: Span,
    pub kind: StmtKind,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Expr(Expr),
    Assign {
        targets: Vec<Expr>,
        value: Expr,
    },
    AugAssign {
        target: Expr,
        op: String,
        value: Expr,
    },
    FunctionDef(FunctionDef),
    Return(Option<Expr>),
    If {
        test: Expr,
        body: Vec<Stmt>,
        orelse: Vec<Stmt>,
    },
    For {
        target: Expr,
        iter: Expr,
        body: Vec<Stmt>,
        orelse: Vec<Stmt>,
    },
    While {
        test: Expr,
        body: Vec<Stmt>,
        orelse: Vec<Stmt>,
    },
    Break,
    Continue,
    Pass,
    Try {
        body: Vec<Stmt>,
        handlers: Vec<ExceptHandler>,
        orelse: Vec<Stmt>,
        finalbody: Vec<Stmt>,
    },
    Raise {
        exc: Option<Expr>,
        cause: Option<Expr>,
    },
    Import(Vec<Alias>),
    ImportFrom {
        module: String,
        level: usize,
        names: Vec<Alias>,
    },
    Unsupported(String),
}

#[derive(Clone, Debug)]
pub struct FunctionDef {
    pub name: String,
    pub args: Vec<Arg>,
    pub returns: Option<Expr>,
    pub body: Vec<Stmt>,
    pub decorators: Vec<Expr>,
    pub parameter_error: Option<Span>,
    pub type_params: Option<Span>,
}

#[derive(Clone, Debug)]
pub struct Arg {
    pub span: Span,
    pub name: String,
    pub annotation: Option<Expr>,
    pub default: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct Alias {
    pub span: Span,
    pub name: String,
    pub asname: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ExceptHandler {
    pub typ: Option<Expr>,
    pub name: Option<String>,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub span: Span,
    pub kind: ExprKind,
}

#[derive(Clone, Debug)]
pub enum Constant {
    None,
    Bool(bool),
    Int(String),
    Float(f64),
    Str(String),
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    BoolOp {
        op: String,
        values: Vec<Expr>,
    },
    BinOp {
        left: Box<Expr>,
        op: String,
        right: Box<Expr>,
    },
    UnaryOp {
        op: String,
        operand: Box<Expr>,
    },
    IfExp {
        test: Box<Expr>,
        body: Box<Expr>,
        orelse: Box<Expr>,
    },
    Dict(Vec<(Option<Expr>, Expr)>),
    List(Vec<Expr>),
    Tuple(Vec<Expr>),
    ListComp {
        elt: Box<Expr>,
        generators: Vec<Comprehension>,
    },
    DictComp {
        key: Box<Expr>,
        value: Box<Expr>,
        generators: Vec<Comprehension>,
    },
    Compare {
        operands: Vec<Expr>,
        ops: Vec<String>,
    },
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
        keywords: Vec<Keyword>,
    },
    Constant(Constant),
    JoinedStr(Vec<Expr>),
    FormattedValue {
        value: Box<Expr>,
        conversion: bool,
        format_spec: bool,
        debug: bool,
    },
    Attribute {
        value: Box<Expr>,
        attr: String,
    },
    Subscript {
        value: Box<Expr>,
        slice: Box<Expr>,
    },
    Name(String),
    Starred(Box<Expr>),
    Unsupported(String),
}

#[derive(Clone, Debug)]
pub struct Keyword {
    pub arg: Option<String>,
    pub value: Expr,
}

#[derive(Clone, Debug)]
pub struct Comprehension {
    pub span: Span,
    pub target: Expr,
    pub iter: Expr,
    pub ifs: Vec<Expr>,
    pub is_async: bool,
}

impl Stmt {
    pub fn name(&self) -> &str {
        match &self.kind {
            StmtKind::Expr(_) => "Expr",
            StmtKind::Assign { .. } => "Assign",
            StmtKind::AugAssign { .. } => "AugAssign",
            StmtKind::FunctionDef(_) => "FunctionDef",
            StmtKind::Return(_) => "Return",
            StmtKind::If { .. } => "If",
            StmtKind::For { .. } => "For",
            StmtKind::While { .. } => "While",
            StmtKind::Break => "Break",
            StmtKind::Continue => "Continue",
            StmtKind::Pass => "Pass",
            StmtKind::Try { .. } => "Try",
            StmtKind::Raise { .. } => "Raise",
            StmtKind::Import(_) => "Import",
            StmtKind::ImportFrom { .. } => "ImportFrom",
            StmtKind::Unsupported(name) => name,
        }
    }

    /// 同一作用域内的子语句；函数体另起作用域。
    pub fn suites(&self) -> Vec<&[Stmt]> {
        match &self.kind {
            StmtKind::If { body, orelse, .. }
            | StmtKind::For { body, orelse, .. }
            | StmtKind::While { body, orelse, .. } => vec![body, orelse],
            StmtKind::Try {
                body,
                handlers,
                orelse,
                finalbody,
            } => {
                let mut out = vec![body.as_slice(), orelse.as_slice(), finalbody.as_slice()];
                out.extend(handlers.iter().map(|h| h.body.as_slice()));
                out
            }
            _ => vec![],
        }
    }
}

impl Expr {
    pub fn name(&self) -> &str {
        match &self.kind {
            ExprKind::BoolOp { .. } => "BoolOp",
            ExprKind::BinOp { .. } => "BinOp",
            ExprKind::UnaryOp { .. } => "UnaryOp",
            ExprKind::IfExp { .. } => "IfExp",
            ExprKind::Dict(_) => "Dict",
            ExprKind::List(_) => "List",
            ExprKind::Tuple(_) => "Tuple",
            ExprKind::ListComp { .. } => "ListComp",
            ExprKind::DictComp { .. } => "DictComp",
            ExprKind::Compare { .. } => "Compare",
            ExprKind::Call { .. } => "Call",
            ExprKind::Constant(_) => "Constant",
            ExprKind::JoinedStr(_) => "JoinedStr",
            ExprKind::FormattedValue { .. } => "FormattedValue",
            ExprKind::Attribute { .. } => "Attribute",
            ExprKind::Subscript { .. } => "Subscript",
            ExprKind::Name(_) => "Name",
            ExprKind::Starred(_) => "Starred",
            ExprKind::Unsupported(name) => name,
        }
    }

    pub fn string(&self) -> Option<&str> {
        if let ExprKind::Constant(Constant::Str(s)) = &self.kind {
            Some(s)
        } else {
            None
        }
    }
}

pub fn diagnostic(path: &str, source: &str, span: &Span, message: impl AsRef<str>) -> String {
    let prefix = &source[..span.start.min(source.len())];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    format!("{path}:{line}:{column}: {}", message.as_ref())
}

pub fn dunder(name: &str) -> bool {
    name.starts_with("__") && name.ends_with("__")
}

pub fn unsupported(name: &str) -> String {
    match name {
        "print" => "用 return 或交出代替；调试看 Journal".into(),
        "ClassDef" => "用 `X = TypedDict(\"X\", {...})` 与函数".into(),
        "Lambda" => "用顶层 def，要固定的参数用 functools.partial（首版不收并发）".into(),
        "With" | "AsyncWith" | "Yield" | "YieldFrom" => "首版不收；with 改成 try/finally".into(),
        "Ellipsis" => "用 pass 代替".into(),
        "类型别名" => "首版不收类型别名".into(),
        "装饰器" => "首版不收装饰器".into(),
        _ => format!("lang0@1 首版不收 {name}"),
    }
}
