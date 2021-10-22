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
    /// Course choices of the participant as indexes into the list of courses
    pub choices: Vec<usize>,
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
    /// Ids of course instructor's indexes in the
    instructors: Vec<usize>,
    // TODO implement room_factors
    // /// Scaling factor for room size check: The room of this course must have
    // /// >= room_factor * num_participants (incl. instructors) places. E.g. for dancing courses
    // /// this might be somewhere around 2.5
    // room_factor: f32
    /// Offset to add to the number of assigned participants to check if the course fits a room of a
    /// specific size
    #[serde(default)]
    room_offset: usize,
    /// If true, the course may *not* be cancelled by the assignment algorithm. This may be the
    /// case, if the course has fixed participants.
    #[serde(default)]
    fixed_course: bool,
}

/// A course assignment as result of the overall algorithm. It maps the participant index to the course index, such that
/// the course of participants[i] is courses[assignment[i]].
pub type Assignment = Vec<usize>;
