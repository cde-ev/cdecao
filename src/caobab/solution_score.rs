use super::{edge_weight, Score, INSTRUCTOR_SCORE, WEIGHT_OFFSET};
use crate::{Assignment, Course, Participant};

/// Calculate a simple upper bound for the solution score of the given problem, assuming all course
/// instructors can instruct their course and all participants can get their best choice.
pub fn theoretical_max_score(participants: &[Participant], courses: &[Course]) -> Score {
    let mut participant_scores: Vec<Score> = participants
        .iter()
        .map(|p| {
            p.choices
                .iter()
                .map(|choice| edge_weight(choice) as Score)
                .max()
                .unwrap_or(0)
        })
        .collect();

    for course in courses {
        for instructor in course.instructors.iter() {
            // instructor_only participants are not considered in the score. See run_bab_node().
            if !participants[*instructor].is_instructor_only() {
                participant_scores[*instructor] = INSTRUCTOR_SCORE;
            }
        }
    }
    participant_scores.into_iter().sum()
}

/// Calculate a comparable solution quality score (invariant to participant changes and available course choices)
pub fn solution_quality(score: Score, participants: &Vec<Participant>) -> f32 {
    let num_real_participants = participants
        .iter()
        .filter(|p| !p.is_instructor_only())
        .count();
    (num_real_participants * WEIGHT_OFFSET as usize - score as usize) as f32
        / num_real_participants as f32
}

/// Calculate a solution quality score from data from a generic course assignment (not necessarily
/// created as a caobab solution)
///
/// # Parameters
/// - `number_instructors` -- Number of course instructors that have been assigned to their course
///   (and are not instructor-only participants)
/// - `assigned_course_choice_penalties` -- the penalty of the assigned course choice for all other
///   participants that are not assigned as course instructors.
pub fn generic_quality(
    number_instructors: usize,
    assigned_course_choice_penalties: &Vec<u32>,
) -> f32 {
    (number_instructors * (WEIGHT_OFFSET as u32 - INSTRUCTOR_SCORE) as usize
        + assigned_course_choice_penalties.iter().sum::<u32>() as usize) as f32
        / (assigned_course_choice_penalties.len() + number_instructors) as f32
}

/// Calculate a solution quality score from a given course assignment (not necessarily created as a
/// caobab solution)
pub fn assignment_quality(
    participants: &[Participant],
    courses: &[Course],
    assignment: &Assignment,
    unassigned_penalty: u32,
    unfulfilled_choices_penalty: u32,
) -> f32 {
    let mut number_instructors = 0;
    let mut assigned_course_choice_penalties = Vec::new();
    for (p_index, (p, assigned)) in participants.iter().zip(assignment).enumerate() {
        if let Some(c_index) = assigned {
            if courses[*c_index].instructors.contains(&p_index) {
                if !p.is_instructor_only() {
                    number_instructors += 1;
                }
            } else {
                if let Some(choice) = p
                    .choices
                    .iter()
                    .find(|choice| choice.course_index == *c_index)
                {
                    assigned_course_choice_penalties.push(choice.penalty);
                } else {
                    assigned_course_choice_penalties.push(unfulfilled_choices_penalty);
                }
            }
        } else if !p.is_instructor_only() {
            assigned_course_choice_penalties.push(unassigned_penalty);
        }
    }
    generic_quality(number_instructors, &assigned_course_choice_penalties)
}
