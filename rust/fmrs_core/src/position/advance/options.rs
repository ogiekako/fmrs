#[derive(Debug, Clone, Default)]
pub struct AdvanceOptions {
    // Set 1 for one-way mate. Pawn drop is added regardless of the value.
    pub max_allowed_branches: Option<usize>,
    /// Caller asserts the side-to-move's king is NOT in check; skip the
    /// `attacker()` call in advance.
    ///
    /// Must be conservative: only set true when DEFINITELY not in check.
    /// False = unknown, full attacker() call performed.
    pub assume_not_in_check: bool,
}

/// Lightweight error for hot-path control flow in advance_aux.
///
/// Constructing an `anyhow::Error` boxes the inner error, which showed up at
/// ~5% of CPU in profiles when `max_allowed_branches: Some(0)` was used as a
/// "stop on first branch" signal in `solutions_overlay_inner`. This Copy
/// enum is allocation-free; conversions to `anyhow::Error` are only paid by
/// callers that actually use `?` against `anyhow::Result`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AdvanceError {
    TooManyBranches,
    NoAttacker,
}

impl std::fmt::Display for AdvanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdvanceError::TooManyBranches => f.write_str("too many branches"),
            AdvanceError::NoAttacker => f.write_str("no attacker found"),
        }
    }
}

impl std::error::Error for AdvanceError {}

pub type AdvanceResult<T> = std::result::Result<T, AdvanceError>;

impl AdvanceOptions {
    pub(crate) fn check_allowed_branches(
        &self,
        branches_without_pawn_drop: usize,
    ) -> AdvanceResult<()> {
        if let Some(max_allowed_branches) = self.max_allowed_branches {
            if branches_without_pawn_drop > max_allowed_branches {
                return Err(AdvanceError::TooManyBranches);
            }
        }
        Ok(())
    }
}
