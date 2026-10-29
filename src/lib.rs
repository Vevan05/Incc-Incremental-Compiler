//! incc: self-checking, interface-aware incremental compiler.
//! The spec and phase plan are documented in the project README.

pub mod ast;
pub mod audit;
pub mod cache;
pub mod codegen;
pub mod deps;
pub mod driver;
pub mod fingerprint;
pub mod ir;
pub mod lexer;
pub mod metrics;
pub mod parser;
pub mod sema;

/// Compiler identity included in every cache key. Replace with a hash of the
/// compiler binary / build id so stale caches are never reused (plan, Phase 4 step 7).
pub const COMPILER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Parse(String),
    Sema(String),
    NotImplemented(&'static str),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Parse(m) => write!(f, "parse error: {m}"),
            Error::Sema(m) => write!(f, "semantic error: {m}"),
            Error::NotImplemented(w) => write!(f, "not implemented yet: {w}"),
        }
    }
}

impl std::error::Error for Error {}
