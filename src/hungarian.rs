use log::trace;
use ndarray::{Array1, Array2, Axis};

/// Return type of the hungarian algorithm. Represents a mapping of columns to rows (i.e. course places to participants)
/// by storing the matched column index for each row.
pub type Matching = Array1<usize>;

/// Type of the result score (target function value) of the hungarian algorithm
pub type Score = u32;

/// Type to use as edge weights in the adjacency matrix.
///
/// Should not be to long, to allow the whole adjacency matrix to fit into a CPU's cache. The adjacency matrix will
/// consist of n^2 entries of this type, where n is the total number of maximum course places. On the other hand, we
/// must be able to represent the required weights. All actual edge weights must be x times larger than the number of
/// participants, where x is the difference between first course choices edge weight and last course choices edge
/// weight, to ensure that assigning every participant to its last choice is a better solution than assigning any
/// participant to an unchosen course.
///
/// With ten course choices per participant, quadratic weighting (x = 100) and 450 participants, edge weights should be
/// in the range 45001 -- 45101, so u16 is still sufficient. With 50 courses and max 20 places in each, the matrix will
/// be 2MB in size, which is easily cachable.
pub type EdgeWeight = u16;
const LARGE_WEIGHT: EdgeWeight = std::u16::MAX;

pub fn hungarian_algorithm(
    adjacency_matrix: &Array2<EdgeWeight>,
    dummy_x: &Array1<bool>,
    mandatory_y: &Array1<bool>,
    skip_x: &Array1<bool>,
    skip_y: &Array1<bool>,
) -> (Matching, Score) {
    let n = adjacency_matrix.dim().0;

    // Initialize labels
    let mut labels_x = Array1::<EdgeWeight>::zeros([n]);
    let mut labels_y = adjacency_matrix.fold_axis(Axis(1), 0, |acc, x| std::cmp::max(*acc, *x));

    // Current matched y (column) nodes
    let mut m = Array1::<bool>::from_elem([n], false);
    // Current matching (mapping y to their associated x index)
    let mut m_match: Matching = Array1::<usize>::zeros([n]);
    // rows waiting to be matched
    let mut free_x: Vec<usize> = skip_x
        .iter()
        .enumerate()
        .filter(|(_i, skip)| !*skip)
        .map(|(i, _skip)| i)
        .collect();

    // Main loop to construct augmenting paths until matching is perfect
    // -> Chose root u of the alternating tree
    while let Some(u) = free_x.pop() {
        // Reset the node sets of the alternating tree
        // The set of row (X) nodes in the alternating tree
        let mut s = Array1::<bool>::from_elem([n], false);
        s[u] = true;
        // Map of row (X) nodes to their parent's index in the alternating tree
        let mut s_parents = Array1::<usize>::zeros([n]);
        // The set of column (Y) nodes in the alternating tree
        let mut t = Array1::<bool>::from_elem([n], false);
        // Map of column (Y) nodes to their parent's index in the alternating tree
        let mut t_parents = Array1::<usize>::zeros([n]);

        // The neighbourhood of S in the equalitygraph, without nodes already in T. -> N_l(S) \ T
        // It is updated dynamically when Nodes are added to S and T
        // TODO improve performance by using ndarray's elementwise operations?
        let mut nlxt = Array1::from_shape_fn([n], |y| {
            !skip_y[y]
                && adjacency_matrix[[u, y]] == labels_x[u] + labels_y[y]
                && (!dummy_x[u] || !mandatory_y[y])
        });
        let mut nlxt_neighbour_of = Array1::from_elem([n], u);

        // Loop to construct alternating tree (incl. updating of labels), until augmenting path is found
        loop {
            // Try to get next neighbour of S in the equality graph. If there is none, update the labels
            let mut y = nlxt.iter().position(|x| *x);
            if let None = y {
                // To update the labels, calculate minimal delta between edge weight and sum of nodes' labels. In the
                // same turn, we can keep track of the new equality graph neighbourhood.
                let mut delta_min = LARGE_WEIGHT;
                for ((x, y), weight) in adjacency_matrix.indexed_iter() {
                    if s[x] && !t[y] && !skip_y[y] && (!dummy_x[u] || !mandatory_y[y]) {
                        let delta = labels_x[x] + labels_y[y] - weight;
                        if delta == delta_min {
                            nlxt[y] = true;
                            nlxt_neighbour_of[y] = x;
                        } else if delta < delta_min {
                            nlxt.fill(false);
                            nlxt[y] = true;
                            nlxt_neighbour_of[y] = x;
                            delta_min = delta;
                        }
                    }
                }

                labels_x -= &s.map(|cond| if *cond { delta_min } else { 0 });
                labels_y += &t.map(|cond| if *cond { delta_min } else { 0 });

                // Now, there must be a neighbour
                y = nlxt.iter().position(|x| *x);
            }

            let y = y.unwrap();
            // Now, extend the alternating tree with this neighbour y
            if m[y] {
                // Add y and its current partner to alternating tree
                t[y] = true;
                t_parents[y] = nlxt_neighbour_of[y];
                let z = m_match[y];
                s[z] = true;
                s_parents[z] = y;

                // Update neighbourhood
                nlxt[y] = false;
                // TODO improve performance by using ndarray's zip etc.?
                for yy in 0..n {
                    if !t[yy]
                        && !skip_y[yy]
                        && adjacency_matrix[[u, y]] == labels_x[u] + labels_y[y]
                        && (!dummy_x[u] || !mandatory_y[y])
                    {
                        nlxt[yy] = true;
                        nlxt_neighbour_of[yy] = z;
                    }
                }
            } else {
                // Yay, our alternating tree contains an augmenting path. Let's reconstruct it and augment the
                // matching with it.
                let mut yy = y;
                let mut xx = nlxt_neighbour_of[yy];
                let mut i = 1; //only for debugging
                m[yy] = true;
                loop {
                    m_match[yy] = xx;
                    if xx == u {
                        break;
                    }
                    yy = s_parents[xx];
                    xx = t_parents[yy];
                    i += 2;
                }
                trace!("Added {}~{} with {}-ary aug. path", xx, yy, i);
                break;
            }
        }
    }

    return (m_match, 0);

// =============================================================================
// Tests
#[cfg(test)]
mod tests {
    use super::{hungarian_algorithm, EdgeWeight};
    use ndarray::{Array1, Array2};

    #[test]
    fn minimal_matching_problem() {
        // X = {1, 2, 3, 4}
        // Xs = {5, 6}
        // Y = {'a1', 'a2', 'b1', 'b2', 'b3', 'c1'}

        let mut adjacency_matrix = Array2::<EdgeWeight>::zeros([6, 6]);
        adjacency_matrix[[0, 5]] = 1005;
        adjacency_matrix[[0, 2]] = 1000;
        adjacency_matrix[[0, 3]] = 1000;
        adjacency_matrix[[0, 4]] = 1000;
        adjacency_matrix[[1, 5]] = 1005;
        adjacency_matrix[[1, 0]] = 1000;
        adjacency_matrix[[1, 1]] = 1000;
        adjacency_matrix[[2, 2]] = 1005;
        adjacency_matrix[[2, 3]] = 1005;
        adjacency_matrix[[2, 4]] = 1005;
        adjacency_matrix[[2, 5]] = 1000;
        adjacency_matrix[[3, 5]] = 1005;
        adjacency_matrix[[3, 2]] = 1000;
        adjacency_matrix[[3, 3]] = 1000;
        adjacency_matrix[[3, 4]] = 1000;

        let mandatory_y = Array1::from_vec(vec![false, false, true, true, true, false]);
        let dummy_x = Array1::from_vec(vec![false, false, false, false, true, true]);

        let (matching, score) = hungarian_algorithm(
            &adjacency_matrix,
            &dummy_x,
            &mandatory_y,
            &Array1::from_elem([6], false),
            &Array1::from_elem([6], false),
        );

        assert_eq!(matching.len(), 6);
        assert!(score > 4000);

        // Since 2,3,4 are mandatory course places, participants 0,2,3 must fill those and participant 1 show reach
        // place 5
        print!("{:?}", matching);
        assert_eq!(matching[5], 1);
    }

}
