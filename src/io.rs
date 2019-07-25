pub mod cdedb;

use super::{Assignment, Course, Participant};
use std::fmt::Write;

/// Format the calculated course assignment into a human readable String (e.g. to print it to
/// stdout).
///
/// The output format will look like
/// ```
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
