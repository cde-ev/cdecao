// Copyright 2020 by Michael Thies <mail@mhthies.de>, Gabriel Guckenbiehl <gabriel.guckenbiehl@gmx.de>
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

//! A specialization of the generic branch and bound algorithm from `bab` for our specific problem.
//!
//! The module provides data types for subproblems and solutions, as well as the `run_bab_node()` function to solve a
//! subproblem of the course assignment prblem. All the data conversion from Course/Participant objects to matrices and
//! vectors for the `hungarian_algorithm()` happens within this function.

use crate::bab::NodeResult::{Feasible, Infeasible, NoSolution};
use crate::hungarian::{EdgeWeight, Score};
use crate::util::{binom, IterSelections};
use crate::{bab, Choice};
use crate::{Assignment, Course, Participant};
use log::{debug, info};
use std::cmp::min;
use std::fmt::Debug;
use std::sync::Arc;

pub mod solution_score;

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
    num_threads: u32,
) -> (Option<(Assignment, u32)>, bab::Statistics) {
    let pre_computed_problem = Arc::new(precompute_problem(&courses, &participants, rooms));

    bab::solve(
        move |sub_problem| -> bab::NodeResult<BABNode, Assignment, Score> {
            run_bab_node(
                &courses,
                &participants,
                &pre_computed_problem,
                sub_problem,
                report_no_solution,
            )
        },
        BABNode {
            cancelled_courses: Vec::new(),
            enforced_courses: Vec::new(),
            shrinked_courses: Vec::new(),
        },
        num_threads,
    )
}

/// Highest value for edge weights to be used. See docs of `super::hungarian::EdgeWeight` for more thoughts on that
/// topic
const WEIGHT_OFFSET: EdgeWeight = 50000;
/// Generate edge weight from course choice
fn edge_weight(choice: &Choice) -> EdgeWeight {
    WEIGHT_OFFSET - choice.penalty as EdgeWeight
}
const INSTRUCTOR_SCORE: Score = WEIGHT_OFFSET as u32;

/// Precomputed problem definition for the hungarian method, that can be reused for every Branch and Bound node
struct PreComputedProblem {
    /// Adjacency matrix generated from course choices. Each row represents one participant (or dummy participant),
    /// each column represents one place in a course.
    adjacency_matrix: ndarray::Array2<EdgeWeight>,
    /// Marks all the rows in the adjacency matrix that do not represent an actual participant (may not be used to
    /// fill mandatory course places)
    dummy_x: ndarray::Array1<bool>,
    /// Marks rows in the adjacency matrix that shall always be ignored for assignment (because these participants
    /// shall/can not be assigned as course attendees).
    ///
    /// This vector is dynamically extended by the list of course instructors in each BaB node to get the complete
    /// skip_x vector. In theory, we could remove the rows with skip_x_always[x]==true completely from the matrix, but
    /// that would require changes to the handling of the participant list to keep the indexes in sync.
    skip_x_always: ndarray::Array1<bool>,
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
    // To determine the required number of extra participant rows (which are filled with dummy
    // participants later), we need to know the maximum number of participants that may be skipped
    // in any assignment. This includes all course instructors and all participants without choices.
    let mut skippable_participants: Vec<bool> = participants
        .iter()
        .map(|p| p.is_instructor_only())
        .collect();
    for course in courses.iter() {
        for instructor in course.instructors.iter() {
            skippable_participants[*instructor] = true;
        }
    }
    let max_num_skipped_x = skippable_participants.iter().filter(|x| **x).count();
    // Calculate adjacency matrix size to allocate 1D-Arrays
    let m = courses.iter().map(|c| c.num_max).sum();
    let n = m + max_num_skipped_x;

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

    // Generate skip_x_always: Skip instructor-only participants, don't skip dummy_x rows
    let skip_x_always: ndarray::Array1<bool> = participants
        .iter()
        .map(|p| p.is_instructor_only())
        .chain(std::iter::repeat(false))
        .take(n)
        .collect();

    // Generate adjacency matrix
    let mut adjacency_matrix = ndarray::Array2::<EdgeWeight>::zeros([n, m]);
    for (x, p) in participants.iter().enumerate() {
        for choice in p.choices.iter() {
            debug_assert!(
                choice.course_index < inverse_course_map.len(),
                "Invalid course choice index {} of participant {}",
                choice.course_index,
                p.index
            );
            for j in 0..courses[choice.course_index].num_max {
                let y = inverse_course_map[choice.course_index] + j;
                adjacency_matrix[[x, y]] = edge_weight(choice);
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
        skip_x_always,
        course_map,
        inverse_course_map,
        room_sizes,
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
    courses: &[Course],
    participants: &[Participant],
    pre_computed_problem: &PreComputedProblem,
    mut current_node: BABNode,
    report_no_solution: bool,
) -> bab::NodeResult<BABNode, Assignment, Score> {
    let n = pre_computed_problem.adjacency_matrix.dim().0;
    let m = pre_computed_problem.adjacency_matrix.dim().1;

    // We will modify the current_node later for creating a new subproblem. Until then, we want to use it readonly.
    let node = &current_node;

    // Generate skip_x from course instructors of non-cancelled courses
    let mut skip_x = pre_computed_problem.skip_x_always.clone();
    for (i, c) in courses.iter().enumerate() {
        if !node.cancelled_courses.contains(&i) {
            for instr in c.instructors.iter() {
                skip_x[*instr] = true;
            }
        }
    }
    let num_skip_x = skip_x.iter().filter(|x| **x).count();

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
        .sum::<usize>()
        > participants.len() - num_skip_x
    {
        debug!("Skipping this branch, since too many course places are enforced");
        return NoSolution;
    }
    if effective_num_max.iter().sum::<usize>() < participants.len() - num_skip_x {
        debug!("Skipping this branch, since not enough course places are left");
        return NoSolution;
    }
    for (x, p) in participants.iter().enumerate() {
        if !skip_x[x]
            && p.choices
                .iter()
                .all(|c| node.cancelled_courses.contains(&c.course_index))
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
            let y = pre_computed_problem.inverse_course_map[c] + courses[c].num_max - 1 - j;
            skip_y[y] = true;
        }
        num_skip_y += delta;
    }

    // Amend skip_x to skip x-dummies which are not needed (make matrix square-sized)
    debug_assert!(
        n - num_skip_x >= m - num_skip_y,
        "effective participants + dummy participants ({} - {}) should be <= effective course places ({} - {})",
        n,
        num_skip_x,
        m,
        num_skip_y);
    for i in 0..(n - m + num_skip_y - num_skip_x) {
        skip_x[participants.len() + i] = true;
    }

    // Generate mandatory_y from enforced courses
    let mut mandatory_y = ndarray::Array1::from_elem([m], false);
    for c in node.enforced_courses.iter() {
        for j in 0..courses[*c].num_min {
            let y = pre_computed_problem.inverse_course_map[*c] + j;
            mandatory_y[y] = true;
            assert!(
                !skip_y[y],
                "Trying to make the {}th course place of course {} mandatory \
            although it is already skipped.",
                j, c
            );
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
    let mut assignment: Assignment = vec![None; participants.len()];
    for (cp, p) in matching.iter().enumerate() {
        if !skip_y[cp] && *p < assignment.len() {
            assignment[*p] = Some(pre_computed_problem.course_map[cp]);
        }
    }
    // Add instructors to matching and increase score w.r.t. instructors
    for (c, course) in courses.iter().enumerate() {
        if !node.cancelled_courses.contains(&c) {
            for instr in course.instructors.iter() {
                assignment[*instr] = Some(c);
                // Don't consider instructor_only participants in the score. Otherwise they will
                // have such a large influence that they effectively soft-enforce their course to
                // take place.
                if !participants[*instr].is_instructor_only() {
                    score += INSTRUCTOR_SCORE;
                }
            }
        }
    }

    // If room size list is given, check feasibility of solution w.r.t room sizes
    if let Some(ref room_sizes) = pre_computed_problem.room_sizes {
        let (feasible, restrictions) =
            check_room_feasibility(courses, &assignment, room_sizes, &current_node);
        if !feasible {
            let mut branches = Vec::<BABNode>::new();
            if let Some(restrictions) = restrictions {
                // Add a new node for every possible constraint to fix room feasibility, as proposed
                // by check_room_feasibility()
                for mut restriction in restrictions {
                    let mut new_node = current_node.clone();
                    new_node
                        .shrinked_courses
                        .append(&mut restriction.shrink_courses);
                    new_node
                        .cancelled_courses
                        .append(&mut restriction.cancel_courses);
                    branches.push(new_node);
                }
            }
            return Infeasible(branches, score);
        }
    }

    // Check feasibility of the solution w.r.t course min size and participants, if not, get the most conflicting course
    let (feasible, participant_problem, branch_course) =
        check_feasibility(courses, participants, &assignment, node, &skip_x);
    if !feasible {
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
                info!(
                    "Cannot cancel course {:?}, as it is fixed.",
                    courses[c].name
                );
            }
        }

        return Infeasible(branches, score);
    }

    Feasible(assignment, score)
}

/// A set of constraints to fix a specific room size violation.
///
/// All the constraints (shrinked courses, cancelled courses) in this set meant to be applied
/// together, in addition to the constraints already present in the current BaB node.
///
/// [check_room_feasibility] will create multiple alternative of these constraint sets allow
/// trying different solutions for the room size violation in different following BaB nodes.
struct RoomConstraintSet {
    /// A Vec of course-shrink constraints ((course_index, new_size)) to be appended to
    /// [BABNode::shrinked_courses]
    shrink_courses: Vec<(usize, usize)>,
    /// A Vec of courses to be cancelled, to be appended to [BABNode::cancelled_courses].
    cancel_courses: Vec<usize>,
}

/// Check if the given matching is a feasible solution in terms of the Branch and Bound algorithm,
/// w.r.t. course rooms' sizes. If it is not feasible, a vector of possible constraint sets
/// is returned.
///
/// To check feasibility, we sort all rooms and courses by size in a descending order and assign
/// the courses to the rooms at the same list position. If any course is larger than the assigned
/// room, the solution is infeasible. For generating constraints, we consider the largest of these
/// conflicting courses.
///
/// To generate the possible constraints sets, we use some heuristics: In theory, we would have to
/// check all variants from restricting all possible selections of courses to the size of the
/// conflicting room. This is not possible due to combinatorial complexity. Thus, we only consider a
/// range of courses of similar size to the conflicting course and take all possible selections of
/// this range. All courses with size below that range are restricted to the conflicting room's size
/// in any case; all courses larger than that range are not considered for restriction.
///
/// To chose the range, we first check how many courses have to be shrinked to the conflicting
/// room's size (i.e. courses that are assigned to a room of that size but not larger). If this
/// number is smaller than `min_k`, we add the next few smaller courses to reach `MIN_K`. The size
/// of this set determines the size `k` of the selections (i.e. how many courses are shrinked in
/// each generated new subproblem).
///
/// If this set is smaller than `MAX_N`, we add up to `MAX_NTOK` larger courses (but limited to
/// `MAX_N`) to form the total set of courses to consider for shrinking. This means, if the number
/// of conflicting courses for the given room size is already larger than `MAX_N`, n equals k, so
/// only one constraint set will be generated. Otherwise, we can generate up to
/// (MAX_N choose (MAX_N-MAX_NTOK)) constraints sets.
///
/// # Arguments
///
/// * `courses` - The list of all courses (as referenced by `node` and `assignment`)
/// * `assignment` - The assignment to be checked (must include course instructors)
/// * `rooms` - An ordered list of course rooms in **descending** order, filled with zero entries to
///     length of course list
/// * `node` – The current BaB node, used to avoid conflicting restrictions (cancelled vs. enforced)
///            and redundant restrictions.
///
/// # Result
///
/// Returns a tuple `(feasible, constraint_sets)`. `constraint_sets` is either None or a vector of
/// possible constraint sets.
fn check_room_feasibility(
    courses: &[Course],
    assignment: &Assignment,
    rooms: &Vec<usize>,
    node: &BABNode,
) -> (bool, Option<Vec<RoomConstraintSet>>) {
    // Calculate course sizes (incl. instructors and room_offset)
    let mut course_size: Vec<(&Course, usize)> = courses.iter().map(|c| (c, 0)).collect();
    for course in assignment.iter() {
        if let Some(c) = course {
            course_size[*c].1 += 1;
        }
    }
    for (c, ref mut s) in course_size.iter_mut() {
        *s = if *s == 0 {
            0
        } else {
            (c.room_offset + c.room_factor * (*s as f32)).ceil() as usize
        };
    }

    // Note: The courses are ordered by (effective) size in ascending order.
    // Only for finding the largest conflicting course, we reverse the iteration order.
    course_size.sort_by_key(|(_c, s)| *s);

    // Find index largest room type with non-fitting courses
    let conflicting_course_index = course_size
        .iter()
        .enumerate()
        .rev()
        .zip(rooms)
        .find(|((_i, (_course, size)), room_size)| size > room_size)
        .map(|((i, (_c, _s)), _r)| i);

    if conflicting_course_index.is_none() {
        // No conflict found -> assignment is feasible w.r.t. course rooms
        return (true, None);
    }

    // Calculate range of courses to generate selections for shrinking from
    const MIN_K: usize = 5;
    const MAX_NTOK: usize = 5;
    const MAX_N: usize = 17;
    // Maximum (n chose k): 17!/12!/5! = 6188

    let conflicting_course_index: usize = conflicting_course_index.unwrap();
    let conflicting_room_size = rooms[rooms.len() - 1 - conflicting_course_index];
    // index of the smallest course, which is too large for `conflicting_room_size`
    let smallest_conflicting_course_index = course_size
        .iter()
        .position(|(_course, size)| *size > conflicting_room_size)
        .unwrap();
    assert!(conflicting_course_index >= smallest_conflicting_course_index);
    let mut k = conflicting_course_index - smallest_conflicting_course_index + 1;
    let lower_bound;
    if k < MIN_K {
        if conflicting_course_index + 1 < MIN_K {
            lower_bound = 0;
            k = conflicting_course_index + 1;
        } else {
            lower_bound = conflicting_course_index + 1 - MIN_K;
            k = MIN_K;
        }
    } else {
        lower_bound = smallest_conflicting_course_index;
    }

    // exclusive upper bound of the "n" range to consider for selections
    let mut upper_bound = conflicting_course_index + 1;
    if upper_bound - lower_bound < MAX_N {
        upper_bound = min(
            min(conflicting_course_index + MAX_NTOK, lower_bound + MAX_N),
            course_size.len(),
        );
    }

    // Generate possible constraint sets from combinatorial selections from the calculated range.
    debug!(
        "Creating room constraint sets from all k-selections from course_size[{}..{}] (effective course size {}-{}) \
        with k={} for room of size {}",
        lower_bound, upper_bound, course_size[lower_bound].1, course_size[upper_bound-1].1, k, conflicting_room_size
    );
    let always_constraints = create_room_constraint_set(
        node,
        course_size
            .iter()
            .filter(|(_c, s)| *s <= conflicting_room_size)
            .map(|(c, _s)| *c),
        conflicting_room_size,
        false,
    )
    .unwrap(); // This cannot fail, because `all_required` is not set
    let mut result = Vec::with_capacity(binom(upper_bound - lower_bound, k));
    for selection in course_size[lower_bound..upper_bound].iter_selections(k) {
        let constraint_set = create_room_constraint_set(
            node,
            selection.iter().map(|(c, _s)| *c),
            conflicting_room_size,
            true,
        );
        // Only consider results where all courses from the selection can be cancelled/shrinked
        if let Some(mut constraint_set) = constraint_set {
            constraint_set
                .shrink_courses
                .extend_from_slice(&always_constraints.shrink_courses[..]);
            constraint_set
                .cancel_courses
                .extend_from_slice(&always_constraints.cancel_courses[..]);
            // We should always generate new constraints, when all courses from the k-selection can
            // be cancelled/shrinked
            assert!(
                !(constraint_set.shrink_courses.is_empty()
                    && constraint_set.cancel_courses.is_empty())
            );
            result.push(constraint_set);
        }
    }

    debug!("Actually created {} room constraint sets", result.len());
    (false, Some(result))
}

/// Helper function of [check_room_feasibility] for generating a valid constraints set which shrinks
/// a selected list courses to the required size.
///
/// This function takes care of
/// - taking course instructors into account for room size
/// - applying respective [Course::room_offset] and [Course::room_factor] of each course
/// - Creating shrink-constraint OR cancel-constraint, depending on the minimum size of the course
/// - ignoring sink-constraints when courses are already shrinked further
/// - ignoring cancel-constraints when courses are enforced or fixed
///
/// # Arguments
///
/// * `current_node` - The current branch-and-bound node, i.e. constraint set, to check for enforced
///     and already shrinked courses
/// * `courses` - The courses to create room constraints for
/// * `to_size` - The room size to shrink the `courses` to
/// * `all_required` – If true, a (new) constraint is expected to be applied to all given `courses`.
///     If this cannot be fulfilled due to existing constraints, the function returns `None`.
///
/// # Result
///
/// Returns
/// * `None` if `all_required`, but some constraint would have been ignored
/// * A [RoomConstraintSet] otherwise
fn create_room_constraint_set<'a>(
    current_node: &BABNode,
    courses: impl IntoIterator<Item = &'a Course>,
    to_size: usize,
    all_required: bool,
) -> Option<RoomConstraintSet> {
    let mut cancel = Vec::new();
    let mut shrink = Vec::new();
    for course in courses {
        // Don't consider courses that are already cancelled in the current node
        if current_node.cancelled_courses.contains(&course.index) {
            if all_required {
                return None;
            } else {
                continue;
            }
        }
        if to_size
            >= ((course.room_offset
                + course.room_factor * (course.num_min + course.instructors.len()) as f32)
                .ceil() as usize)
        {
            let shrink_size = (((to_size as f32) - course.room_offset) / course.room_factor).floor()
                as usize
                - course.instructors.len();
            // Don't shrink courses that are already shrinked further in the current node
            if current_node
                .shrinked_courses
                .iter()
                .any(|(c, s)| *c == course.index && *s <= shrink_size)
            {
                if all_required {
                    return None;
                } else {
                    continue;
                }
            }
            shrink.push((course.index, shrink_size));
        } else {
            // Don't cancel courses that are fixed or enforced in the current node
            if current_node.enforced_courses.contains(&course.index) || course.fixed_course {
                if all_required {
                    return None;
                } else {
                    continue;
                }
            }
            cancel.push(course.index);
        }
    }
    Some(RoomConstraintSet {
        shrink_courses: shrink,
        cancel_courses: cancel,
    })
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
    courses: &[Course],
    participants: &[Participant],
    assignment: &Assignment,
    node: &BABNode,
    is_instructor: &ndarray::Array1<bool>,
) -> (bool, bool, Option<usize>) {
    // Calculate course sizes
    let mut course_size = vec![0usize; courses.len()];
    for (p, course) in assignment.iter().enumerate() {
        if !is_instructor[p] {
            if let Some(c) = course {
                course_size[*c] += 1;
            }
        }
    }

    // Check if solution is infeasible, such that any participant is in an un-chosen course
    for (p, c) in assignment.iter().enumerate() {
        if !is_instructor[p]
            && !participants[p].is_instructor_only()
            && !participants[p]
                .choices
                .iter()
                .any(|choice| Some(choice.course_index) == *c)
        {
            // If so, get smallest non-constrained course, that has an instructor, who chose c
            let mut relevant_courses: Vec<usize> = (0..courses.len())
                .filter(|rc| !node.cancelled_courses.contains(rc))
                .filter(|rc| !node.enforced_courses.contains(rc))
                .filter(|rc| {
                    courses[*rc].instructors.iter().any(|instr| {
                        participants[*instr]
                            .choices
                            .iter()
                            .any(|choice| Some(choice.course_index) == *c)
                    })
                })
                .collect();
            if relevant_courses.is_empty() {
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
        if !node.cancelled_courses.contains(&c) && *size < courses[c].num_min {
            assert!(
                !node.enforced_courses.contains(&c),
                "Course {} has been enforced but still does not meet its minimum number: {} < {}",
                courses[c].index,
                *size,
                courses[c].num_min
            );
            let score = courses[c].num_min - *size;
            if score > max_score {
                max_score = score;
                course = Some(c);
            }
        }
    }
    (course.is_none(), false, course)
}

#[cfg(test)]
mod tests;
