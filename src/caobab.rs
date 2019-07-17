//! A specialization of the generic branch and bound algorithm from `bab` for our specific problem.
//!
//! The module provides data types for subproblems and solutions, as well as the `run_bab_node()` function to solve a
//! subproblem of the course assignment prblem. All the data conversion from Course/Participant objects to matrices and
//! vectors for the `hungarian_algorithm()` happens within this function.

use super::bab::NodeResult::{Feasible, Infeasible, NoSolution};
use super::hungarian::{EdgeWeight, Score};
use super::{Assignment, Course, Participant};
use log::debug;
use std::sync::Arc;

/// Main method of the module to solve a course assignement problem using the branch and bound method together with the
/// hungarian method.
///
/// It takes a list of Courses and a list of Participants to create an optimal assignment of courses to participants.
pub fn solve(
    courses: Arc<Vec<Course>>,
    participants: Arc<Vec<Participant>>,
) -> Option<(Assignment, u32)> {
    let pre_computed_problem = Arc::new(precompute_problem(&*courses, &*participants));

    super::bab::solve(
        move |sub_problem| -> super::bab::NodeResult<BABNode, Assignment, Score> {
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

/// Highest value for edge weights to be used. See docs of `super::hungarian::EdgeWeight` for more thoughts on that
/// topic
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
    /// Maps each column in the adjacency matrix to the course's index, the represented course place is belonging to
    course_map: ndarray::Array1<usize>,
    /// maps Course index to the first column index of its first course places
    inverse_course_map: Vec<usize>,
}

/// Generate the general precomputed problem defintion (esp. the adjacency matrix) based on the Course and Participant
/// objects
fn precompute_problem(
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

/// Parameter set for one subproblem of the Branch and Bound algorithm
#[derive(Clone)]
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

/// Solver for a single branch and bound node/subproblem. It takes the precomputed problem description and the
/// additional restrictions for the specific node and solves the resulting matching subproblem using the hungarian
/// method.
///
/// To do so, we first need to calculate some vectors specific for this subproblem (mandatory course places (from
/// enforced courses), skipped participants (from course instructors), skipped courses). Afterwards we can use the
/// `hungarian::hungarian_algorithm()` function to solve the optimization problem. Then, we need to transform the
/// matching of participants with course places into an assignment of participants to courses and check the feasibility
/// of the solution for our overall problem.
fn run_bab_node(
    courses: &Vec<Course>,
    participants: &Vec<Participant>,
    pre_computed_problem: &PreComputedProblem,
    mut current_node: BABNode,
) -> super::bab::NodeResult<BABNode, Assignment, Score> {
    let n = pre_computed_problem.adjacency_matrix.dim().0;

    // We will modify the current_node later for creating a new subproblem. Until then, we want to use it readonly.
    let node = &current_node;
    debug!(
        "Solving subproblem with cancelled courses {:?} and enforced courses {:?}",
        node.cancelled_courses, node.enforced_courses
    );

    // Generate skip_x from course instructors of non-cancelled courses
    let mut skip_x = ndarray::Array1::from_elem([n], false);
    let mut num_skip_x: usize = 0;
    for (i, c) in courses.iter().enumerate() {
        if !node.cancelled_courses.contains(&i) {
            for instr in c.instructors.iter() {
                skip_x[*instr] = true;
                num_skip_x += 1;
            }
        }
    }

    // Check for general feasibility
    // (this is done after calculating the course instructors/skip_x, as we need their number here)
    if node
        .enforced_courses
        .iter()
        .map(|c| courses[*c].num_min)
        .fold(0, |acc, x| acc + x)
        > participants.len() - num_skip_x
    {
        debug!("Skipping this branch, since too much course places are enforced");
        return NoSolution;
    }
    if (n - node
        .cancelled_courses
        .iter()
        .map(|c| courses[*c].num_max)
        .fold(0, |acc, x| acc + x))
        < participants.len() - num_skip_x
    {
        debug!("Skipping this branch, since not enough course places are left");
        return NoSolution;
    }
    for (x, p) in participants.iter().enumerate() {
        if !skip_x[x]
            && p.choices
                .iter()
                .all(|c| node.cancelled_courses.contains(&c))
        {
            debug!("Skipping this branch, since not all course choices can be fulfilled");
            return NoSolution;
        }
    }

    // Generate skip_y from cancelled courses
    let mut skip_y = ndarray::Array1::from_elem([n], false);
    let mut num_skip_y: usize = 0;
    for c in node.cancelled_courses.iter() {
        for j in 0..courses[*c].num_max {
            let y = pre_computed_problem.inverse_course_map[*c] + j;
            skip_y[y] = true;
        }
        num_skip_y += courses[*c].num_max;
    }

    // Amend skip_x to skip x-dummies for cancelled course places
    for i in 0..num_skip_y {
        skip_x[participants.len() + i] = true;
    }

    // Generate mandatory_y from enforced courses
    let mut mandatory_y = ndarray::Array1::from_elem([n], false);
    for c in node.enforced_courses.iter() {
        for j in 0..courses[*c].num_min {
            let y = pre_computed_problem.inverse_course_map[*c] + j;
            mandatory_y[y] = true;
        }
    }

    // Run hungarian method
    let (matching, mut score) = super::hungarian::hungarian_algorithm(
        &pre_computed_problem.adjacency_matrix,
        &pre_computed_problem.dummy_x,
        &mandatory_y,
        &skip_x,
        &skip_y,
    );

    // Convert course place matching to course assignment
    let mut assignment: Assignment = vec![0usize; participants.len()];
    for (cp, p) in matching.iter().enumerate() {
        if *p < assignment.len() {
            assignment[*p] = pre_computed_problem.course_map[cp];
        }
    }
    // Add instructors to matching and increase score w.r.t. instructors
    for (c, course) in courses.iter().enumerate() {
        if !node.cancelled_courses.contains(&c) {
            for instr in course.instructors.iter() {
                assignment[*instr] = c;
            }
            score += (course.instructors.len() as u32) * (WEIGHT_OFFSET as u32);
        }
    }

    // Check feasibility of the solution for the Branch and Bound algorithm and, if not, get the most conflicting course
    let (feasible, participant_problem, branch_course) =
        check_feasibility(courses, participants, &assignment, &node, &skip_x);
    if feasible {
        debug!("Yes! We found a feasible solution with score {}.", score);
        return Feasible(assignment, score);
    } else {
        let mut branches = Vec::<BABNode>::new();
        if let Some(c) = branch_course {
            // If we didn't fail at an unresolvable wrong assignment error, create new subproblem with course enforced
            if !participant_problem {
                let mut new_node = current_node.clone();
                new_node.enforced_courses.push(c);
                branches.push(new_node);
            }

            // Return modified subproblem with course in cancelled courses
            current_node.cancelled_courses.push(c);
            branches.push(current_node);
        }

        return Infeasible(branches, score);
    }
}

/// Check if the given matching is a feasible solution in terms of the Branch and Bound algorithm. If not, find the
/// most conflicting course, to apply a constraint to it in the following nodes.
///
/// If any participant is in an unwanted course (wrong assignment), the solution is infeasible. We try to find a
/// course with an instructor, who chose this course, to try to cancel that course. This type of infeasibility
/// sets the second return flag. It may be, that no such course is found, which is signalled by returning None. In
/// this case, additional restrictions are pointless and we can abandon the node.
///
/// Additionally, the solution is infeasible, if any course has less participants than demanded. In this case we
/// return the course with the highest discrepancy to apply further restrictions on it.
fn check_feasibility(
    courses: &Vec<Course>,
    participants: &Vec<Participant>,
    assignment: &Assignment,
    node: &BABNode,
    is_instructor: &ndarray::Array1<bool>,
) -> (bool, bool, Option<usize>) {
    // Calculate course sizes
    let mut course_size = vec![0usize; courses.len()];
    for (p, c) in assignment.iter().enumerate() {
        if !is_instructor[p] {
            course_size[*c] += 1;
        }
    }

    // Check if solution is infeasible, such that any participant is in an un-chosen course
    for (p, c) in assignment.iter().enumerate() {
        if !is_instructor[p] && !participants[p].choices.contains(c) {
            // If so, get smallest non-constrained course, that has an instructor, who chose c
            let mut relevant_courses: Vec<usize> = (0..courses.len())
                .filter(|rc| node.cancelled_courses.contains(rc))
                .filter(|rc| node.enforced_courses.contains(rc))
                .filter(|rc| {
                    courses[*rc]
                        .instructors
                        .iter()
                        .any(|instr| participants[*instr].choices.contains(c))
                })
                .collect();
            if relevant_courses.len() == 0 {
                return (false, true, None);
            } else {
                relevant_courses.sort_by_key(|rc| course_size[*rc]);
                return (false, true, Some(relevant_courses[0]));
            }
        }
    }

    // Check if solution is feasible, such that any course has its minimum participant number violated
    let mut max_score = 0;
    let mut course: Option<usize> = None;
    for (c, size) in course_size.iter().enumerate() {
        if !node.cancelled_courses.contains(&c) && !node.enforced_courses.contains(&c) {
            if *size < courses[c].num_min {
                let score = courses[c].num_min - size;
                if score > max_score {
                    max_score = score;
                    course = Some(c);
                }
            }
        }
    }
    return (course == None, false, course);
}
