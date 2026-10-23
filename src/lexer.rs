//! Phase 1: hand-written lexer.
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    KwInt, KwBool, KwVoid, KwConst, KwIf, KwElse, KwWhile, KwReturn, KwTrue, KwFalse,
    Ident(String),
    IntLit(i64),
    Plus, Minus, Star, Slash, Percent,
    EqEq, BangEq, Le, Ge, Lt, Gt,
    AndAnd, OrOr, Bang,
    Eq,
    LParen, RParen, LBrace, RBrace, Semi, Comma,
    Eof,
}

pub fn lex(src: &str) -> Result<Vec<Token>> {
    let mut toks = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < bytes.len() && bytes[i+1] as char == '/' {
            // line comment
            while i < bytes.len() && bytes[i] as char != '\n' { i += 1; }
            continue;
        }
        if c == '/' && i + 1 < bytes.len() && bytes[i+1] as char == '*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] as char == '*' && bytes[i+1] as char == '/') { i += 1; }
            i += 2;
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < bytes.len() && (bytes[i] as char).is_ascii_alphanumeric() || (bytes[i] as char == '_') { i += 1; }
            let word = &src[start..i];
            let tok = match word {
                "int" => Token::KwInt,
                "bool" => Token::KwBool,
                "void" => Token::KwVoid,
                "const" => Token::KwConst,
                "if" => Token::KwIf,
                "else" => Token::KwElse,
                "while" => Token::KwWhile,
                "return" => Token::KwReturn,
                "true" => Token::KwTrue,
                "false" => Token::KwFalse,
                _ => Token::Ident(word.to_string()),
            };
            toks.push(tok);
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i] as char).is_ascii_digit() { i += 1; }
            let n: i64 = src[start..i].parse().map_err(|_| Error::Parse("bad int literal".into()))?;
            toks.push(Token::IntLit(n));
            continue;
        }
        // two-char operators
        let two = if i + 1 < bytes.len() { &src[i..i+2] } else { "" };
        match two {
            "==" => { toks.push(Token::EqEq); i += 2; continue; }
            "!=" => { toks.push(Token::BangEq); i += 2; continue; }
            "<=" => { toks.push(Token::Le); i += 2; continue; }
            ">=" => { toks.push(Token::Ge); i += 2; continue; }
            "&&" => { toks.push(Token::AndAnd); i += 2; continue; }
            "||" => { toks.push(Token::OrOr); i += 2; continue; }
            _ => {}
        }
        match c {
            '+' => { toks.push(Token::Plus); i += 1; }
            '-' => { toks.push(Token::Minus); i += 1; }
            '*' => { toks.push(Token::Star); i += 1; }
            '/' => { toks.push(Token::Slash); i += 1; }
            '%' => { toks.push(Token::Percent); i += 1; }
            '!' => { toks.push(Token::Bang); i += 1; }
            '=' => { toks.push(Token::Eq); i += 1; }
            '<' => { toks.push(Token::Lt); i += 1; }
            '>' => { toks.push(Token::Gt); i += 1; }
            '(' => { toks.push(Token::LParen); i += 1; }
            ')' => { toks.push(Token::RParen); i += 1; }
            '{' => { toks.push(Token::LBrace); i += 1; }
            '}' => { toks.push(Token::RBrace); i += 1; }
            ';' => { toks.push(Token::Semi); i += 1; }
            ',' => { toks.push(Token::Comma); i += 1; }
            _ => return Err(Error::Parse(format!("unexpected char: {c}"))),
        }
    }
    toks.push(Token::Eof);
    Ok(toks)
}
