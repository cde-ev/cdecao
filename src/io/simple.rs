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

use crate::{Assignment, Course, Participant};
use serde_json::json;

/// Read the list of participants and courses from the simple JSON representation (canonical
/// serde_json serialization of `Participant` and `Course` objects).
pub fn read<R: std::io::Read>(reader: R) -> Result<(Vec<Participant>, Vec<Course>), String> {
    let mut data: serde_json::Value =
        serde_json::from_reader(reader).map_err(|err| err.to_string())?;

    let participants_data = data
        .get_mut("participants")
        .ok_or("No 'participants' found in data.")?;
    let mut participants: Vec<Participant> =
        serde_json::from_value(participants_data.take()).map_err(|e| format!("{}", e))?;
    for (i, mut p) in participants.iter_mut().enumerate() {
        p.index = i;
    }
    let courses_data = data
        .get_mut("courses")
        .ok_or("No 'courses' found in data.")?;
    let mut courses: Vec<Course> =
        serde_json::from_value(courses_data.take()).map_err(|e| format!("{}", e))?;
    for (i, mut c) in courses.iter_mut().enumerate() {
        c.index = i;
    }

    Ok((participants, courses))
}

/// Write the calculated course assignment as simple JSON representation (canonical
/// serde_json serialization of `Assignmet` objects) to a Writer (e.g. an output file).
pub fn write<W: std::io::Write>(writer: W, assignment: &Assignment) -> Result<(), String> {
    let a: serde_json::Value = serde_json::to_value(assignment).map_err(|e| format!("{}", e))?;
    let data = json!({
        "format": "X-courseassignment-simple",
        "version": "1.0",
        "assignment": a
    });
    serde_json::to_writer(writer, &data).map_err(|e| format!("{}", e))?;

    Ok(())
}

/// Write the list of participants and courses to the simple JSON representation (canonical
/// serde_json serialization of `Participant` and `Course` objects).
pub fn write_input_data<W: std::io::Write>(
    writer: W,
    participants: &Vec<Participant>,
    courses: &Vec<Course>,
) -> Result<(), String> {
    let p: serde_json::Value = serde_json::to_value(participants).map_err(|e| format!("{}", e))?;
    let c: serde_json::Value = serde_json::to_value(courses).map_err(|e| format!("{}", e))?;
    let data = json!({
        "format": "X-coursedata-simple",
        "version": "1.0",
        "participants": p,
        "courses": c,
    });
    serde_json::to_writer(writer, &data).map_err(|e| format!("{}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_simple_file() {
        let data = include_bytes!("test_ressources/simple_input.json");
        let (participants, courses) = super::read(&data[..]).unwrap();

        super::super::assert_data_consitency(&participants, &courses);
        assert_eq!(participants.len(), 6);
        assert_eq!(courses.len(), 4);
        assert_eq!(participants[2].name, "Charly Clown");
        assert_eq!(participants[2].choices, vec![
            crate::Choice{course_index: 2, penalty: 0},
            crate::Choice{course_index: 0, penalty: 1},
            crate::Choice{course_index: 1, penalty: 42},
        ]);
        assert_eq!(courses[2].name, "3. The Third Course");
        assert_eq!(courses[2].num_min, 3);
        assert_eq!(courses[2].num_max, 20);
        assert_eq!(courses[2].instructors, vec![4]);
        assert_eq!(courses[2].room_offset, 12.0);
        assert_eq!(courses[2].fixed_course, false);
        assert_eq!(courses[0].room_offset, 0.0);
        assert_eq!(courses[1].fixed_course, true);
    }

    #[test]
    fn write_simple_file() {
        let assignment: crate::Assignment =
            vec![Some(0), Some(0), Some(2), Some(2), Some(2), Some(0)];
        let mut buffer = Vec::<u8>::new();
        let result = super::write(&mut buffer, &assignment);
        assert!(result.is_ok());

        // Parse buffer as JSON file
        let mut data: serde_json::Value = serde_json::from_reader(&buffer[..]).unwrap();
        let parsed_assignment =
            serde_json::from_value::<Vec<Option<usize>>>(data["assignment"].take()).unwrap();
        assert_eq!(assignment, parsed_assignment);
    }
}
