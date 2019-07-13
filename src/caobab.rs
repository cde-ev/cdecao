use super::{Course, Mapping, Participant};
use std::sync::Arc;

/// Parameter set for one subproblem of the Branch and Bound algorithm
struct BABNode {
    /// Indexes of the cancelled courses in this node
    cancelled_courses: Vec<u32>,
    /// Indexes of the courses with enforced minimum participant number
    enforced_courses: Vec<u32>,
}

// As we want to do a pseudo depth-first search, BABNodes are ordered by their depth in the Branch and Bound tree for
// the prioritization by the parallel workers.
impl Ord for BABNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.cancelled_courses.len() + self.enforced_courses.len())
            .cmp(&(other.cancelled_courses.len() + other.enforced_courses.len()))
    }
}

impl PartialOrd for BABNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for BABNode {}

impl PartialEq for BABNode {
    fn eq(&self, other: &Self) -> bool {
        (self.cancelled_courses.len() + self.enforced_courses.len())
            == (other.cancelled_courses.len() + other.enforced_courses.len())
    }
}

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
type EdgeWeight = u16;

/// Precomputed problem definition for the hungarian method, that can be reused for every Branch and Bound node
struct PreComputedProblem {
    adjacency_matrix: ndarray::Array2<EdgeWeight>,
    mandatory_y: ndarray::Array1<bool>,
    dummy_x: ndarray::Array1<bool>, // TODO mapping between entrys and courses
}

/// Generate the general precomputed problem defintion (esp. the adjacency matrix) based on the Course and Participant
/// objects
fn build_pre_computed_problem(
    courses: &Vec<Course>,
    participants: &Vec<Participant>,
) -> PreComputedProblem {
    // TODO
    PreComputedProblem {
        adjacency_matrix: ndarray::Array2::default([0, 0]),
        mandatory_y: ndarray::Array1::default([0]),
        dummy_x: ndarray::Array1::default([0]),
    }
}

/// Solver for a single branch and bound node/subproblem. It takes the precomputed problem description and the
/// additional restrictions for the specific node and solves the resulting matching subproblem using the hungarian
/// method.
fn run_bab_node(
    courses: &Vec<Course>,
    participants: &Vec<Participant>,
    pre_computed_problem: &PreComputedProblem,
    node: &BABNode,
) -> super::bab::NodeResult<BABNode, Mapping> {
    // TODO
    super::bab::NodeResult::NoSolution
}

/// Main method of the module to solve a course assignement problem using the branch and bound method together with the
/// hungarian method.
///
/// It takes a list of Courses and a list of Participants to create an optimal mapping of courses to participants.
pub fn solve(
    courses: Arc<Vec<Course>>,
    participants: Arc<Vec<Participant>>,
) -> Option<(Mapping, u32)> {
    let pre_computed_problem = Arc::new(build_pre_computed_problem(&*courses, &*participants));

    super::bab::solve(
        move |sub_problem| -> super::bab::NodeResult<BABNode, Mapping> {
            run_bab_node(
                &*courses,
                &*participants,
                &*pre_computed_problem,
                sub_problem,
            )
        },
        BABNode {
            cancelled_courses: Vec::new(),
            enforced_courses: Vec::new(),
        },
        4, // TODO guess number of threads
    )
}
