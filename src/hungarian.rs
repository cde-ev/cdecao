type Mapping = ndarray::Array1<usize>;

// TODO get EdgeWeight type from somewhere
// TODO get Score type from somewhere
pub fn hungarian_algorithm(
    adjacency_matrix: &ndarray::Array2<u16>,
    dummy_x: &ndarray::Array1<bool>,
    mandatory_y: &ndarray::Array1<bool>,
    skip_x: &ndarray::Array1<bool>,
    skip_y: &ndarray::Array1<bool>,
) -> (Mapping, u32) {
    // TODO
    (ndarray::Array1::<usize>::zeros([0]), 0)
}
