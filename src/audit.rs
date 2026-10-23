//! Phase 6: audit mode.
//! 1. Clean-build into a scratch dir (cache disabled).
//! 2. Compare per-function artifacts and the final output byte-for-byte.
//! 3. Compute the oracle minimal rebuild set between consecutive versions:
//!    false reuse     = reused, but clean artifact changed (must be 0)
//!    over-invalidate = rebuilt, but clean artifact unchanged

#[derive(Debug, Default)]
pub struct AuditReport {
    pub mismatched_functions: Vec<String>,
    pub over_invalidated: Vec<String>,
}

impl AuditReport {
    pub fn passed(&self) -> bool {
        self.mismatched_functions.is_empty()
    }
}
