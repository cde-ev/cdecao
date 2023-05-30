// Copyright 2019 by Michael Thies <mail@mhthies.de>
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

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
    courses: &[Course],
    participants: &[Participant],
) -> String {
    let mut result = String::new();
    for c in courses.iter() {
        write!(result, "\n===== {} =====\n", c.name).unwrap();
        for (ap, ac) in assignment.iter().enumerate() {
            if *ac == c.index {
                writeln!(
                    result,
                    "{}{}",
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

    result
}

pub fn debug_list_of_courses(courses: &[Course]) -> String {
    courses.iter()
        .map(|c| format!("{:02} {}", c.index, c.name))
        .collect::<Vec<String>>()
        .join("\n")
}


/// Assert that a given courses/participants data structure is consistent (in terms of object's
/// indexes and cross referencing indexes)
pub fn assert_data_consitency(participants: &Vec<Participant>, courses: &Vec<Course>) {
    for (i, p) in participants.iter().enumerate() {
        assert_eq!(i, p.index, "Index of {}. participant is {}", i, p.index);
        for c in p.choices.iter() {
            assert!(
                *c < courses.len(),
                "Choice {} of {}. participant is invalid",
                c,
                i
            );
        }
    }
    for (i, c) in courses.iter().enumerate() {
        assert_eq!(i, c.index, "Index of {}. course is {}", i, c.index);
        for instr in c.instructors.iter() {
            assert!(
                *instr < participants.len(),
                "Instructor {} of {}. course is invalid",
                instr,
                i
            );
        }

        assert!(
            c.num_min <= c.num_max,
            "Min size ({}) > max size ({}) of course {}",
            c.num_min,
            c.num_max,
            c.index
        );
    }
}
