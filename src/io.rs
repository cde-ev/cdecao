pub mod cdedb;
pub mod simple;

use super::{Assignment, Course, Participant};
use std::fmt::Write;

/// Format the calculated course assignment into a human readable String (e.g. to print it to
/// stdout).
///
/// The output format will look like
/// ```text
/// ===== Course name =====
/// Anton Administrator
/// Bertalotta Beispiel
///
/// ===== Another course name =====
///
/// ===== A third course name =====
/// …
/// ```
pub fn format_assignment(
    assignment: &Assignment,
    courses: &Vec<Course>,
    participants: &Vec<Participant>,
) -> String {
    let mut result = String::new();
    for c in courses.iter() {
        write!(result, "\n===== {} =====\n", c.name).unwrap();
        for (ap, ac) in assignment.iter().enumerate() {
            if *ac == c.index {
                write!(
                    result,
                    "{}{}\n",
                    participants[ap].name,
                    if c.instructors.contains(&ap) {
                        " (instr)"
                    } else {
                        ""
                    }
                )
                .unwrap();
            }
        }
    }
    return result;
}

/// Assert that a given courses/participants data structure is consistent (in terms of object's
/// indexes and cross referencing indexes)
pub fn assert_data_consitency(participants: &Vec<Participant>, courses: &Vec<Course>) {
    for (i, p) in participants.iter().enumerate() {
        assert_eq!(i, p.index, "Index of {}. participant is {}", i, p.index);
        for c in p.choices.iter() {
            assert!(*c < courses.len(), "Choice {} of {}. participant is invalid", c, i);
        }
    }
    for (i, c) in courses.iter().enumerate() {
        assert_eq!(i, c.index, "Index of {}. course is {}", i, c.index);
        for instr in c.instructors.iter() {
            assert!(*instr < participants.len(), "Instructor {} of {}. course is invalid", instr, i);
        }
    }
}
