pub struct Sqlite {
    tree: sled::Db,
}

const DB_NAME: &str = "./solve.db";

impl Sqlite {
    pub fn new() -> anyhow::Result<Self> {
        if std::path::Path::new(DB_NAME).exists() {
            std::fs::remove_dir_all(DB_NAME)?;
        }
        let conn = sled::open(DB_NAME)?;
        Ok(Self { tree: conn })
    }

    pub fn insert(&self, digest: u64, step: i32) -> anyhow::Result<()> {
        self.tree
            .insert(digest.to_ne_bytes(), &step.to_ne_bytes())?;
        Ok(())
    }

    pub fn get(&self, digest: &u64) -> anyhow::Result<Option<i32>> {
        let value = self.tree.get(digest.to_ne_bytes())?;
        Ok(value.map(|x| i32::from_ne_bytes(x.as_ref().try_into().unwrap())))
    }

    pub fn contains_key(&self, digest: &u64) -> anyhow::Result<bool> {
        Ok(self.get(digest)?.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::Sqlite;

    #[test]
    fn insert_get() {
        let db = Sqlite::new().unwrap();
        db.insert(1, 2).unwrap();
        db.insert(u64::MAX, 3).unwrap();
        assert_eq!(db.get(&1).unwrap(), 2.into());
        assert_eq!(db.get(&2).unwrap(), None);
        assert_eq!(db.get(&u64::MAX).unwrap(), 3.into());
    }
}
