//! Phase 1: recursive-descent parser (Pratt for expressions).
use crate::ast::*;
use crate::lexer::Token;
use crate::{Error, Result};

pub fn parse(src: &str) -> Result<Program> {
    let toks = crate::lexer::lex(src)?;
    let mut p = Parser { toks, pos: 0, consts: std::collections::BTreeMap::new() };
    p.parse_program()
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    consts: std::collections::BTreeMap<String, i64>,
}

impl Parser {
    fn peek(&self) -> &Token { &self.toks[self.pos] }
    fn advance(&mut self) -> Token { let t = self.toks[self.pos].clone(); self.pos += 1; t }
    fn expect(&mut self, tok: &Token) -> Result<()> {
        if &self.toks[self.pos] == tok { self.pos += 1; Ok(()) }
        else { Err(Error::Parse(format!("expected {:?}, got {:?}", tok, self.toks[self.pos]))) }
    }
    fn at_eof(&self) -> bool { matches!(self.peek(), Token::Eof) }

    fn parse_program(&mut self) -> Result<Program> {
        let mut prog = Program::default();
        while !self.at_eof() {
            // const
            if matches!(self.peek(), Token::KwConst) {
                self.advance();
                let ty = self.parse_type()?;
                let name = self.expect_ident()?;
                self.expect(&Token::Eq)?;
                let e = self.parse_expr()?;
                self.expect(&Token::Semi)?;
                let val = eval_const_expr(&e, &self.consts).unwrap_or(0);
                self.consts.insert(name.clone(), val);
                prog.consts.push(ConstDecl { name, ty, value: val });
                continue;
            }
            // type-led declaration/function
            if self.is_type(self.peek()) {
                let ty = self.parse_type()?;
                let name = self.expect_ident()?;
                if matches!(self.peek(), Token::LParen) {
                    // function
                    self.advance();
                    let mut params = Vec::new();
                    if !matches!(self.peek(), Token::RParen) {
                        loop {
                            let pty = self.parse_type()?;
                            let pname = self.expect_ident()?;
                            params.push(Param { name: pname, ty: pty });
                            if matches!(self.peek(), Token::Comma) { self.advance(); } else { break; }
                        }
                    }
                    self.expect(&Token::RParen)?;
                    self.expect(&Token::LBrace)?;
                    let body = self.parse_stmts()?;
                    self.expect(&Token::RBrace)?;
                    prog.functions.push(Function { sig: Signature { name, params, ret: ty }, body, span: Span { start: 0, end: 0 } });
                } else {
                    // global var
                    let mut init = None;
                    if matches!(self.peek(), Token::Eq) {
                        self.advance();
                        init = Some(self.parse_expr()?);
                    }
                    self.expect(&Token::Semi)?;
                    let mutable = true;
                    prog.globals.push(GlobalDecl { name, ty, mutable, init });
                }
                continue;
            }
            return Err(Error::Parse(format!("unexpected token {:?}", self.peek())));
        }
        Ok(prog)
    }

    fn is_type(&self, t: &Token) -> bool {
        matches!(t, Token::KwInt | Token::KwBool | Token::KwVoid)
    }

    fn parse_type(&mut self) -> Result<Type> {
        match self.advance() {
            Token::KwInt => Ok(Type::Int),
            Token::KwBool => Ok(Type::Bool),
            Token::KwVoid => Ok(Type::Void),
            other => Err(Error::Parse(format!("expected type, got {:?}", other))),
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            other => Err(Error::Parse(format!("expected ident, got {:?}", other))),
        }
    }

    fn parse_stmts(&mut self) -> Result<Vec<Stmt>> {
        let mut stmts = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> Result<Stmt> {
        // let declaration
        if self.is_type(self.peek()) {
            let ty = self.parse_type()?;
            let name = self.expect_ident()?;
            let mut init = None;
            if matches!(self.peek(), Token::Eq) {
                self.advance();
                init = Some(self.parse_expr()?);
            }
            self.expect(&Token::Semi)?;
            return Ok(Stmt::Let { name, ty, init });
        }
        if matches!(self.peek(), Token::KwIf) {
            self.advance();
            self.expect(&Token::LParen)?;
            let cond = self.parse_expr()?;
            self.expect(&Token::RParen)?;
            self.expect(&Token::LBrace)?;
            let then = self.parse_stmts()?;
            self.expect(&Token::RBrace)?;
            let mut else_ = None;
            if matches!(self.peek(), Token::KwElse) {
                self.advance();
                self.expect(&Token::LBrace)?;
                else_ = Some(self.parse_stmts()?);
                self.expect(&Token::RBrace)?;
            }
            return Ok(Stmt::If { cond, then, else_ });
        }
        if matches!(self.peek(), Token::KwWhile) {
            self.advance();
            self.expect(&Token::LParen)?;
            let cond = self.parse_expr()?;
            self.expect(&Token::RParen)?;
            self.expect(&Token::LBrace)?;
            let body = self.parse_stmts()?;
            self.expect(&Token::RBrace)?;
            return Ok(Stmt::While { cond, body });
        }
        if matches!(self.peek(), Token::KwReturn) {
            self.advance();
            if matches!(self.peek(), Token::Semi) {
                self.expect(&Token::Semi)?;
                return Ok(Stmt::Return(None));
            }
            let e = self.parse_expr()?;
            self.expect(&Token::Semi)?;
            return Ok(Stmt::Return(Some(e)));
        }
        // assignment or expr statement
        let e = self.parse_expr()?;
        if matches!(self.peek(), Token::Eq) {
            self.advance();
            // target must be var
            let target = match e { Expr::Var(s) => s, _ => return Err(Error::Parse("invalid assignment target".into())) };
            let value = self.parse_expr()?;
            self.expect(&Token::Semi)?;
            return Ok(Stmt::Assign { target, value });
        }
        self.expect(&Token::Semi)?;
        Ok(Stmt::Expr(e))
    }

    // Pratt expression parser
    fn parse_expr(&mut self) -> Result<Expr> { self.parse_or() }
    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while matches!(self.peek(), Token::OrOr) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }
    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_eq()?;
        while matches!(self.peek(), Token::AndAnd) {
            self.advance();
            let right = self.parse_eq()?;
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }
    fn parse_eq(&mut self) -> Result<Expr> {
        let mut left = self.parse_rel()?;
        loop {
            if matches!(self.peek(), Token::EqEq) { self.advance(); let r = self.parse_rel()?; left = Expr::Binary(BinOp::Eq, Box::new(left), Box::new(r)); }
            else if matches!(self.peek(), Token::BangEq) { self.advance(); let r = self.parse_rel()?; left = Expr::Binary(BinOp::Ne, Box::new(left), Box::new(r)); }
            else { break; }
        }
        Ok(left)
    }
    fn parse_rel(&mut self) -> Result<Expr> {
        let mut left = self.parse_add()?;
        loop {
            if matches!(self.peek(), Token::Lt) { self.advance(); let r = self.parse_add()?; left = Expr::Binary(BinOp::Lt, Box::new(left), Box::new(r)); }
            else if matches!(self.peek(), Token::Le) { self.advance(); let r = self.parse_add()?; left = Expr::Binary(BinOp::Le, Box::new(left), Box::new(r)); }
            else if matches!(self.peek(), Token::Gt) { self.advance(); let r = self.parse_add()?; left = Expr::Binary(BinOp::Gt, Box::new(left), Box::new(r)); }
            else if matches!(self.peek(), Token::Ge) { self.advance(); let r = self.parse_add()?; left = Expr::Binary(BinOp::Ge, Box::new(left), Box::new(r)); }
            else { break; }
        }
        Ok(left)
    }
    fn parse_add(&mut self) -> Result<Expr> {
        let mut left = self.parse_mul()?;
        loop {
            if matches!(self.peek(), Token::Plus) { self.advance(); let r = self.parse_mul()?; left = Expr::Binary(BinOp::Add, Box::new(left), Box::new(r)); }
            else if matches!(self.peek(), Token::Minus) { self.advance(); let r = self.parse_mul()?; left = Expr::Binary(BinOp::Sub, Box::new(left), Box::new(r)); }
            else { break; }
        }
        Ok(left)
    }
    fn parse_mul(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            if matches!(self.peek(), Token::Star) { self.advance(); let r = self.parse_unary()?; left = Expr::Binary(BinOp::Mul, Box::new(left), Box::new(r)); }
            else if matches!(self.peek(), Token::Slash) { self.advance(); let r = self.parse_unary()?; left = Expr::Binary(BinOp::Div, Box::new(left), Box::new(r)); }
            else if matches!(self.peek(), Token::Percent) { self.advance(); let r = self.parse_unary()?; left = Expr::Binary(BinOp::Mod, Box::new(left), Box::new(r)); }
            else { break; }
        }
        Ok(left)
    }
    fn parse_unary(&mut self) -> Result<Expr> {
        if matches!(self.peek(), Token::Minus) { self.advance(); let e = self.parse_unary()?; return Ok(Expr::Unary(UnOp::Neg, Box::new(e))); }
        if matches!(self.peek(), Token::Bang) { self.advance(); let e = self.parse_unary()?; return Ok(Expr::Unary(UnOp::Not, Box::new(e))); }
        self.parse_primary()
    }
    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek().clone() {
            Token::IntLit(n) => { self.advance(); Ok(Expr::IntLit(n)) }
            Token::KwTrue => { self.advance(); Ok(Expr::BoolLit(true)) }
            Token::KwFalse => { self.advance(); Ok(Expr::BoolLit(false)) }
            Token::Ident(name) => {
                self.advance();
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Token::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if matches!(self.peek(), Token::Comma) { self.advance(); } else { break; }
                        }
                    }
                    self.expect(&Token::RParen)?;
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            Token::LParen => {
                self.advance();
                let e = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(e)
            }
            other => Err(Error::Parse(format!("unexpected in expr: {:?}", other))),
        }
    }
}

fn eval_const_expr(e: &Expr, consts: &std::collections::BTreeMap<String, i64>) -> Option<i64> {
    match e {
        Expr::IntLit(n) => Some(*n),
        Expr::BoolLit(b) => Some(if *b { 1 } else { 0 }),
        Expr::Var(s) => consts.get(s).copied(),
        Expr::Unary(UnOp::Neg, e) => eval_const_expr(e, consts).map(|v| -v),
        Expr::Unary(UnOp::Not, e) => eval_const_expr(e, consts).map(|v| if v == 0 { 1 } else { 0 }),
        Expr::Binary(op, l, r) => {
            let lv = eval_const_expr(l, consts)?;
            let rv = eval_const_expr(r, consts)?;
            Some(match op {
                BinOp::Add => lv + rv,
                BinOp::Sub => lv - rv,
                BinOp::Mul => lv * rv,
                BinOp::Div => if rv != 0 { lv / rv } else { 0 },
                BinOp::Mod => if rv != 0 { lv % rv } else { 0 },
                BinOp::Eq => if lv == rv { 1 } else { 0 },
                BinOp::Ne => if lv != rv { 1 } else { 0 },
                BinOp::Lt => if lv < rv { 1 } else { 0 },
                BinOp::Le => if lv <= rv { 1 } else { 0 },
                BinOp::Gt => if lv > rv { 1 } else { 0 },
                BinOp::Ge => if lv >= rv { 1 } else { 0 },
                BinOp::And => if lv != 0 && rv != 0 { 1 } else { 0 },
                BinOp::Or => if lv != 0 || rv != 0 { 1 } else { 0 },
            })
        }
        _ => None,
    }
}
