//! Phases 1 and 3: name resolution, type checking, interface table.
use std::collections::BTreeSet;

use crate::ast::{Expr, Function, Program, Stmt, Type};
use crate::Error;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lookup {
    Function(String),
    Const(String),
    Global(String),
    Missing(String),
}

pub struct InterfaceView<'a> {
    program: &'a Program,
    recorded: BTreeSet<Lookup>,
}

impl<'a> InterfaceView<'a> {
    pub fn new(program: &'a Program) -> Self {
        Self { program, recorded: BTreeSet::new() }
    }

    pub fn lookup_function(&mut self, name: &str) -> Option<crate::ast::Signature> {
        if let Some(f) = self.program.functions.iter().find(|f| f.sig.name == name) {
            self.recorded.insert(Lookup::Function(name.to_string()));
            Some(f.sig.clone())
        } else {
            self.recorded.insert(Lookup::Missing(name.to_string()));
            None
        }
    }

    pub fn lookup_const(&mut self, name: &str) -> Option<(Type, i64)> {
        if let Some(c) = self.program.consts.iter().find(|c| c.name == name) {
            self.recorded.insert(Lookup::Const(name.to_string()));
            Some((c.ty.clone(), c.value))
        } else {
            self.recorded.insert(Lookup::Missing(name.to_string()));
            None
        }
    }

    pub fn lookup_global(&mut self, name: &str) -> Option<(Type, bool)> {
        if let Some(g) = self.program.globals.iter().find(|g| g.name == name) {
            self.recorded.insert(Lookup::Global(name.to_string()));
            Some((g.ty.clone(), g.mutable))
        } else {
            self.recorded.insert(Lookup::Missing(name.to_string()));
            None
        }
    }

    pub fn record(&mut self, l: Lookup) {
        self.recorded.insert(l);
    }

    pub fn into_dependencies(self) -> BTreeSet<Lookup> {
        self.recorded
    }
}

// type checking
pub fn check_function(f: &Function, program: &Program, view: &mut InterfaceView) -> Result<(), Error> {
    let mut locals: std::collections::BTreeMap<String, Type> = std::collections::BTreeMap::new();
    for p in &f.sig.params {
        locals.insert(p.name.clone(), p.ty.clone());
    }
    for s in &f.body {
        check_stmt(s, &mut locals, &f.sig.ret, program, view)?;
    }
    Ok(())
}

fn check_stmt(stmt: &Stmt, locals: &mut std::collections::BTreeMap<String, Type>, ret: &Type, program: &Program, view: &mut InterfaceView) -> Result<(), Error> {
    match stmt {
        Stmt::Let { name, ty, init } => {
            if let Some(e) = init {
                let t = type_of(e, locals, program, view)?;
                if t != *ty { return Err(Error::Sema(format!("type mismatch in let {name}"))); }
            }
            locals.insert(name.clone(), ty.clone());
        }
        Stmt::Assign { target, value } => {
            let ttarget = if let Some(t) = locals.get(target) {
                t.clone()
            } else if let Some((ty, _)) = view.lookup_global(target) {
                ty
            } else {
                return Err(Error::Sema(format!("unknown variable {target}")));
            };
            let tv = type_of(value, locals, program, view)?;
            if tv != ttarget { return Err(Error::Sema(format!("type mismatch in assign {target}"))); }
        }
        Stmt::If { cond, then, else_ } => {
            let tc = type_of(cond, locals, program, view)?;
            if tc != Type::Bool { return Err(Error::Sema("if condition must be bool".into())); }
            let mut then_locals = locals.clone();
            for s in then { check_stmt(s, &mut then_locals, ret, program, view)?; }
            if let Some(v) = else_ {
                let mut else_locals = locals.clone();
                for s in v { check_stmt(s, &mut else_locals, ret, program, view)?; }
            }
        }
        Stmt::While { cond, body } => {
            let tc = type_of(cond, locals, program, view)?;
            if tc != Type::Bool { return Err(Error::Sema("while condition must be bool".into())); }
            let mut body_locals = locals.clone();
            for s in body { check_stmt(s, &mut body_locals, ret, program, view)?; }
        }
        Stmt::Return(e) => {
            match (e, ret) {
                (None, Type::Void) => {}
                (Some(expr), _) => {
                    let t = type_of(expr, locals, program, view)?;
                    if t != *ret { return Err(Error::Sema("return type mismatch".into())); }
                }
                _ => return Err(Error::Sema("return mismatch".into())),
            }
        }
        Stmt::Expr(e) => { let _ = type_of(e, locals, program, view)?; }
    }
    Ok(())
}

fn type_of(e: &Expr, locals: &std::collections::BTreeMap<String, Type>, program: &Program, view: &mut InterfaceView) -> Result<Type, Error> {
    match e {
        Expr::IntLit(_) => Ok(Type::Int),
        Expr::BoolLit(_) => Ok(Type::Bool),
        Expr::Var(name) => {
            if let Some(t) = locals.get(name) { return Ok(t.clone()); }
            if let Some((ty, _)) = view.lookup_const(name) { return Ok(ty); }
            if let Some((ty, _)) = view.lookup_global(name) { return Ok(ty); }
            // function reference as value not allowed
            view.record(Lookup::Missing(name.clone()));
            Err(Error::Sema(format!("unknown name {name}")))
        }
        Expr::Call(name, args) => {
            let sig = view.lookup_function(name).ok_or_else(|| Error::Sema(format!("unknown function {name}")))?;
            if sig.params.len() != args.len() { return Err(Error::Sema(format!("arg count mismatch in {name}"))); }
            for (a, p) in args.iter().zip(sig.params.iter()) {
                let ta = type_of(a, locals, program, view)?;
                if ta != p.ty { return Err(Error::Sema(format!("arg type mismatch in {name}"))); }
            }
            Ok(sig.ret)
        }
        Expr::Unary(op, e) => {
            let t = type_of(e, locals, program, view)?;
            match op {
                crate::ast::UnOp::Neg => { if t != Type::Int { return Err(Error::Sema("neg requires int".into())); } Ok(Type::Int) }
                crate::ast::UnOp::Not => { if t != Type::Bool { return Err(Error::Sema("not requires bool".into())); } Ok(Type::Bool) }
            }
        }
        Expr::Binary(op, l, r) => {
            use crate::ast::BinOp;
            let tl = type_of(l, locals, program, view)?;
            let tr = type_of(r, locals, program, view)?;
            match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                    if tl != Type::Int || tr != Type::Int { return Err(Error::Sema("arith requires int".into())); }
                    Ok(Type::Int)
                }
                BinOp::Eq | BinOp::Ne => {
                    if tl != tr { return Err(Error::Sema("eq requires same types".into())); }
                    Ok(Type::Bool)
                }
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    if tl != Type::Int || tr != Type::Int { return Err(Error::Sema("cmp requires int".into())); }
                    Ok(Type::Bool)
                }
                BinOp::And | BinOp::Or => {
                    if tl != Type::Bool || tr != Type::Bool { return Err(Error::Sema("logical requires bool".into())); }
                    Ok(Type::Bool)
                }
            }
        }
    }
}

pub fn dependencies(f: &Function, program: &Program) -> Result<std::collections::BTreeSet<Lookup>, Error> {
    let mut view = InterfaceView::new(program);
    check_function(f, program, &mut view)?;
    Ok(view.into_dependencies())
}
