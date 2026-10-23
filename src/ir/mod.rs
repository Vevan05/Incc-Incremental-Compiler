//! Phase 1: three-address-code IR (basic blocks), builder, and optimizations.
use crate::ast::{BinOp, Expr, Function, Program, Stmt, UnOp};

#[derive(Debug, Clone)]
pub enum Value {
    ConstInt(i64),
    ConstBool(bool),
    Var(String),
    Temp(usize),
}

#[derive(Debug, Clone)]
pub enum Instr {
    Assign { dst: Value, src: Value },
    BinOp { op: BinOp, dst: Value, l: Value, r: Value },
    UnOp { op: UnOp, dst: Value, src: Value },
    Call { dst: Option<Value>, name: String, args: Vec<Value> },
    LoadGlobal { dst: Value, name: String },
    StoreGlobal { name: String, src: Value },
    Label(String),
    Jump(String),
    CondJump { cond: Value, t: String, f: String },
    Return(Option<Value>),
}

#[derive(Debug, Clone)]
pub struct FunctionIR {
    pub name: String,
    pub params: Vec<String>,
    pub locals: Vec<String>,
    pub instrs: Vec<Instr>,
}

pub fn lower_function(f: &Function, program: &Program) -> FunctionIR {
    let mut instrs = Vec::new();
    let mut temp_counter = 0usize;
    let mut locals = Vec::new();
    let params: Vec<String> = f.sig.params.iter().map(|p| p.name.clone()).collect();
    let fname = f.sig.name.clone();
    for s in &f.body {
        lower_stmt(s, &mut instrs, &mut temp_counter, &mut locals, program, &fname);
    }
    FunctionIR { name: f.sig.name.clone(), params, locals, instrs }
}

fn new_temp(c: &mut usize) -> usize { let t = *c; *c += 1; t }
fn new_label(c: &mut usize, prefix: &str) -> String { let t = *c; *c += 1; format!("{}_L{}", prefix, t) }

fn lower_expr(e: &Expr, instrs: &mut Vec<Instr>, tc: &mut usize, program: &Program) -> Value {
    match e {
        Expr::IntLit(n) => Value::ConstInt(*n),
        Expr::BoolLit(b) => Value::ConstBool(*b),
        Expr::Var(s) => {
            if let Some(c) = program.consts.iter().find(|c| c.name == *s) {
                return Value::ConstInt(c.value);
            }
            if program.globals.iter().any(|g| g.name == *s) {
                let t = new_temp(tc);
                instrs.push(Instr::LoadGlobal { dst: Value::Temp(t), name: s.clone() });
                return Value::Temp(t);
            }
            Value::Var(s.clone())
        }
        Expr::Call(name, args) => {
            let argvals: Vec<Value> = args.iter().map(|a| lower_expr(a, instrs, tc, program)).collect();
            // type unknown here; assume non-void if used as expr. dst temp always.
            let t = new_temp(tc);
            instrs.push(Instr::Call { dst: Some(Value::Temp(t)), name: name.clone(), args: argvals });
            Value::Temp(t)
        }
        Expr::Unary(op, e) => {
            let v = lower_expr(e, instrs, tc, program);
            let t = new_temp(tc);
            instrs.push(Instr::UnOp { op: op.clone(), dst: Value::Temp(t), src: v });
            Value::Temp(t)
        }
        Expr::Binary(op, l, r) => {
            let lv = lower_expr(l, instrs, tc, program);
            let rv = lower_expr(r, instrs, tc, program);
            let t = new_temp(tc);
            instrs.push(Instr::BinOp { op: op.clone(), dst: Value::Temp(t), l: lv, r: rv });
            Value::Temp(t)
        }
    }
}

fn lower_stmt(stmt: &Stmt, instrs: &mut Vec<Instr>, tc: &mut usize, locals: &mut Vec<String>, program: &Program, prefix: &str) {
    match stmt {
        Stmt::Let { name, ty: _, init } => {
            if let Some(e) = init {
                let v = lower_expr(e, instrs, tc, program);
                instrs.push(Instr::Assign { dst: Value::Var(name.clone()), src: v });
            }
            locals.push(name.clone());
        }
        Stmt::Assign { target, value } => {
            let v = lower_expr(value, instrs, tc, program);
            if program.globals.iter().any(|g| g.name == *target) {
                instrs.push(Instr::StoreGlobal { name: target.clone(), src: v });
            } else {
                instrs.push(Instr::Assign { dst: Value::Var(target.clone()), src: v });
            }
        }
        Stmt::If { cond, then, else_ } => {
            let c = lower_expr(cond, instrs, tc, program);
            let l_then = new_label(tc, prefix);
            let l_else = new_label(tc, prefix);
            let l_end = new_label(tc, prefix);
            if let Some(ev) = else_ {
                instrs.push(Instr::CondJump { cond: c, t: l_then.clone(), f: l_else.clone() });
                instrs.push(Instr::Label(l_then));
                for s in then { lower_stmt(s, instrs, tc, locals, program, prefix); }
                instrs.push(Instr::Jump(l_end.clone()));
                instrs.push(Instr::Label(l_else));
                for s in ev { lower_stmt(s, instrs, tc, locals, program, prefix); }
            } else {
                instrs.push(Instr::CondJump { cond: c, t: l_then.clone(), f: l_end.clone() });
                instrs.push(Instr::Label(l_then));
                for s in then { lower_stmt(s, instrs, tc, locals, program, prefix); }
            }
            instrs.push(Instr::Label(l_end));
        }
        Stmt::While { cond, body } => {
            let l_cond = new_label(tc, prefix);
            let l_body = new_label(tc, prefix);
            let l_end = new_label(tc, prefix);
            instrs.push(Instr::Label(l_cond.clone()));
            let c = lower_expr(cond, instrs, tc, program);
            instrs.push(Instr::CondJump { cond: c, t: l_body.clone(), f: l_end.clone() });
            instrs.push(Instr::Label(l_body));
            for s in body { lower_stmt(s, instrs, tc, locals, program, prefix); }
            instrs.push(Instr::Jump(l_cond));
            instrs.push(Instr::Label(l_end));
        }
        Stmt::Return(e) => {
            let v = e.as_ref().map(|x| lower_expr(x, instrs, tc, program));
            instrs.push(Instr::Return(v));
        }
        Stmt::Expr(e) => {
            // if call statement, emit call with no dst
            if let Expr::Call(name, args) = e {
                let argvals: Vec<Value> = args.iter().map(|a| lower_expr(a, instrs, tc, program)).collect();
                instrs.push(Instr::Call { dst: None, name: name.clone(), args: argvals });
            } else {
                let _ = lower_expr(e, instrs, tc, program);
            }
        }
    }
}

// ---- optimizations ----

pub fn optimize(fir: &mut FunctionIR) {
    for _ in 0..3 {
        constant_fold(&mut fir.instrs);
        copy_propagate(&mut fir.instrs);
        dead_code(&mut fir.instrs);
    }
}

fn constant_fold(instrs: &mut Vec<Instr>) {
    for i in 0..instrs.len() {
        match &instrs[i] {
            Instr::BinOp { op, dst, l, r } => {
                let res = match (l, r) {
                    (Value::ConstInt(a), Value::ConstInt(b)) => {
                        use crate::ast::BinOp;
                        match op {
                            BinOp::Add => Some(Value::ConstInt(a + b)),
                            BinOp::Sub => Some(Value::ConstInt(a - b)),
                            BinOp::Mul => Some(Value::ConstInt(a * b)),
                            BinOp::Div => if *b != 0 { Some(Value::ConstInt(a / b)) } else { None },
                            BinOp::Mod => if *b != 0 { Some(Value::ConstInt(a % b)) } else { None },
                            BinOp::Eq => Some(Value::ConstBool(*a == *b)),
                            BinOp::Ne => Some(Value::ConstBool(*a != *b)),
                            BinOp::Lt => Some(Value::ConstBool(*a < *b)),
                            BinOp::Le => Some(Value::ConstBool(*a <= *b)),
                            BinOp::Gt => Some(Value::ConstBool(*a > *b)),
                            BinOp::Ge => Some(Value::ConstBool(*a >= *b)),
                            BinOp::And => Some(Value::ConstBool(*a != 0 && *b != 0)),
                            BinOp::Or => Some(Value::ConstBool(*a != 0 || *b != 0)),
                        }
                    }
                    (Value::ConstBool(a), Value::ConstBool(b)) => {
                        use crate::ast::BinOp;
                        match op {
                            BinOp::Eq => Some(Value::ConstBool(*a == *b)),
                            BinOp::Ne => Some(Value::ConstBool(*a != *b)),
                            BinOp::And => Some(Value::ConstBool(*a && *b)),
                            BinOp::Or => Some(Value::ConstBool(*a || *b)),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(v) = res {
                    let dstc = dst.clone();
                    instrs[i] = Instr::Assign { dst: dstc, src: v };
                }
            }
            Instr::UnOp { op, dst, src } => {
                let res = match src {
                    Value::ConstInt(a) => match op {
                        UnOp::Neg => Some(Value::ConstInt(-a)),
                        UnOp::Not => Some(Value::ConstBool(*a == 0)),
                    },
                    Value::ConstBool(a) => match op {
                        UnOp::Not => Some(Value::ConstBool(!a)),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(v) = res {
                    let dstc = dst.clone();
                    instrs[i] = Instr::Assign { dst: dstc, src: v };
                }
            }
            _ => {}
        }
    }
}

fn copy_propagate(instrs: &mut Vec<Instr>) {
    // build map from temp -> value for simple copies, propagate forward
    let mut map: std::collections::BTreeMap<usize, Value> = std::collections::BTreeMap::new();
    for instr in instrs.iter_mut() {
        // substitute uses
        substitute(instr, &map);
        // record new copy
        if let Instr::Assign { dst: Value::Temp(t), src } = instr {
            if !matches!(src, Value::Temp(x) if *x == *t) {
                map.insert(*t, src.clone());
            }
        }
    }
}

fn substitute(instr: &mut Instr, map: &std::collections::BTreeMap<usize, Value>) {
    let sub = |v: &mut Value| {
        if let Value::Temp(t) = v {
            if let Some(repl) = map.get(t) {
                *v = repl.clone();
            }
        }
    };
    match instr {
        Instr::Assign { src, .. } => sub(src),
        Instr::BinOp { l, r, .. } => { sub(l); sub(r); }
        Instr::UnOp { src, .. } => sub(src),
        Instr::Call { args, .. } => { for a in args { sub(a); } }
        Instr::StoreGlobal { src, .. } => sub(src),
        Instr::CondJump { cond, .. } => sub(cond),
        Instr::Return(Some(v)) => sub(v),
        _ => {}
    }
}

fn dead_code(instrs: &mut Vec<Instr>) {
    // compute used temps
    let mut used: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    for instr in instrs.iter() {
        match instr {
            Instr::Assign { src: Value::Temp(t), .. } => { used.insert(*t); }
            Instr::BinOp { l: Value::Temp(t), .. } => { used.insert(*t); }
            Instr::BinOp { r: Value::Temp(t), .. } => { used.insert(*t); }
            Instr::UnOp { src: Value::Temp(t), .. } => { used.insert(*t); }
            Instr::Call { dst: Some(Value::Temp(_t)), .. } => { /* dst defined; args used */ }
            Instr::Call { args, .. } => { for a in args { if let Value::Temp(t)=a { used.insert(*t); } } }
            Instr::LoadGlobal { dst: _, .. } => {}
            Instr::StoreGlobal { src: Value::Temp(t), .. } => { used.insert(*t); }
            Instr::CondJump { cond: Value::Temp(t), .. } => { used.insert(*t); }
            Instr::Return(Some(Value::Temp(t))) => { used.insert(*t); }
            _ => {}
        }
        // also args of call
        if let Instr::Call { args, .. } = instr {
            for a in args { if let Value::Temp(t)=a { used.insert(*t); } }
        }
        if let Instr::BinOp { l, r, .. } = instr {
            if let Value::Temp(t)=l { used.insert(*t); }
            if let Value::Temp(t)=r { used.insert(*t); }
        }
    }
    // remove pure temp assignments/ops whose dst temp unused
    instrs.retain(|instr| {
        match instr {
            Instr::Assign { dst: Value::Temp(t), .. } => used.contains(t),
            Instr::BinOp { dst: Value::Temp(t), .. } => used.contains(t),
            Instr::UnOp { dst: Value::Temp(t), .. } => used.contains(t),
            Instr::Call { dst: Some(Value::Temp(_)), .. } => {
                // keep calls (side effects) always
                true
            }
            _ => true,
        }
    });
}
