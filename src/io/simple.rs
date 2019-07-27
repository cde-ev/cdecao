

use crate::{Course, Participant};
use serde_json::json;

/// Read the list of participants and courses from the simple JSON representation (canonical
/// serde_json serialization of `Participant` and `Course` objects).
pub fn read<R: std::io::Read>(reader: R) -> Result<(Vec<Participant>, Vec<Course>), String> {
    let mut data: serde_json::Value = serde_json::from_reader(reader).map_err(|err| err.to_string())?;

    let mut participants: Vec<Participant> = serde_json::from_value(data["participants"].take()).map_err(|e| format!("{}", e))?;
    for (i, mut p) in participants.iter_mut().enumerate() {
        p.index = i;
    }
    let mut courses: Vec<Course> = serde_json::from_value(data["courses"].take()).map_err(|e| format!("{}", e))?;
    for (i, mut c) in courses.iter_mut().enumerate() {
        c.index = i;
    }

    Ok((participants, courses))
}

/// Write the list of participants and courses to the simple JSON representation (canonical
/// serde_json serialization of `Participant` and `Course` objects).
pub fn write<W: std::io::Write>(writer: W, participants: &Vec<Participant>, courses: &Vec<Course>) -> Result<(), String> {

    let p: serde_json::Value = serde_json::to_value(participants).map_err(|e| format!("{}", e))?;
    let c: serde_json::Value = serde_json::to_value(courses).map_err(|e| format!("{}", e))?;
    let data = json!({
        "format": "1.0",
        "participants": p,
        "courses": c,
    });
    serde_json::to_writer(writer, &data);

    Ok(())
}