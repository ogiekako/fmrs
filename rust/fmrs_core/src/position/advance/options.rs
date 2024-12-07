use anyhow::{bail, Result};

use crate::position::Position;

#[derive(Debug, Clone, Default)]
pub struct AdvanceOptions {
    // Set 1 for one-way mate.
    pub max_allowed_branches: Option<usize>,
}

impl AdvanceOptions {
    pub(crate) fn check_allowed_branches(&self, branches: &[Position]) -> Result<()> {
        if let Some(max_allowed_branches) = self.max_allowed_branches {
            if branches.len() > max_allowed_branches {
                bail!("Too many branches");
            }
        }
        Ok(())
    }
}
