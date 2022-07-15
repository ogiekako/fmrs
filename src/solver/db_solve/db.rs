use sled::Mode;
use sysinfo::SystemExt;

pub struct Database {
    tree: sled::Db,
}

impl Database {
    pub fn new() -> anyhow::Result<Self> {
        let config = sled::Config::default()
            .mode(Mode::HighThroughput)
            .temporary(true)
            .cache_capacity(sysinfo::System::new_all().available_memory() * 1024);
        let db = config.open()?;
        Ok(Self { tree: db })
    }

    pub fn insert(&self, digest: u64, step: i32) -> anyhow::Result<()> {
        self.tree
            .insert(&digest.to_be_bytes(), &step.to_be_bytes())?;
        Ok(())
    }

    pub fn get(&self, digest: &u64) -> anyhow::Result<Option<i32>> {
        let value = self.tree.get(&digest.to_be_bytes())?;
        Ok(value.map(|x| i32::from_be_bytes(x.as_ref().try_into().unwrap())))
    }

    pub fn contains_key(&self, digest: &u64) -> anyhow::Result<bool> {
        Ok(self.tree.contains_key(&digest.to_be_bytes())?)
    }
}

#[cfg(test)]
mod tests {
    use super::Database;

    #[test]
    fn insert_get() {
        let db = Database::new().unwrap();
        db.insert(1, 2).unwrap();
        db.insert(u64::MAX, 3).unwrap();
        assert_eq!(db.get(&1).unwrap(), 2.into());
        assert_eq!(db.get(&2).unwrap(), None);
        assert_eq!(db.get(&u64::MAX).unwrap(), 3.into());
    }
}
