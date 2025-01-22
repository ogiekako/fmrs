use anyhow::{bail, Result};

#[derive(Debug, Clone, Default)]
pub struct AdvanceOptions {
    // Set 1 for one-way mate. Pawn drop is added regardless of the value.
    pub max_allowed_branches: Option<usize>,
}

impl AdvanceOptions {
    pub(crate) fn check_allowed_branches(&self, branches_without_pawn_drop: usize) -> Result<()> {
        if let Some(max_allowed_branches) = self.max_allowed_branches {
            if branches_without_pawn_drop > max_allowed_branches {
                bail!("Too many branches");
            }
        }
        Ok(())
    }
}
