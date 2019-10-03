use crate::{Assignment, Course, Participant};
use serde_json::json;

/// Read the list of participants and courses from the simple JSON representation (canonical
/// serde_json serialization of `Participant` and `Course` objects).
pub fn read<R: std::io::Read>(reader: R) -> Result<(Vec<Participant>, Vec<Course>), String> {
    let mut data: serde_json::Value =
        serde_json::from_reader(reader).map_err(|err| err.to_string())?;

    let mut participants: Vec<Participant> =
        serde_json::from_value(data["participants"].take()).map_err(|e| format!("{}", e))?;
    for (i, mut p) in participants.iter_mut().enumerate() {
        p.index = i;
    }
    let mut courses: Vec<Course> =
        serde_json::from_value(data["courses"].take()).map_err(|e| format!("{}", e))?;
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
mod test {
    #[test]
    fn parse_simple_file() {
        let data = include_bytes!("test_ressources/simple_input.json");
        let (participants, courses) = super::read(&data[..]).unwrap();

        super::super::assert_data_consitency(&participants, &courses);
        assert_eq!(participants.len(), 6);
        assert_eq!(courses.len(), 4);
        assert_eq!(participants[2].name, "Charly Clown");
        assert_eq!(participants[2].choices, vec![2, 0, 1]);
        assert_eq!(courses[2].name, "3. The Third Course");
        assert_eq!(courses[2].num_min, 3);
        assert_eq!(courses[2].num_max, 20);
        assert_eq!(courses[2].instructors, vec![4]);
        assert_eq!(courses[2].room_offset, 12);
        assert_eq!(courses[2].fixed_course, false);
        assert_eq!(courses[0].room_offset, 0);
        assert_eq!(courses[1].fixed_course, true);
    }

    #[test]
    fn write_simple_file() {
        let assignment: crate::Assignment = vec![0, 0, 2, 2, 2, 0];
        let mut buffer = Vec::<u8>::new();
        let result = super::write(&mut buffer, &assignment);
        assert!(result.is_ok());

        // Parse buffer as JSON file
        let mut data: serde_json::Value = serde_json::from_reader(&buffer[..]).unwrap();
        let parsed_assignment =
            serde_json::from_value::<Vec<usize>>(data["assignment"].take()).unwrap();
        assert_eq!(assignment, parsed_assignment);
    }

}
