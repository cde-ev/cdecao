use std::fmt::Display;

use serde::Serialize;

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
pub fn solution_quality(score: Score, participants: &[Participant]) -> f32 {
    let num_real_participants = participants
        .iter()
        .filter(|p| !p.is_instructor_only())
        .count();
    (num_real_participants * WEIGHT_OFFSET as usize - score as usize) as f32
        / num_real_participants as f32
}

/// Calculate a comparable solution quality score for a combined assignment from a cdecao solution
/// and external assignment quality data
pub fn combined_quality(
    score: Score,
    participants: &[Participant],
    external_assignment_data: &AssignmentQualityInfo,
) -> f32 {
    let num_real_participants = participants
        .iter()
        .filter(|p| !p.is_instructor_only())
        .count();
    (num_real_participants * WEIGHT_OFFSET as usize - score as usize
        + external_assignment_data.number_instructors
            * (WEIGHT_OFFSET as u32 - INSTRUCTOR_SCORE) as usize
        + external_assignment_data
            .assigned_course_choice_penalties
            .iter()
            .sum::<u32>() as usize) as f32
        / (num_real_participants
            + external_assignment_data
                .assigned_course_choice_penalties
                .len()
            + external_assignment_data.number_instructors) as f32
}

pub struct AssignmentQualityInfo {
    /// Number of course instructors that have been assigned to their course (and are not
    /// instructor-only participants)
    number_instructors: usize,
    /// the penalty of the assigned course choice for all other participants that are not assigned
    /// as course instructors.
    assigned_course_choice_penalties: Vec<u32>,
}

impl AssignmentQualityInfo {
    pub fn new(number_instructors: usize, assigned_course_choice_penalties: Vec<u32>) -> Self {
        Self {
            number_instructors,
            assigned_course_choice_penalties,
        }
    }

    pub fn from_caobab_assignment(
        participants: &[Participant],
        courses: &[Course],
        assignment: &Assignment,
        unassigned_penalty: u32,
        unfulfilled_choices_penalty: u32,
    ) -> Self {
        let mut number_instructors = 0;
        let mut assigned_course_choice_penalties = Vec::new();
        for (p_index, (p, assigned)) in participants.iter().zip(assignment).enumerate() {
            if let Some(c_index) = assigned {
                if courses[*c_index].instructors.contains(&p_index) {
                    if !p.is_instructor_only() {
                        number_instructors += 1;
                    }
                } else if let Some(choice) = p
                    .choices
                    .iter()
                    .find(|choice| choice.course_index == *c_index)
                {
                    assigned_course_choice_penalties.push(choice.penalty);
                } else {
                    assigned_course_choice_penalties.push(unfulfilled_choices_penalty);
                }
            } else if !p.is_instructor_only() {
                assigned_course_choice_penalties.push(unassigned_penalty);
            }
        }
        Self {
            number_instructors,
            assigned_course_choice_penalties,
        }
    }

    pub fn add_assigned_choice_penalty(&mut self, penalty: u32) {
        self.assigned_course_choice_penalties.push(penalty);
    }

    pub fn add_instructor(&mut self) {
        self.number_instructors += 1;
    }

    pub fn get_quality(&self) -> f32 {
        (self.number_instructors * (WEIGHT_OFFSET as u32 - INSTRUCTOR_SCORE) as usize
            + self.assigned_course_choice_penalties.iter().sum::<u32>() as usize) as f32
            / (self.assigned_course_choice_penalties.len() + self.number_instructors) as f32
    }
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
    AssignmentQualityInfo::from_caobab_assignment(
        participants,
        courses,
        assignment,
        unassigned_penalty,
        unfulfilled_choices_penalty,
    )
    .get_quality()
}

/// Combined struct of all the quality info that a user (human or wrapper program) might be
/// interested in
#[derive(Serialize)]
pub struct QualityInfo {
    pub solution_score: Score,
    pub theoretical_max_score: Score,
    pub solution_quality: f32,
    pub theoretical_max_quality: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overall_quality: Option<f32>,
}

impl QualityInfo {
    pub fn calculate(
        solution_score: Score,
        participants: &[Participant],
        courses: &[Course],
        external_assignment_data: Option<&AssignmentQualityInfo>,
    ) -> Self {
        let theoretical_max_score = theoretical_max_score(participants, courses);
        Self {
            solution_score,
            theoretical_max_score,
            solution_quality: solution_quality(solution_score, participants),
            theoretical_max_quality: solution_quality(theoretical_max_score, participants),
            overall_quality: external_assignment_data
                .map(|external| combined_quality(solution_score, participants, external)),
        }
    }
}

impl Display for QualityInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Solution score:                     {: >9}
(Perfect matching would have been:  {: >9})
----------------------------------------------
Solution quality lack:               {: >8.6}
(Perfect matching would have been:   {: >8.6})
{}\n",
            self.solution_score,
            self.theoretical_max_score,
            self.solution_quality,
            self.theoretical_max_quality,
            match self.overall_quality {
                Some(q) => format!("New overall assignment quality lack: {: >8.6}", q),
                None => "".to_owned(),
            },
        )
    }
}
