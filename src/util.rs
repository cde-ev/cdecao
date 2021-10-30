
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
            data:self,
            index: None
        }
    }
}

pub struct KSelectionIterator<'a, T>
{
    k: usize,
    data: &'a[T],
    index: Option<Vec<usize>>,
}

impl<'a, T> Iterator for KSelectionIterator<'a, T> {
    type Item = Vec<&'a T>;

    fn next(&mut self) -> Option<Self::Item> {
        let n = self.data.len();

        // update self.current_index
        if let Some(ref mut index) = self.index {
            let mut j = self.k-1;
            loop {
                if index[j] < n-(self.k-j) {
                    index[j] += 1;
                    break;
                }
                if j == 0 {
                    return None
                }
                j -= 1;
            }
            if j < self.k-1 {
                let v = index[j];
                for l in 1..self.k-j {
                    index[j+l] = v + l;
                }
            }

            // initialization of index
        } else {
            if self.k == 0 || self.k > n {
                return None;
            } else {
                self.index = Some((0..self.k).collect());
            }
        }

        Some(self.index.as_ref().unwrap().iter().map(|i| &self.data[*i]).collect())
    }
}

#[cfg(test)]
mod test {
    use super::IterSelections;

    #[test]
    fn simple_test() {
        let data = [1, 2, 3, 4];
        let selections: Vec<Vec<&i32>> = data[..].iter_selections(3).collect();
        assert_eq!(selections, vec![vec![&1,&2,&3], vec![&1,&2,&4], vec![&1,&3,&4], vec![&2,&3,&4]])
    }

    #[test]
    fn simple_test_owned_data() {
        let data = vec![
            String::from("a"), String::from("b"), String::from("c"), String::from("d")];
        let selections: Vec<Vec<&String>> = data[..].iter_selections(2).collect();
        assert_eq!(selections, vec![
            vec!["a", "b"],
            vec!["a", "c"],
            vec!["a", "d"],
            vec!["b", "c"],
            vec!["b", "d"],
            vec!["c", "d"],
        ])
    }
}
