/// Return type of the hungarian algorithm. Represents a mapping of rows to columns (i.e. participants to course places)
/// by storing the matched column index for each row.
pub type Matching = ndarray::Array1<usize>;

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

pub fn hungarian_algorithm(
    adjacency_matrix: &ndarray::Array2<EdgeWeight>,
    dummy_x: &ndarray::Array1<bool>,
    mandatory_y: &ndarray::Array1<bool>,
    skip_x: &ndarray::Array1<bool>,
    skip_y: &ndarray::Array1<bool>,
) -> (Matching, Score) {
    // TODO
    (ndarray::Array1::<usize>::zeros([0]), 0)
}
