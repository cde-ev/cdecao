use log::trace;
use ndarray::{Array1, Array2, Axis, Zip};

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
    let nx = adjacency_matrix.dim().0;
    let ny = adjacency_matrix.dim().1;

    // In debug build: Check sizes
    if cfg!(debug_assertions) {
        assert_eq!(dummy_x.dim(), nx);
        assert_eq!(skip_x.dim(), nx);
        assert_eq!(mandatory_y.dim(), ny);
        assert_eq!(skip_y.dim(), ny);
        let count_skip_x = skip_x.fold(0, |acc, x| if *x { acc + 1 } else { acc });
        let count_skip_y = skip_y.fold(0, |acc, x| if *x { acc + 1 } else { acc });
        assert_eq!(nx - count_skip_x, ny - count_skip_y);
    }

    // Initialize labels
    let mut labels_x = adjacency_matrix.fold_axis(Axis(1), 0, |acc, x| std::cmp::max(*acc, *x));
    let mut labels_y = Array1::<EdgeWeight>::zeros([ny]);

    // Current matched y (column) nodes
    let mut m = Array1::<bool>::from_elem([ny], false);
    // Current matching (mapping y to their associated x index)
    let mut m_match: Matching = Array1::<usize>::zeros([ny]);
    // Indices of rows waiting to be matched
    let mut free_x: Vec<usize> = skip_x
        .indexed_iter()
        .filter(|(_i, skip)| !*skip)
        .map(|(i, _skip)| i)
        .collect();

    // Main loop to construct augmenting paths until matching is perfect
    // -> Chose root u of the alternating tree
    while let Some(u) = free_x.pop() {
        // Reset the node sets of the alternating tree
        // The set of row (X) nodes in the alternating tree
        let mut s = Array1::<bool>::from_elem([nx], false);
        s[u] = true;
        // Map of row (X) nodes to their parent's index in the alternating tree
        let mut s_parents = Array1::<usize>::zeros([nx]);
        // The set of column (Y) nodes in the alternating tree
        let mut t = Array1::<bool>::from_elem([ny], false);
        // Map of column (Y) nodes to their parent's index in the alternating tree
        let mut t_parents = Array1::<usize>::zeros([ny]);

        // The neighbourhood of S in the equalitygraph, without nodes already in T. -> N_l(S) \ T
        // It is updated dynamically when Nodes are added to S and T
        let mut nlxt = !skip_y;
        Zip::from(&mut nlxt)
            .and(adjacency_matrix.index_axis(Axis(0), u))
            .and(&labels_y)
            .and(mandatory_y)
            .apply(|w, &a, &l, &m| {
                *w = (a == labels_x[u] + l && (!dummy_x[u] || !m));
            });
        let mut nlxt_neighbour_of = Array1::from_elem([ny], u);

        // Loop to construct alternating tree (incl. updating of labels), until augmenting path is found
        loop {
            // Try to get next neighbour of S in the equality graph. If there is none, update the labels
            let mut y = nlxt.iter().position(|x| *x);
            if let None = y {
                // To update the labels, calculate minimal delta between edge weight and sum of nodes' labels. In the
                // same turn, we can keep track of the new equality graph neighbourhood:
                // After the updates, the neighbourhood consists of Y-nodes, not in T, connected to X-nodes in S via
                // edges that have currently the same delta beetwen edgeweight and node labels.
                let mut delta_min = LARGE_WEIGHT;
                for (x, s_x) in s.indexed_iter() {
                    if *s_x {
                        for (y, weight) in adjacency_matrix.index_axis(Axis(0), x).indexed_iter() {
                            // TODO speed up, use ndarray::Zip?
                            if !t[y] && !skip_y[y] && (!dummy_x[x] || !mandatory_y[y]) {
                                let delta = labels_x[x] + labels_y[y] - weight;
                                if delta == delta_min {
                                    // Y-Node with edge with same delta found. Add it to the new neighbourhood.
                                    nlxt[y] = true;
                                    nlxt_neighbour_of[y] = x;
                                } else if delta < delta_min {
                                    // New minimal delta found. Update delta and clear new neighbourhood.
                                    nlxt.fill(false);
                                    nlxt[y] = true;
                                    nlxt_neighbour_of[y] = x;
                                    delta_min = delta;
                                }
                            }
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

                // Update neighbourhood with equalitygraph-neighbours of z
                nlxt[y] = false;
                Zip::from(&mut nlxt)
                    .and(&mut nlxt_neighbour_of)
                    .and(&(!skip_y & !&t)) // A little trick, because ndarray::Zip only takes 6 Arrays
                    .and(adjacency_matrix.index_axis(Axis(0), z))
                    .and(&labels_y)
                    .and(mandatory_y)
                    .apply(|v, w, &t_nor_s, &a, &l, &m| {
                        if t_nor_s && a == labels_x[z] + l && (!dummy_x[z] || !m) {
                            *v = true;
                            *w = z;
                        }
                    });
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
                trace!("Added {}~{} with {}-ary aug. path\n", xx, yy, i);
                break;
            }
        }
    }

    // Calculate score and return results
    let score = m_match
        .indexed_iter()
        .map(|(y, x)| adjacency_matrix[(*x, y)] as Score)
        .fold(Score::from(0u8), |acc, x| acc + x);
    return (m_match, score);
}

// =============================================================================
// Tests
#[cfg(test)]
mod tests {
    use super::{hungarian_algorithm, EdgeWeight};
    use ndarray::{Array1, Array2};

    #[test]
    fn minimal_matching_problem() {
        // X = {0, 1, skip, 3, 4}
        // Xs = {5, 6}
        // Y = {'a1', 'a2', 'b1', 'b2', 'b3', 'c1', skip}

        let mut adjacency_matrix = Array2::<EdgeWeight>::zeros([7, 7]);
        adjacency_matrix[[0, 5]] = 1005;
        adjacency_matrix[[0, 2]] = 1000;
        adjacency_matrix[[0, 3]] = 1000;
        adjacency_matrix[[0, 4]] = 1000;
        adjacency_matrix[[1, 5]] = 1005;
        adjacency_matrix[[1, 0]] = 1000;
        adjacency_matrix[[1, 1]] = 1000;
        adjacency_matrix[[3, 2]] = 1005;
        adjacency_matrix[[3, 3]] = 1005;
        adjacency_matrix[[3, 4]] = 1005;
        adjacency_matrix[[3, 5]] = 1000;
        adjacency_matrix[[4, 5]] = 1005;
        adjacency_matrix[[4, 2]] = 1000;
        adjacency_matrix[[4, 3]] = 1000;
        adjacency_matrix[[4, 4]] = 1000;

        let mandatory_y = Array1::from_vec(vec![false, false, true, true, true, false, false]);
        let dummy_x = Array1::from_vec(vec![false, false, false, false, false, true, true]);
        let skip_x = Array1::from_vec(vec![false, false, true, false, false, false, false]);
        let skip_y = Array1::from_vec(vec![false, false, false, false, false, false, true]);

        let (matching, score) =
            hungarian_algorithm(&adjacency_matrix, &dummy_x, &mandatory_y, &skip_x, &skip_y);

        assert_eq!(matching.len(), 7);
        assert!(score > 4000);

        // Since 2,3,4 are mandatory course places, participants 0,3,4 must fill those and participant 1 should gain
        // place 5
        print!("{:?}", matching);
        assert_eq!(matching[5], 1);
    }

    #[test]
    fn larger_matching_problem() {
        const NUM_COURSES: usize = 30;
        const PLACES_PER_COURSE: usize = 10;
        const NUM_PARTICIPANTS: usize = 200;
        const WEIGHT_OFFSET: u16 = 50000;
        const CHOICES: usize = 3;

        let n = NUM_COURSES * PLACES_PER_COURSE;
        let mut dummy_x = Array1::<bool>::from_elem([n], false);
        for i in NUM_PARTICIPANTS..n {
            dummy_x[i] = true;
        }

        let mut adjacency_matrix = Array2::<EdgeWeight>::zeros([n, n]);
        // Every participant chooses three different courses, but always one from the first third of courses
        for p in 0..NUM_PARTICIPANTS {
            for choice in 0..CHOICES {
                let course = if choice == 0 {
                    p % NUM_COURSES / 3
                } else {
                    (p + choice) % NUM_COURSES
                };
                for place in (course * PLACES_PER_COURSE)..((course + 1) * PLACES_PER_COURSE) {
                    adjacency_matrix[(p, place)] = WEIGHT_OFFSET - choice as EdgeWeight;
                }
            }
        }

        let (matching, score) = hungarian_algorithm(
            &adjacency_matrix,
            &dummy_x,
            &Array1::<bool>::from_elem([n], false),
            &Array1::<bool>::from_elem([n], false),
            &Array1::<bool>::from_elem([n], false),
        );

        assert_eq!(matching.len(), n);
        // All participants should at least be scored with the worst choice
        assert!(score as usize >= (WEIGHT_OFFSET as usize - CHOICES) * NUM_PARTICIPANTS);
        // TODO check, that enough participants got their 1. choice

        // Every participant must be assigned to one course place
        let mut is_assigned = Array1::<bool>::from_elem([n], false);
        for (cp, p) in matching.indexed_iter() {
            assert!(!is_assigned[*p], "participant {} is assigned to course place {} and another course place", p, cp);
            is_assigned[*p] = true;
        }
        for (p, ia) in is_assigned.indexed_iter() {
            assert!(ia, "participant {} is not assigned to any course place", p);
        }
    }
}
