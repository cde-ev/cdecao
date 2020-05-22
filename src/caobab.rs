//! A specialization of the generic branch and bound algorithm from `bab` for our specific problem.
//!
//! The module provides data types for subproblems and solutions, as well as the `run_bab_node()` function to solve a
//! subproblem of the course assignment prblem. All the data conversion from Course/Participant objects to matrices and
//! vectors for the `hungarian_algorithm()` happens within this function.

use crate::bab;
use crate::bab::NodeResult::{Feasible, Infeasible, NoSolution};
use crate::hungarian::{EdgeWeight, Score};
use crate::{Assignment, Course, Participant};
use log::{debug, info};
use std::sync::Arc;

/// Main method of the module to solve a course assignement problem using the branch and bound method together with the
/// hungarian method.
///
/// It takes a list of Courses, a list of Participants and a list of available rooms sizes to create
/// an optimal assignment of courses to participants.
pub fn solve(
    courses: Arc<Vec<Course>>,
    participants: Arc<Vec<Participant>>,
    rooms: Option<&Vec<usize>>,
    report_no_solution: bool,
) -> (Option<(Assignment, u32)>, bab::Statistics) {
    let pre_computed_problem = Arc::new(precompute_problem(&*courses, &*participants, rooms));

    bab::solve(
        move |sub_problem| -> bab::NodeResult<BABNode, Assignment, Score> {
            run_bab_node(
                &*courses,
                &*participants,
                &*pre_computed_problem,
                sub_problem,
                report_no_solution,
            )
        },
        BABNode {
            cancelled_courses: Vec::new(),
            enforced_courses: Vec::new(),
            shrinked_courses: Vec::new(),
            no_more_shrinking: Vec::new(),
        },
        num_cpus::get() as u32,
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
    /// Ordered list of rooms' sizes (descending), filled with zero entries to length of course list
    room_sizes: Option<Vec<usize>>,
}

/// Generate the general precomputed problem defintion (esp. the adjacency matrix) based on the Course and Participant
/// objects
fn precompute_problem(
    courses: &Vec<Course>,
    participants: &Vec<Participant>,
    rooms: Option<&Vec<usize>>,
) -> PreComputedProblem {
    // Calculate adjacency matrix size to allocate 1D-Arrays
    let m = courses.iter().map(|c| c.num_max).fold(0, |acc, x| acc + x);
    let max_num_instructors = courses
        .iter()
        .map(|c| c.instructors.len())
        .fold(0, |acc, x| acc + x);
    let n = m + max_num_instructors;

    // Generate course_map, inverse_course_map and madatory_y from course list
    let mut course_map = ndarray::Array1::<usize>::zeros([m]);
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
    let mut adjacency_matrix = ndarray::Array2::<EdgeWeight>::zeros([n, m]);
    for (x, p) in participants.iter().enumerate() {
        for (i, c) in p.choices.iter().enumerate() {
            // TODO check c < inverse_course_map.len() ?
            for j in 0..courses[*c].num_max {
                let y = inverse_course_map[*c] + j;
                adjacency_matrix[[x, y]] = edge_weight(i);
            }
        }
    }

    // Clone, fix and resize rooms Vec
    let room_sizes = rooms.map(|r| {
        let mut rooms = r.clone();
        rooms.sort();
        rooms.reverse();
        rooms.resize(courses.len(), 0);
        rooms
    });

    PreComputedProblem {
        adjacency_matrix,
        dummy_x,
        course_map,
        inverse_course_map,
        room_sizes: room_sizes,
    }
}

/// Parameter set for one subproblem of the Branch and Bound algorithm
#[derive(Clone, Debug)]
struct BABNode {
    /// Indexes of the cancelled courses in this node
    cancelled_courses: Vec<usize>,
    /// Indexes of the courses with enforced minimum participant number
    enforced_courses: Vec<usize>,
    /// Index and new max_num of courses (excl. instructors) that have been restricted due to room
    /// problems.
    ///
    /// A single course might be listed multiple times (to fix ordering of BABNodes), whereby in
    /// this case the lowest num_max bound must be applied. The max_num represents the maximum
    /// number of actual attendees to be assigned by the algorithm (without course instructors and
    /// room_offset etc.)
    shrinked_courses: Vec<(usize, usize)>,
    /// Courses that should not be shrinked any more. (to eliminate redundant branches)
    no_more_shrinking: Vec<usize>,
}

// As we want to do a pseudo depth-first search, BABNodes are ordered by their depth in the Branch and Bound tree for
// the prioritization by the parallel workers.
impl Ord for BABNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.cancelled_courses.len() + self.enforced_courses.len() + self.shrinked_courses.len())
            .cmp(
                &(other.cancelled_courses.len()
                    + other.enforced_courses.len()
                    + other.shrinked_courses.len()),
            )
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
        (self.cancelled_courses.len() + self.enforced_courses.len() + self.shrinked_courses.len())
            == (other.cancelled_courses.len()
                + other.enforced_courses.len()
                + other.shrinked_courses.len())
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
    report_no_solution: bool,
) -> bab::NodeResult<BABNode, Assignment, Score> {
    let n = pre_computed_problem.adjacency_matrix.dim().0;
    let m = pre_computed_problem.adjacency_matrix.dim().1;

    // We will modify the current_node later for creating a new subproblem. Until then, we want to use it readonly.
    let node = &current_node;
    debug!(
        "Solving subproblem with cancelled courses {:?} and enforced courses {:?} and shrinked courses {:?}",
        node.cancelled_courses, node.enforced_courses, node.shrinked_courses
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

    // Generate effective_num_max from cancelled courses and shrinked courses
    let mut effective_num_max: Vec<usize> = courses.iter().map(|c| c.num_max).collect();
    for c in node.cancelled_courses.iter() {
        effective_num_max[*c] = 0;
    }
    for (c, s) in node.shrinked_courses.iter() {
        effective_num_max[*c] = std::cmp::min(effective_num_max[*c], *s);
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
        debug!("Skipping this branch, since too many course places are enforced");
        return NoSolution;
    }
    if effective_num_max.iter().fold(0, |acc, x| acc + x) < participants.len() - num_skip_x {
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
            if report_no_solution {
                info!(
                    "Cannot cancel courses {:?}, since {:?}'s course choices cannot be fulfilled anymore.",
                    node.cancelled_courses.iter().map(|x| courses[*x].name.as_str()).collect::<Vec<&str>>(),
                    p.name,
                );
            }
            return NoSolution;
        }
    }

    // Generate skip_y from effective_num_max
    let mut skip_y = ndarray::Array1::from_elem([m], false);
    let mut num_skip_y: usize = 0;
    for (c, s) in effective_num_max.iter().enumerate() {
        let delta = courses[c].num_max - s;
        for j in 0..delta {
            let y = pre_computed_problem.inverse_course_map[c] + j;
            skip_y[y] = true;
        }
        num_skip_y += delta;
    }

    // Amend skip_x to skip x-dummies which are not needed (make matrix square-sized)
    for i in 0..(n - m + num_skip_y - num_skip_x) {
        skip_x[participants.len() + i] = true;
    }

    // Generate mandatory_y from enforced courses
    let mut mandatory_y = ndarray::Array1::from_elem([m], false);
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
        if !skip_y[cp] && *p < assignment.len() {
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

    // If room size list is given, check feasibility of solution w.r.t room sizes
    if let Some(ref room_sizes) = pre_computed_problem.room_sizes {
        let (feasible, restrictions) = check_room_feasibility(courses, &assignment, room_sizes);
        if !feasible {
            let mut branches = Vec::<BABNode>::new();
            if let Some(restrictions) = restrictions {
                // Add a new node for every possible constraint to fix room feasibility, as proposed
                // by check_room_feasibility()
                for (i, (c, action)) in restrictions.iter().enumerate() {
                    if !node.no_more_shrinking.contains(c) {
                        let mut new_node = current_node.clone();
                        match action {
                            RoomCourseFitAction::ShrinkCourse(s) => {
                                new_node.shrinked_courses.push((*c, *s));
                            }
                            RoomCourseFitAction::CancelCourse => {
                                if courses[*c].fixed_course {
                                    continue;
                                }
                                new_node.cancelled_courses.push(*c);
                            }
                        }
                        // Do not consider courses for future shrinking/cancelling, that have been
                        // considered in their own branch.
                        // With three rooms (15, 10, 5) and three courses (A, B, C), but only A and
                        // B being restrictable to 5 people, the tree might look like this:
                        //
                        //                                                       current node
                        //                                                /              |       \
                        //                                           /                   |          \
                        //                          shrink A to 10                shrink B to 10    shrink C to 10
                        //                         /             \                       |                |
                        //            shrink B to 10            shrink C to 10    shrink C to 10          ⚡
                        //             /         \                    |                  |
                        //    shrink A to 5    shrink B to 5    shrink A to 5     shrink B to 5
                        //
                        // As you can see, any additional node at one of the right branches, would
                        // introduce a redundant restriction with the branches left of it.
                        new_node
                            .no_more_shrinking
                            .extend(restrictions[..i].iter().map(|(c, _s)| c));
                        branches.push(new_node);
                    }
                }
            }
            return Infeasible(branches, score);
        }
    }

    // Check feasibility of the solution w.r.t course min size and participants, if not, get the most conflicting course
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

            // Return modified subproblem with course in cancelled courses (if cancelling the course is allowed)
            if !courses[c].fixed_course {
                current_node.cancelled_courses.push(c);
                branches.push(current_node);
            } else if report_no_solution {
                info!("Cannot cancel course {:?}, as it is fixed.", courses[c].name);
            }
        }

        return Infeasible(branches, score);
    }
}

/// Possible actions to take, if a course does not fit in the given rooms
enum RoomCourseFitAction {
    CancelCourse,
    ShrinkCourse(usize),
}

/// Check if the given matching is a feasible solution in terms of the Branch and Bound algorithm,
/// w.r.t. course rooms' sizes. If it is not feasible, a vector of possible max_size constraints
/// is returned.
///
/// # Arguments
///
/// * `courses` - List of courses
/// * `assignment` - The assignment to be checked (must include course instructors)
/// * `rooms` - An ordered list of course rooms in **descending** order, filled with zero entries to
///     length of course list
///
/// # Result
///
/// Returns a pair of `(is_feasible, restrictions)`.
///
/// `restrictions` is a Vec of possible size constraints to fix feasibility. Each entry has to be
/// interpreted as `(course_index, action)`, where `action` is either `CancelCourse` or
/// `ShrinkCourse(max_num)` with max_num not including course instructors and room_offset, i.e. it
/// can be directly used for `BABNode.shrinked_courses`.
/// The vector is ordered in ascending order by course sizes in the current assignment. This order
/// should be kept to solve more promising subproblems (with size restrictions on smaller courses)
/// first.
fn check_room_feasibility(
    courses: &Vec<Course>,
    assignment: &Assignment,
    rooms: &Vec<usize>,
) -> (bool, Option<Vec<(usize, RoomCourseFitAction)>>) {
    // Calculate course sizes (incl. instructors and room_offset)
    let mut course_size: Vec<(&Course, usize)> =
        courses.iter().map(|c| (c, c.room_offset)).collect();
    for c in assignment.iter() {
        course_size[*c].1 += 1;
    }
    course_size.sort_by_key(|(_c, s)| *s);

    // Find largest room type with non-fitting courses
    let conflicting_room = course_size
        .iter()
        .rev()
        .zip(rooms)
        .skip_while(|((_course, size), room_size)| size <= room_size)
        .next() // Iterator -> Option
        .map(|((_c, _s), r)| r);

    if let None = conflicting_room {
        // No conflict found -> assignment is feasible w.r.t. course rooms
        return (true, None);
    }

    // Build course constraint alternatives by finding all courses larger than the conflicting room,
    // beginning with the smallest
    let conflicting_room = conflicting_room.unwrap();
    return (
        false,
        Some(
            course_size
                .iter()
                .filter(|(_c, s)| s > conflicting_room)
                .map(|(c, _s)| {
                    (
                        c.index,
                        if *conflicting_room >= (c.num_min + c.room_offset + c.instructors.len()) {
                            RoomCourseFitAction::ShrinkCourse(
                                *conflicting_room - c.room_offset - c.instructors.len(),
                            )
                        } else {
                            RoomCourseFitAction::CancelCourse
                        },
                    )
                })
                .collect(),
        ),
    );
}

/// Check if the given matching is a feasible solution in terms of the Branch and Bound algorithm,
/// w.r.t. to minimum sizes of courses (and wrongly assinged participants). If not, find the most
/// conflicting course, to apply a constraint to it in the following nodes.
///
/// If any participant is in an unwanted course (wrong assignment), the solution is infeasible. We try to find an
/// other course with an instructor, who chose this course, to try to cancel that other course. This type of
/// infeasibility sets the second return flag. It may be, that no such course is found, which is signalled by
/// returning None. In this case, additional restrictions are pointless and we can abandon the node.
///
/// Additionally, the solution is infeasible, if any course has less participants than demanded. In this case we
/// return the course with the highest discrepancy to apply further restrictions on it.
///
/// # Result
///
/// The result is a triple: (is_feasible, has_participant_problem, course_index_to_restrict)
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
                .filter(|rc| !node.cancelled_courses.contains(rc))
                .filter(|rc| !node.enforced_courses.contains(rc))
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

#[cfg(test)]
mod tests;
