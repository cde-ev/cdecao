use super::{Course, Mapping, Participant};
use std::sync::Arc;

/// Parameter set for one subproblem of the Branch and Bound algorithm
struct BABNode {
    /// Indexes of the cancelled courses in this node
    cancelled_courses: Vec<usize>,
    /// Indexes of the courses with enforced minimum participant number
    enforced_courses: Vec<usize>,
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

/// Highest value for edge weights to be used. See docs of `EdgeWeight` for more thoughts on that topic
const WEIGHT_OFFSET: EdgeWeight = 50000;
/// Generate edge weight from course choice
fn edge_weight(choice_index: usize) -> EdgeWeight {
    WEIGHT_OFFSET - (choice_index as EdgeWeight)
}

/// Precomputed problem definition for the hungarian method, that can be reused for every Branch and Bound node
struct PreComputedProblem {
    /// Adjacency matrix generated from course choices. Each row represents one participant (or dummy participant),
    /// each column represents one place in a course.
    adjacency_matrix: ndarray::Array2<EdgeWeight>,
    /// Marks all the rows in the adjacency matrix that do not represent an actual participant (may not be used to
    /// fill mandatory course places)
    dummy_x: ndarray::Array1<bool>,
    /// Marks all the columns in the adjacency matrix that represet a mandatory course place (may not be matched with a
    /// dummy participant)
    course_map: ndarray::Array1<usize>,
    /// maps Course index to the first column index of its first course places
    inverse_course_map: Vec<usize>,
}

/// Generate the general precomputed problem defintion (esp. the adjacency matrix) based on the Course and Participant
/// objects
fn build_pre_computed_problem(
    courses: &Vec<Course>,
    participants: &Vec<Participant>,
) -> PreComputedProblem {
    // Calculate adjacency matrix size to allocate 1D-Arrays
    let n = courses.iter().map(|c| c.num_max).fold(0, |acc, x| acc + x);

    // Generate course_map, inverse_course_map and madatory_y from course list
    let mut course_map = ndarray::Array1::<usize>::zeros([n]);
    let mut inverse_course_map = Vec::<usize>::new();
    let mut k = 0;
    for (i, c) in courses.iter().enumerate() {
        for j in 0..c.num_max {
            course_map[k + j] = i;
        }
        inverse_course_map.push(k);
        k += c.num_max;
    }

    // Generate dummy_x
    let mut dummy_x = ndarray::Array1::from_elem([n], false);
    for i in participants.len()..n {
        dummy_x[i] = true;
    }

    // Generate adjacency matrix
    let mut adjacency_matrix = ndarray::Array2::<EdgeWeight>::zeros([n, n]);
    for (x, p) in participants.iter().enumerate() {
        for (i, c) in p.choices.iter().enumerate() {
            // TODO check c < inverse_course_map.len() ?
            for j in 0..courses[*c].num_max {
                let y = inverse_course_map[*c] + j;
                adjacency_matrix[[x, y]] = edge_weight(i);
            }
        }
    }

    PreComputedProblem {
        adjacency_matrix,
        dummy_x,
        course_map,
        inverse_course_map,
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
    let n = pre_computed_problem.adjacency_matrix.dim().0;

    // Check for general feasibility
    if node
        .enforced_courses
        .iter()
        .map(|c| courses[*c].num_min)
        .fold(0, |acc, x| acc + x)
        > participants.len()
    {
        print!("Skipping this branch, since too much course places are enforced");
        return super::bab::NodeResult::NoSolution;
    }
    if (n - node
        .cancelled_courses
        .iter()
        .map(|c| courses[*c].num_max)
        .fold(0, |acc, x| acc + x))
        < participants.len()
    {
        print!("Skipping this branch, since not enough course places are left");
        return super::bab::NodeResult::NoSolution;
    }
    for p in participants {
        if p.choices.iter().all(
            |x| match node.cancelled_courses.iter().position(|y| y == x) {
                None => false,
                Some(_) => true,
            },
        ) {
            print!("Skipping this branch, since not all course choices can be fulfilled");
            return super::bab::NodeResult::NoSolution;
        }
    }

    // Generate skip_x from course instructors of non-cancelled courses
    let mut skip_x = ndarray::Array1::from_elem([n], false);
    for (i, c) in courses.iter().enumerate() {
        if let None = node.cancelled_courses.iter().position(|x| *x == i) {
            for instr in c.instructors.iter() {
                skip_x[*instr] = true;
            }
        }
    }

    // Generate skip_y from cancelled courses
    let mut skip_y = ndarray::Array1::from_elem([n], false);
    for c in node.cancelled_courses.iter() {
        for j in 0..courses[*c].num_max {
            let y = pre_computed_problem.inverse_course_map[*c] + j;
            skip_y[y] = true;
        }
    }

    // Generate mandatory_y from enforced courses
    let mut mandatory_y = ndarray::Array1::from_elem([n], false);
    for c in node.enforced_courses.iter() {
        for j in 0..courses[*c].num_min {
            let y = pre_computed_problem.inverse_course_map[*c] + j;
            mandatory_y[y] = true;
        }
    }

    // TODO run hungarian method

    // TODO check feasibility of the solution for the Branch and Bound algorithm and, if not, the most conflicting course
    // TODO add course instructors to matching and score
        // TODO return Feasible
    // TODO if not feasible but found a conflicting course, generate new sub-branches, based on conflicting course
        // TODO return IsFeasible

    return super::bab::NodeResult::NoSolution;
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
