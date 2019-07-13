
mod hungarian;
mod bab;
pub mod caobab;


/// Representation of an event participant's data
pub struct Participant {
    /// id/index of the Participant in the list of participants
    index: u32,
    /// Participant's registration id in the CdE Datebank
    dbid: u32,
    /// Participant's name. Mainly used for info/debug output
    name: String,
    /// Course choices of the participant as indexes into the list of courses
    choices: Vec<u32>
}

/// Representation of an event course's data
pub struct Course {
    /// id/index of the Course in the list of courses
    index: u32,
    /// Course's id in the CdE Datebank
    dbid: u32,
    /// Course's name. Mainly used for info/debug output
    name: String,
    /// Maximum number of attendees
    num_max: u32,
    /// Maximum number of attendees
    num_min: u32,
    /// Ids of course instructor's indexes in the 
    instructors: Vec<u32>
}

pub type Mapping = Vec<(u32, u32)>;

