//! Phase 1: AST with source spans. Use BTreeMap/Vec (never HashMap iteration) to keep output deterministic.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int,
    Bool,
    Void,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    IntLit(i64),
    BoolLit(bool),
    Var(String),
    Call(String, Vec<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Let { name: String, ty: Type, init: Option<Expr> },
    Assign { target: String, value: Expr },
    If { cond: Expr, then: Vec<Stmt>, else_: Option<Vec<Stmt>> },
    While { cond: Expr, body: Vec<Stmt> },
    Return(Option<Expr>),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Type,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub sig: Signature,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstDecl {
    pub name: String,
    pub ty: Type,
    pub value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalDecl {
    pub name: String,
    pub ty: Type,
    pub mutable: bool,
    pub init: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Program {
    pub consts: Vec<ConstDecl>,
    pub globals: Vec<GlobalDecl>,
    pub functions: Vec<Function>,
}

pub fn push_u8(out: &mut Vec<u8>, b: u8) { out.push(b); }
pub fn push_u64(out: &mut Vec<u8>, v: u64) { out.extend_from_slice(&v.to_le_bytes()); }
pub fn push_str(out: &mut Vec<u8>, s: &str) {
    push_u64(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

pub fn canon_type(t: &Type, out: &mut Vec<u8>) {
    match t {
        Type::Int => push_u8(out, 1),
        Type::Bool => push_u8(out, 2),
        Type::Void => push_u8(out, 3),
    }
}

pub fn canon_expr(e: &Expr, out: &mut Vec<u8>) {
    match e {
        Expr::IntLit(n) => { push_u8(out, 10); push_u64(out, *n as u64); }
        Expr::BoolLit(b) => { push_u8(out, 11); push_u8(out, if *b { 1 } else { 0 }); }
        Expr::Var(s) => { push_u8(out, 12); push_str(out, s); }
        Expr::Call(name, args) => {
            push_u8(out, 13); push_str(out, name); push_u64(out, args.len() as u64);
            for a in args { canon_expr(a, out); }
        }
        Expr::Unary(op, e) => {
            push_u8(out, 14);
            push_u8(out, match op { UnOp::Neg => 1, UnOp::Not => 2 });
            canon_expr(e, out);
        }
        Expr::Binary(op, l, r) => {
            push_u8(out, 15);
            push_u8(out, match op {
                BinOp::Add => 1, BinOp::Sub => 2, BinOp::Mul => 3, BinOp::Div => 4, BinOp::Mod => 5,
                BinOp::Eq => 6, BinOp::Ne => 7, BinOp::Lt => 8, BinOp::Le => 9, BinOp::Gt => 10, BinOp::Ge => 11,
                BinOp::And => 12, BinOp::Or => 13,
            });
            canon_expr(l, out); canon_expr(r, out);
        }
    }
}

pub fn canon_stmt(s: &Stmt, out: &mut Vec<u8>) {
    match s {
        Stmt::Let { name, ty, init } => {
            push_u8(out, 20); push_str(out, name); canon_type(ty, out);
            match init { Some(e) => { push_u8(out, 1); canon_expr(e, out); }, None => { push_u8(out, 0); } }
        }
        Stmt::Assign { target, value } => {
            push_u8(out, 21); push_str(out, target); canon_expr(value, out);
        }
        Stmt::If { cond, then, else_ } => {
            push_u8(out, 22); canon_expr(cond, out);
            push_u64(out, then.len() as u64); for s in then { canon_stmt(s, out); }
            match else_ { Some(v) => { push_u8(out, 1); push_u64(out, v.len() as u64); for s in v { canon_stmt(s, out); } }, None => { push_u8(out, 0); } }
        }
        Stmt::While { cond, body } => {
            push_u8(out, 23); canon_expr(cond, out);
            push_u64(out, body.len() as u64); for s in body { canon_stmt(s, out); }
        }
        Stmt::Return(e) => {
            push_u8(out, 24);
            match e { Some(x) => { push_u8(out, 1); canon_expr(x, out); }, None => { push_u8(out, 0); } }
        }
        Stmt::Expr(e) => { push_u8(out, 25); canon_expr(e, out); }
    }
}

pub fn canon_function_body(f: &Function) -> Vec<u8> {
    let mut out = Vec::new();
    push_u64(&mut out, f.body.len() as u64);
    for s in &f.body { canon_stmt(s, &mut out); }
    out
}

pub fn canon_program_consts(c: &ConstDecl) -> Vec<u8> {
    let mut out = Vec::new();
    push_str(&mut out, &c.name); canon_type(&c.ty, &mut out); push_u64(&mut out, c.value as u64);
    out
}

pub fn canon_program_global(g: &GlobalDecl) -> Vec<u8> {
    let mut out = Vec::new();
    push_str(&mut out, &g.name); canon_type(&g.ty, &mut out); push_u8(&mut out, if g.mutable { 1 } else { 0 });
    // include init expr if present
    match &g.init { Some(e) => { push_u8(&mut out, 1); canon_expr(e, &mut out); }, None => { push_u8(&mut out, 0); } }
    out
}
