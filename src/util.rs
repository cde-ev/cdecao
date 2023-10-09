pub trait IterSelections<'a, T> {
    /// Iterate all possible selections with k elements from the elements of this collection
    ///
    /// Returns an iterator which returns Vec's of borrowed elements from the collection.
    /// The iterator is empty if k == 0 or k > n, where n is the length of the collection.
    /// Otherwise it will return (n chose k) selections.
    ///
    /// The iterator keeps an internal vector of `k` usize entries. Thus, the memory consumption is
    /// linear in k. The runtime of the next() method is linear in k as well.
    fn iter_selections(&'a self, k: usize) -> KSelectionIterator<'a, T>;
}

impl<'a, T> IterSelections<'a, T> for [T] {
    fn iter_selections(&'a self, k: usize) -> KSelectionIterator<'a, T> {
        KSelectionIterator {
            k,
            data: self,
            index: None,
        }
    }
}

pub struct KSelectionIterator<'a, T> {
    k: usize,
    data: &'a [T],
    index: Option<Vec<usize>>,
}

impl<'a, T> Iterator for KSelectionIterator<'a, T> {
    type Item = Vec<&'a T>;

    fn next(&mut self) -> Option<Self::Item> {
        let n = self.data.len();

        // update self.index
        if let Some(ref mut index) = self.index {
            let mut j = 0;
            loop {
                if j == self.k - 1 {
                    if index[j] >= n - 1 {
                        return None;
                    }
                    index[j] += 1;
                    break;
                } else if index[j] < index[j + 1] - 1 {
                    index[j] += 1;
                    break;
                }
                j += 1;
            }
            for k in 0..j {
                index[k] = k;
            }

        // empty iterator
        } else if self.k == 0 || self.k > n {
            return None;

        // initialization of index
        } else {
            self.index = Some((0..self.k).collect());
        }

        Some(
            self.index
                .as_ref()
                .unwrap()
                .iter()
                .map(|i| &self.data[*i])
                .collect(),
        )
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if let Some(ref index) = self.index {
            let mut rank = 0;
            for (i, index_entry) in index.iter().enumerate() {
                rank += binom(*index_entry, i + 1);
            }
            let remaining = binom(self.data.len(), self.k) - rank - 1;

            (remaining, Some(remaining))
        } else {
            let num = binom(self.data.len(), self.k);

            (num, Some(num))
        }
    }
}

pub fn binom(n: usize, k: usize) -> usize {
    if k > n {
        return 0;
    }
    let mut res = 1usize;
    for i in 0..k {
        res = res * (n - i) / (i + 1);
    }

    res
}

#[cfg(test)]
mod test {
    use super::binom;
    use super::IterSelections;

    #[test]
    fn simple_test() {
        let data = [1, 2, 3, 4];
        let selections: Vec<Vec<&i32>> = data[..].iter_selections(3).collect();
        assert_eq!(
            selections,
            vec![
                vec![&1, &2, &3],
                vec![&1, &2, &4],
                vec![&1, &3, &4],
                vec![&2, &3, &4]
            ]
        )
    }

    #[test]
    fn simple_test_owned_data() {
        let data = vec![
            String::from("a"),
            String::from("b"),
            String::from("c"),
            String::from("d"),
        ];
        let selections: Vec<Vec<&String>> = data[..].iter_selections(2).collect();
        assert_eq!(
            selections,
            vec![
                vec!["a", "b"],
                vec!["a", "c"],
                vec!["b", "c"],
                vec!["a", "d"],
                vec!["b", "d"],
                vec!["c", "d"],
            ]
        )
    }

    #[test]
    fn binom_test() {
        assert_eq!(binom(4, 4), 1);
        assert_eq!(binom(10, 9), 10);
        assert_eq!(binom(4, 2), 6);
        assert_eq!(binom(4, 1), 4);
        assert_eq!(binom(4, 0), 1);
        assert_eq!(binom(0, 0), 1);
        assert_eq!(binom(3, 4), 0);
    }

    #[test]
    fn size_hint_test() {
        let data = [1, 2, 3, 4];
        let mut iterator = data[..].iter_selections(2);
        assert_eq!(iterator.size_hint().0, 6);
        assert_eq!(iterator.size_hint().1, Some(6));
        for i in 0..6 {
            iterator.next();
            assert_eq!(iterator.size_hint().0, 6 - i - 1);
        }
    }
}
