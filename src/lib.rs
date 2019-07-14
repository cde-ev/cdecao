mod bab;
pub mod caobab;
mod hungarian;

/// Representation of an event participant's data
pub struct Participant {
    /// id/index of the Participant in the list of participants
    index: usize,
    /// Participant's registration id in the CdE Datebank
    dbid: usize,
    /// Participant's name. Mainly used for info/debug output
    name: String,
    /// Course choices of the participant as indexes into the list of courses
    choices: Vec<usize>,
}

/// Representation of an event course's data
pub struct Course {
    /// id/index of the Course in the list of courses
    index: usize,
    /// Course's id in the CdE Datebank
    dbid: usize,
    /// Course's name. Mainly used for info/debug output
    name: String,
    /// Maximum number of attendees
    num_max: usize,
    /// Maximum number of attendees
    num_min: usize,
    /// Ids of course instructor's indexes in the
    instructors: Vec<usize>,
}

/// A course assignment as result of the overall algorithm. It maps the participant index to the course index, such that
/// the course of participants[i] is courses[assignment[i]].
pub type Assignment = Vec<usize>;
