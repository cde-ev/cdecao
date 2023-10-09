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

mod bab;
pub mod caobab;
mod hungarian;
mod util;

pub mod io;

use serde::{Deserialize, Serialize};

/// Representation of an event participant's data
#[derive(Deserialize, Serialize)]
pub struct Participant {
    /// id/index of the Participant in the list of participants
    #[serde(skip)]
    index: usize,
    /// Participant's registration id in the CdE Datebank
    #[serde(skip)]
    dbid: usize,
    /// Participant's name. Mainly used for info/debug output
    name: String,
    /// Course choices
    pub choices: Vec<Choice>,
}

impl Participant {
    /// Check if this participant should be considered for assignment at all or is only there to be
    /// assigned as a course instructor (as long as their course is not cancelled)
    pub fn is_instructor_only(&self) -> bool {
        self.choices.is_empty()
    }
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct Choice {
    /// Index of the chosen choice in the list of courses
    #[serde(rename = "course")]
    course_index: usize,
    /// Negative weight of this course choice, e.g. 0 for first choice, 1 for second choice, etc.
    penalty: u32,
}

pub fn choices_from_list(choices: &[usize]) -> Vec<Choice> {
    choices
        .iter()
        .enumerate()
        .map(|(i, c)| Choice {
            course_index: *c,
            penalty: i as u32,
        })
        .collect()
}

/// Representation of an event course's data
#[derive(Deserialize, Serialize)]
pub struct Course {
    /// id/index of the Course in the list of courses
    #[serde(skip)]
    index: usize,
    /// Course's id in the CdE Datebank
    #[serde(skip)]
    dbid: usize,
    /// Course's name. Mainly used for info/debug output
    name: String,
    /// Maximum number of attendees (excl. course instructors)
    num_max: usize,
    /// Minimum number of attendees (excl. course instructors)
    num_min: usize,
    /// Indexes of course instructor's indexes in the list of participants
    instructors: Vec<usize>,
    /// Scaling factor for room size check: The room of this course must have
    /// >= room_offset + room_factor * num_participants (incl. instructors) places. E.g. for dancing
    /// courses this might be somewhere around 2.5
    #[serde(default = "default_room_factor")]
    room_factor: f32,
    /// Offset to add to the number of assigned participants to check if the course fits a room of a
    /// specific size
    #[serde(default)]
    room_offset: f32,
    /// If true, the course may *not* be cancelled by the assignment algorithm. This may be the
    /// case, if the course has fixed participants.
    #[serde(default)]
    fixed_course: bool,
    /// Additional participant names to be included in the printed result output
    #[serde(default)]
    hidden_participant_names: Vec<String>,
}

fn default_room_factor() -> f32 {
    1.0
}

/// A course assignment as result of the overall algorithm. It maps the participant index to the course index, such that
/// the course of participants\[i\] is courses\[assignment\[i\]\].
pub type Assignment = Vec<Option<usize>>;
