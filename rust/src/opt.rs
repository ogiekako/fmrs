use std::{collections::HashMap, ops::RangeInclusive};

#[derive(Debug, Clone)]
pub struct BoxKd(Vec<RangeInclusive<i32>>);

impl From<Vec<RangeInclusive<i32>>> for BoxKd {
    fn from(ranges: Vec<RangeInclusive<i32>>) -> Self {
        Self(ranges)
    }
}

impl BoxKd {
    pub fn new(ranges: Vec<RangeInclusive<i32>>) -> Self {
        Self(ranges)
    }

    fn split(&self, i: usize) -> (Self, Self) {
        let (mut a, mut b) = (self.0.clone(), self.0.clone());

        let m = (self.0[i].start() + self.0[i].end()) / 2;
        a[i] = *self.0[i].start()..=m;
        b[i] = m + 1..=*self.0[i].end();

        (Self::new(a), Self::new(b))
    }

    pub fn left(&self) -> Vec<i32> {
        self.0.iter().map(|r| *r.start()).collect()
    }

    pub fn right(&self) -> Vec<i32> {
        self.0.iter().map(|r| *r.end()).collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = Vec<i32>> + '_ {
        BoxKdIter {
            inner: self,
            cur: self.left().into(),
        }
    }

    fn len(&self) -> usize {
        self.0.len()
    }
}

struct BoxKdIter<'a> {
    inner: &'a BoxKd,
    cur: Option<Vec<i32>>,
}

impl Iterator for BoxKdIter<'_> {
    type Item = Vec<i32>;

    fn next(&'_ mut self) -> Option<Self::Item> {
        if let Some(cur) = self.cur.as_mut() {
            let res = cur.clone();
            for i in 0..cur.len() {
                if cur[i] < *self.inner.0[i].end() {
                    cur[i] += 1;
                    for j in 0..i {
                        cur[j] = *self.inner.0[j].start();
                    }
                    return Some(res);
                }
            }
            self.cur = None;
            Some(res)
        } else {
            None
        }
    }
}

// f: Z^k -> {-1, 0, 1}
// f(x) >= c && f(x + ei) >= c => f(y) >= c (y >= x)
// f(x) <= c && f(x - ei) <= c => f(y) <= c (y <= x)
pub fn zero_region_of_almost_monotone_func<I: FnMut(&[i32]) -> i8>(
    f: I,
    region: &BoxKd,
    strictly_monotone: u64,
) -> Vec<BoxKd> {
    let mut finder = ZeroFinder::new(f, strictly_monotone);
    finder.find_zero_region(region);
    finder.res
}

struct ZeroFinder<I: FnMut(&[i32]) -> i8> {
    memo: HashMap<Vec<i32>, i8>,
    res: Vec<BoxKd>,
    f: I,
    strictly_monotone: u64,
}

impl<I: FnMut(&[i32]) -> i8> ZeroFinder<I> {
    fn new(f: I, strictly_monotone: u64) -> Self {
        Self {
            memo: HashMap::new(),
            res: Vec::new(),
            f,
            strictly_monotone,
        }
    }

    fn f(&mut self, x: &Vec<i32>) -> i8 {
        if let Some(i) = self.memo.get(x) {
            return *i;
        }
        let i = (self.f)(x);
        self.memo.insert(x.clone(), i);
        i
    }

    fn is_strictly_monotone_index(&self, i: usize) -> bool {
        self.strictly_monotone & 1 << i != 0
    }

    fn find_zero_region(&mut self, region: &BoxKd) {
        let l = region.left();
        let r = region.right();
        let fl = self.f(&l);
        if l == r {
            if fl == 0 {
                self.res.push(region.clone());
            }
            return;
        }
        let fr = self.f(&r);
        if fl == 1 && fr == 1 {
            if (0..region.len()).all(|i| {
                if self.is_strictly_monotone_index(i) || l[i] == r[i] {
                    return true;
                }
                let mut l2 = l.clone();
                l2[i] += 1;
                self.f(&l2) == 1
            }) {
                // #[cfg(debug_assertions)]
                // {
                //     for x in region.iter() {
                //         assert_eq!(self.f(&x), 1);
                //     }
                // }

                return;
            }
        } else if fl == -1 && fr == -1 {
            if (0..region.len()).all(|i| {
                if self.is_strictly_monotone_index(i) || l[i] == r[i] {
                    return true;
                }
                let mut r2 = r.clone();
                r2[i] -= 1;
                self.f(&r2) == -1
            }) {
                // #[cfg(debug_assertions)]
                // {
                //     for x in region.iter() {
                //         assert_eq!(self.f(&x), -1, "{:?} {:?}", region, x);
                //     }
                // }

                return;
            }
        } else if fl == 0
            && fr == 0
            && (0..region.len()).all(|i| {
                if self.is_strictly_monotone_index(i) || l[i] == r[i] {
                    return true;
                }
                let mut l2 = l.clone();
                l2[i] += 1;
                let mut r2 = r.clone();
                r2[i] -= 1;
                self.f(&l2) == 0 && self.f(&r2) == 0
            })
        {
            // #[cfg(debug_assertions)]
            // {
            //     for x in region.iter() {
            //         assert_eq!(self.f(&x), 0);
            //     }
            // }

            self.res.push(region.clone());
            return;
        }
        let (a, b) = region.split((0..region.len()).max_by_key(|&i| r[i] - l[i]).unwrap());

        self.find_zero_region(&a);
        self.find_zero_region(&b);
    }
}

#[cfg(test)]
mod tests {
    use crate::opt::{zero_region_of_almost_monotone_func, BoxKd};

    #[test]
    fn test_zero_region_of_almost_monotone_func() {
        let v = [
            [-1, 0, 0, 0, 1, 1, 1, 1],
            [-1, -1, 1, 1, 1, 1, 1, 1],
            [0, 1, 0, 1, 1, 1, 1, 1],
            [1, 0, 1, 1, 1, 1, 1, 1],
        ];

        let region = BoxKd::new(vec![0..=3, 0..=7]);

        let mut called = 0;
        let res = zero_region_of_almost_monotone_func(
            |x| {
                called += 1;
                v[x[0] as usize][x[1] as usize]
            },
            &region,
            0,
        );
        let mut xy = res
            .into_iter()
            .flat_map(|r| r.iter().map(|x| (x[0], x[1])).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        xy.sort();

        assert_eq!(xy, vec![(0, 1), (0, 2), (0, 3), (2, 0), (2, 2), (3, 1)]);
        assert!(called < 32);
    }
}
