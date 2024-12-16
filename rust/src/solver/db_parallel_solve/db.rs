use sled::Mode;

#[derive(Clone)]
pub(super) struct Database {
    tree: sled::Db,
}

pub(super) trait DatabaseGet {
    fn get(&self, digest: &u64) -> anyhow::Result<Option<i32>>;
}

impl DatabaseGet for Database {
    fn get(&self, digest: &u64) -> anyhow::Result<Option<i32>> {
        let value = self.tree.get(&digest.to_be_bytes())?;
        Ok(value.map(|x| i32::from_be_bytes(x.as_ref().try_into().unwrap())))
    }
}

const GB: i64 = 1024 * 1024 * 1024;

impl Database {
    pub fn new() -> anyhow::Result<Self> {
        let tempdir = tempfile::tempdir()?;
        let available_memory = sysinfo::System::new_all().available_memory() * 1024;
        let cache_capacity = (available_memory as i64 - 64 * GB).max(GB) as u64;
        let config = sled::Config::default()
            .path(tempdir)
            .mode(Mode::HighThroughput)
            .temporary(true)
            .cache_capacity(cache_capacity);
        let db = config.open()?;
        Ok(Self { tree: db })
    }

    // If digest is contained, does nothing and returns true.
    // Otherwise, updates the value and returns false.
    pub fn insert_if_empty(&self, digest: u64, step: i32) -> anyhow::Result<bool> {
        let res = self.tree.compare_and_swap(
            digest.to_be_bytes(),
            None as Option<&[u8]>,
            Some(&step.to_be_bytes()),
        )?;
        Ok(res.is_err())
    }
}

#[cfg(test)]
mod tests {
    use crate::solver::db_parallel_solve::db::DatabaseGet;

    use super::Database;

    #[test]
    fn insert_get() {
        let db = Database::new().unwrap();
        db.insert_if_empty(1, 2).unwrap();
        db.insert_if_empty(u64::MAX, 3).unwrap();
        assert_eq!(db.get(&1).unwrap(), 2.into());
        assert_eq!(db.get(&2).unwrap(), None);
        assert_eq!(db.get(&u64::MAX).unwrap(), 3.into());
    }
}
