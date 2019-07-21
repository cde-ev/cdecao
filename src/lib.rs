mod bab;
pub mod caobab;
mod hungarian;

pub mod io;

/// Representation of an event participant's data
pub struct Participant {
    /// id/index of the Participant in the list of participants
    pub index: usize,
    /// Participant's registration id in the CdE Datebank
    pub dbid: usize,
    /// Participant's name. Mainly used for info/debug output
    pub name: String,
    /// Course choices of the participant as indexes into the list of courses
    pub choices: Vec<usize>,
}

/// Representation of an event course's data
pub struct Course {
    /// id/index of the Course in the list of courses
    pub index: usize,
    /// Course's id in the CdE Datebank
    pub dbid: usize,
    /// Course's name. Mainly used for info/debug output
    pub name: String,
    /// Maximum number of attendees
    pub num_max: usize,
    /// Maximum number of attendees
    pub num_min: usize,
    /// Ids of course instructor's indexes in the
    pub instructors: Vec<usize>,
}

/// A course assignment as result of the overall algorithm. It maps the participant index to the course index, such that
/// the course of participants[i] is courses[assignment[i]].
pub type Assignment = Vec<usize>;
