//! IO functionality for reading the list of available course rooms from a json file and attaching
//! the additional information to the assignment result.

use serde::Deserialize;

use crate::{Assignment, Course};

/// representation of a named course room kind in the rooms JSON file
#[derive(Deserialize, Debug, PartialEq)]
pub struct CourseRoomKind {
    /// Name for this kind of course room
    name: String,
    /// Capacity of a single course room of this kind as number of possible course participants. Multiple room kinds
    /// with the same capacity are allowed.
    capacity: usize,
    /// Number of available rooms of this kind
    quantity: usize,
}

/// Read the available course rooms from a JSON-serialized list of course room kinds
pub fn read<R: std::io::Read>(reader: R) -> Result<(Vec<usize>, Vec<CourseRoomKind>), String> {
    let mut room_kinds =
        serde_json::from_reader::<_, Vec<CourseRoomKind>>(reader).map_err(|err| err.to_string())?;

    room_kinds.sort_by_key(|room_kind| room_kind.capacity);
    room_kinds.reverse();

    let rooms = room_kinds
        .iter()
        .flat_map(|room_kind| std::iter::repeat(room_kind.capacity).take(room_kind.quantity))
        .collect();

    Ok((rooms, room_kinds))
}

/// Returns a human-readable list of possible course room kind names in the form
/// "room kind 1, room kind 2" for each course
pub fn get_course_room_kind_names(
    assignment: &Assignment,
    courses: &[Course],
    room_kinds: &[CourseRoomKind],
) -> Vec<String> {
    let rooms: Vec<usize> = room_kinds
        .iter()
        .flat_map(|room_kind| std::iter::repeat(room_kind.capacity).take(room_kind.quantity))
        .collect();

    let course_rooms = calculate_possible_course_room_sizes(assignment, courses, rooms);
    course_rooms
        .into_iter()
        .map(|rooms| {
            rooms
                .into_iter()
                .flat_map(|r| {
                    room_kinds
                        .iter()
                        .filter(move |rk| rk.capacity == r)
                        .map(|rk| rk.name.as_str())
                })
                .collect::<Vec<&str>>()
                .join(", ")
        })
        .collect()
}

/// Returns a human-readable list of possible course room sizes in the form "15, 12, 10" for each
/// course
pub fn get_course_room_size_list(
    assignment: &Assignment,
    courses: &[Course],
    rooms: &[usize],
) -> Vec<String> {
    let course_rooms = calculate_possible_course_room_sizes(assignment, courses, rooms.into());
    course_rooms
        .into_iter()
        .map(|rooms| {
            rooms
                .into_iter()
                .map(|r| r.to_string())
                .collect::<Vec<String>>()
                .join(", ")
        })
        .collect()
}

/// Helper function for get_course_room_size_list() and get_course_room_kind_names():
/// Returns a list of possible course room sizes for each course (in descending order)
///
/// It is assumed, that `assignment` is a valid course assignment, such that for each course,
/// a matching room can be found.
fn calculate_possible_course_room_sizes(
    assignment: &Assignment,
    courses: &[Course],
    mut rooms: Vec<usize>,
) -> Vec<Vec<usize>> {
    let mut course_sizes = crate::caobab::room_effective_course_sizes(assignment, courses);
    course_sizes.sort_unstable_by_key(|(_c, s)| std::cmp::Reverse(*s));
    let num = courses.len();
    rooms.sort_unstable_by_key(|v| std::cmp::Reverse(*v));
    let mut result: Vec<(&Course, Vec<usize>)> = course_sizes
        .iter()
        .map(|(c, _s)| (*c, Vec::new()))
        .collect();

    for i in 0..num {
        for j in i..rooms.len() {
            if rooms[j] < course_sizes[i].1 {
                break;
            }
            result[i].1.push(rooms[j]);
            if j < num {
                result[j].1.push(rooms[i]);
            }
        }
    }
    result.sort_by_key(|c| c.0.index);
    for (_c, rooms) in result.iter_mut() {
        // For dedup to work, we need to guarantee that the vector is sorted. This is given by the
        // order of insertion of the room sizes above: For each course, we first insert rooms which
        // are "mapped" to larger courses (beginning with the largest room). Afterwards we add the
        // "mapped" room of the course and all smaller rooms, again beginning with the largest room.
        // As a result, the rooms vector of each course is monotonic decreasing.
        rooms.dedup();
    }
    result.into_iter().map(|(_c, r)| r).collect()
}

#[cfg(test)]
mod tests {
    use std::vec;

    use crate::{io::rooms::CourseRoomKind, Course};

    fn create_courses_with_room_offset_factor(offset_factor: &[(f32, f32)]) -> Vec<Course> {
        offset_factor
            .iter()
            .enumerate()
            .map(|(i, (room_offset, room_factor))| Course {
                index: i,
                dbid: i,
                name: format!("Course {}", i),
                num_min: 2,
                num_max: 10,
                instructors: vec![],
                room_factor: *room_factor,
                room_offset: *room_offset,
                fixed_course: false,
                hidden_participant_names: vec![],
            })
            .collect()
    }

    #[test]
    fn simple_test_get_course_room_sizes() {
        let courses =
            create_courses_with_room_offset_factor(&[(0.0, 2.0), (10.0, 1.0), (0.0, 1.5)]);
        let assignment = [0, 0, 0, 1, 1, 2, 2, 2].iter().map(|v| Some(*v)).collect();
        // effective room sizes:
        // course 0:    3*2   =  6
        // course 1: 10+2     = 12
        // course 2:    3*1.5 =  5
        let rooms = vec![15, 7, 7, 6, 3];

        let assigned_course_rooms =
            super::calculate_possible_course_room_sizes(&assignment, &courses, rooms);
        let expected_course_rooms: Vec<Vec<usize>> = vec![vec![7, 6], vec![15], vec![7, 6]];
        assert_eq!(assigned_course_rooms, expected_course_rooms);
    }

    #[test]
    fn simple_test_get_course_room_size_list() {
        let courses =
            create_courses_with_room_offset_factor(&[(10.0, 1.0), (0.0, 2.0), (0.0, 1.5)]);
        let assignment = [0, 0, 1, 1, 1, 2, 2, 2].iter().map(|v| Some(*v)).collect();
        // effective room sizes:
        // course 0: 10+2     = 12
        // course 1:    3*2   =  6
        // course 2:    3*1.5 =  5
        let rooms = [15, 7, 7, 6, 3];

        let assigned_course_rooms = super::get_course_room_size_list(&assignment, &courses, &rooms);
        let expected_course_rooms: Vec<String> = vec!["15".into(), "7, 6".into(), "7, 6".into()];
        assert_eq!(assigned_course_rooms, expected_course_rooms);
    }

    #[test]
    fn simple_test_get_course_room_kind_names() {
        let courses = create_courses_with_room_offset_factor(&[
            (10.0, 1.0),
            (0.0, 2.0),
            (0.0, 1.5),
            (0.0, 1.0),
        ]);
        let assignment = [0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3]
            .iter()
            .map(|v| Some(*v))
            .collect();
        // effective room sizes:
        // course 0: 10+2     = 12
        // course 1:    3*2   =  6
        // course 2:    3*1.5 =  5
        // course 3:    3*1   =  3
        let room_kinds = [
            CourseRoomKind {
                name: "Seminar Room".into(),
                capacity: 15,
                quantity: 1,
            },
            CourseRoomKind {
                name: "Meeting Room".into(),
                capacity: 6,
                quantity: 2,
            },
            CourseRoomKind {
                name: "Seating Area".into(),
                capacity: 6,
                quantity: 1,
            },
            CourseRoomKind {
                name: "Normal Room".into(),
                capacity: 3,
                quantity: 1,
            },
            CourseRoomKind {
                name: "Office".into(),
                capacity: 1,
                quantity: 1,
            },
        ];

        let assigned_course_rooms =
            super::get_course_room_kind_names(&assignment, &courses, &room_kinds);
        let expected_course_rooms: Vec<String> = vec![
            "Seminar Room".into(),
            "Meeting Room, Seating Area".into(),
            "Meeting Room, Seating Area".into(),
            "Meeting Room, Seating Area, Normal Room".into(),
        ];
        assert_eq!(assigned_course_rooms, expected_course_rooms);
    }

    #[test]
    fn test_read() {
        let data = include_bytes!("test_ressources/rooms_example.json");
        let (rooms, room_kinds) = super::read(&data[..]).unwrap();

        let expected_rooms = [15, 6, 6, 1];
        let expected_room_kinds = vec![
            CourseRoomKind {
                name: "Seminar Room".into(),
                capacity: 15,
                quantity: 1,
            },
            CourseRoomKind {
                name: "Meeting Room".into(),
                capacity: 6,
                quantity: 2,
            },
            CourseRoomKind {
                name: "Office".into(),
                capacity: 1,
                quantity: 1,
            },
        ];
        assert_eq!(room_kinds, expected_room_kinds);
        assert_eq!(rooms, expected_rooms);
    }
}
