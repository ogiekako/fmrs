use anyhow::{bail, Result};

#[derive(Debug, Clone, Default)]
pub struct AdvanceOptions {
    pub no_memo: bool,
    // Set 1 for one-way mate.
    pub max_allowed_branches: Option<usize>,
}

impl AdvanceOptions {
    pub(crate) fn check_allowed_branches(&self, branches: usize) -> Result<()> {
        if let Some(max_allowed_branches) = self.max_allowed_branches {
            if branches > max_allowed_branches {
                bail!("Too many branches");
            }
        }
        Ok(())
    }
}
