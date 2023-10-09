// Copyright 2020 by Michael Thies <mail@mhthies.de>
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

//! IO functionality for use of this program with the CdE Datenbank export and import file formats.

use crate::{Assignment, Choice, Course, Participant};
use std::collections::HashMap;

use chrono::{SecondsFormat, Utc};
use serde_json::json;
use std::cmp::max;

use log::{info, warn};

const MINIMUM_EXPORT_VERSION: (u64, u64) = (7, 0);
const MAXIMUM_EXPORT_VERSION: (u64, u64) = (16, std::u64::MAX);
const OUTPUT_EXPORT_VERSION: (u64, u64) = (16, 0);

pub struct ImportAmbienceData {
    event_id: u64,
    track_id: u64,
}

/// Read course and participant data from an JSON event export of the CdE Datenbank
///
/// This function takes a Reader (e.g. an open filehandle), reads its contents and interprets them
/// as a partial event export from the CdE Datenbank 2.
///
/// If the event data comprises multiple course tracks and no track id is selected via the `track`
/// parameter, this function fails with "Event has more than one course track". Otherwise, the only
/// existing track is selected automatically.
///
/// Only registrations with status *Participant* in the relevant part (the part of the selected
/// course track) are considered. Existing course assignments and cancelled course segments are
/// ignored—they will be overridden by importing the result file into the CdE Datenbank.
///
/// Minimum and maximum participant numbers of courses are interpreted as total number of attendees,
/// **including** course instructors. The registered course
/// If no maximum size is given for a course, we assume num_max = 25 (incl. instructors).
/// If no minimum size is given for a course, we assume num_min = 0 (excl. instructors).
///
/// # Arguments
///
/// * reader: The Reader (e.g. open file) to read the json data from
/// * track: The CdEDB id of the event's course track, if the user specified one on the command
///   line. If None and the event has only one course track, it is selected automatically.
/// * ignore_inactive_courses: If true, courses with an inactive segment in the relevant track are
///   not added to the results.
/// * ignore_assigned: If true, participants who are assigned to a valid course are not added to the
///   results. If `ignore_inactive_courses` is true, participants assigned to a cancelled course are
///   not ignored.
///
/// # Errors
///
/// Fails with a string error message to be displayed to the user, if
/// * the file has invalid JSON syntax (the string representation of the serde_json error is returned)
/// * the file is not a 'partial' CdEDB export
/// * the file has no version within the supported version range (MINIMUM_/MAXIMUM_EXPORT_VERSION)
/// * any expected entry in the json fields is missing
/// * the event has no course tracks
/// * the event has more than one course track, but no `track` is given.
///
pub fn read<R: std::io::Read>(
    reader: R,
    track: Option<u64>,
    ignore_inactive_courses: bool,
    ignore_assigned: bool,
    room_factor_field: Option<&str>,
    room_offset_field: Option<&str>,
) -> Result<(Vec<Participant>, Vec<Course>, ImportAmbienceData), String> {
    let data: serde_json::Value = serde_json::from_reader(reader).map_err(|err| err.to_string())?;
    check_export_type_and_version(&data)?;

    // Find part and track ids
    let parts_data = data
        .get("event")
        .and_then(|v| v.as_object())
        .ok_or("No 'event' object found in data.")?
        .get("parts")
        .and_then(|v| v.as_object())
        .ok_or("No 'parts' object found in event.")?;
    let (part_id, track_id) = find_track(parts_data, track)?;

    // Parse courses
    let mut courses = Vec::new();
    let mut skipped_course_ids = Vec::new(); // Used to ignore KeyErrors for those later
    let courses_data = data
        .get("courses")
        .and_then(|v| v.as_object())
        .ok_or("No 'courses' object found in data.".to_owned())?;
    for (course_id, course_data) in courses_data.iter() {
        let course_id: usize = course_id
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;

        let (course_name, course_status, num_min, num_max, sort_key) =
            parse_course_base_data(course_id, course_data, track_id)?;

        if matches!(course_status, CourseStatus::NotOffered) {
            skipped_course_ids.push(course_id);
            continue;
        }
        if matches!(course_status, CourseStatus::Cancelled) && ignore_inactive_courses {
            skipped_course_ids.push(course_id);
            continue;
        }

        let (room_factor, room_offset) = extract_room_factor_fields(
            course_data,
            &course_name,
            room_factor_field,
            room_offset_field,
        )?;

        courses.push((
            sort_key,
            crate::Course {
                index: 0,
                dbid: course_id,
                name: course_name,
                num_min,
                num_max,
                instructors: Vec::new(),
                room_factor,
                room_offset,
                fixed_course: false,
                hidden_participant_names: Vec::new(),
            },
        ));
    }

    // Sort courses, drop sort key and add indexes
    courses.sort_by(|(k1, _), (k2, _)| k1.cmp(k2));
    let mut courses: Vec<crate::Course> = courses.into_iter().map(|(_k, c)| c).collect();
    for (index, course) in courses.iter_mut().enumerate() {
        course.index = index;
    }

    // Store, how many instructors attendees are already set for each course (only relevant if
    // ignore_assigned == true). The vector holds a tuple
    // (num_hidden_instructors, num_hidden_attendees) for each course in the same order as the
    // `courses` vector.
    let mut invisible_course_participants = vec![(0usize, 0usize); courses.len()];
    let mut course_index_by_id: HashMap<u64, Option<usize>> = courses
        .iter()
        .map(|r| (r.dbid as u64, Some(r.index)))
        .collect();
    for course_id in skipped_course_ids {
        course_index_by_id.insert(course_id as u64, None);
    }

    // Parse Registrations
    let mut registrations = Vec::new();
    let registrations_data = data
        .get("registrations")
        .and_then(|v| v.as_object())
        .ok_or("No 'registrations' object found in data.".to_owned())?;
    let mut i = 0;
    for (reg_id, reg_data) in registrations_data {
        let reg_id: u64 = reg_id
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;

        let (reg_state, reg_name) = extract_participant_base_data(reg_id, reg_data, part_id)?;

        if !matches!(reg_state, ParticipationState::Participant) {
            continue;
        }

        let (assigned_course, instructed_course, choices) = parse_participant_course_data(
            &format!("{} (id={})", reg_name, reg_id),
            reg_data,
            track_id,
            &course_index_by_id,
        )?;

        // Skip already assigned participants (if wanted)
        if ignore_assigned {
            if let Some(course_index) = assigned_course {
                match instructed_course {
                    // In case, they are (invisible) instructor of the course ...
                    Some(c) if c == course_index => {
                        invisible_course_participants[course_index].0 += 1;
                    }
                    // In case, they are (invisible) attendee of the course ...
                    _ => {
                        invisible_course_participants[course_index].1 += 1;
                    }
                };
                courses[course_index]
                    .hidden_participant_names
                    .push(reg_name);
                continue;
            }
        }

        // Filter out registrations without choices
        if choices.is_empty() && instructed_course.is_none() {
            warn!(
                "Ignoring participant '{}', who has no (valid) course choices.",
                reg_name
            );
            continue;
        }

        // Add course instructors to courses
        if let Some(instructed_course_index) = instructed_course {
            courses[instructed_course_index].instructors.push(i);
        }

        registrations.push(crate::Participant {
            index: i,
            dbid: reg_id as usize,
            name: reg_name,
            choices,
        });
        i += 1;
    }

    for course in courses.iter_mut() {
        adapt_course_for_invisible_participants(
            course,
            invisible_course_participants[course.index].0,
            invisible_course_participants[course.index].1,
        )
    }

    Ok((
        registrations,
        courses,
        ImportAmbienceData {
            event_id: data
                .get("id")
                .and_then(|v| v.as_u64())
                .ok_or("No event 'id' found in data")?,
            track_id,
        },
    ))
}

/**
 * Check the JSON data structure for the correct CdEDB export type ("partial") and version number
 *
 * # Arguments
 * - `data` -- The full JSON document parsed from the CdEDB export file
 *
 * # Result
 * Returns Ok(()) when the data indicates correct format and version; an error message otherwise.
 */
fn check_export_type_and_version(data: &serde_json::Value) -> Result<(), String> {
    let export_kind = data
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or("No 'kind' field found in data. Is this a correct CdEdb export file?")?;
    if export_kind != "partial" {
        return Err("The given JSON file is no 'Partial Export' of the CdE Datenbank".to_owned());
    }
    let export_version = if let Some(version_tag) = data.get("EVENT_SCHEMA_VERSION") {
        version_tag
            .as_array()
            .ok_or("'EVENT_SCHEMA_VERSION' is not an array!")
            .and_then(|v| {
                if v.len() == 2 {
                    Ok(v)
                } else {
                    Err("'EVENT_SCHEMA_VERSION' does not have 2 entries.")
                }
            })
            .and_then(|v| {
                v.iter()
                    .map(|x| {
                        x.as_u64()
                            .ok_or("Entry of 'EVENT_SCHEMA_VERSION' is not an u64 value.")
                    })
                    .collect::<Result<Vec<u64>, &str>>()
            })
            .map(|v| (v[0], v[1]))
    } else if let Some(version_tag) = data.get("CDEDB_EXPORT_EVENT_VERSION") {
        // Support for old export schema version field
        version_tag
            .as_u64()
            .ok_or("'CDEDB_EXPORT_EVENT_VERSION' is not an u64 value")
            .map(|v| (v, 0))
    } else {
        Err(
            "No 'EVENT_SCHEMA_VERSION' field found in data. Is this a correct CdEdb \
            export file?",
        )
    }?;
    if export_version < MINIMUM_EXPORT_VERSION || export_version > MAXIMUM_EXPORT_VERSION {
        return Err(format!(
            "The given CdE Datenbank Export is not within the supported version range \
            [{}.{},{}.{}]",
            MINIMUM_EXPORT_VERSION.0,
            MINIMUM_EXPORT_VERSION.1,
            MAXIMUM_EXPORT_VERSION.0,
            MAXIMUM_EXPORT_VERSION.1
        ));
    }

    Ok(())
}

enum CourseStatus {
    NotOffered,
    Cancelled,
    TakesPlace,
}

/**
 * Extract basic course information from a course object of the JSON data
 *
 * # Arguments
 * - `course_id` -- CdEDB id of the course for error message output
 * - `course_data` -- The course object from the CdEDB JSON export
 * - `track_id` -- The id of the event track for which the data shall be extracted
 *
 * # Return value
 * Returns a tuple (course_name, status, num_min, num_max).
 *
 * - The `course_name` is meant for stdout output and error messages. Thus, it is composed from the
 * course number and short name.
 * - `status` is determined with regard to the given `track_id`.
 * - `num_min` and `num_max` are -- according to the CdEDB convention -- counted excl. instructors
 * - A `sort_key` (based on the course number) for a simple sorting of the courses
 */
fn parse_course_base_data(
    course_id: usize,
    course_data: &serde_json::Value,
    track_id: u64,
) -> Result<(String, CourseStatus, usize, usize, String), String> {
    let course_segments_data = course_data
        .get("segments")
        .and_then(|v| v.as_object())
        .ok_or(format!(
            "No 'segments' object found for course {}",
            course_id
        ))?;

    let course_status = if let Some(v) = course_segments_data.get(&format!("{}", track_id)) {
        let v = v
            .as_bool()
            .ok_or(format!("Segment of course {} is not a boolean.", course_id))?;
        if v {
            CourseStatus::TakesPlace
        } else {
            CourseStatus::Cancelled
        }
    } else {
        CourseStatus::NotOffered
    };

    let course_nr = course_data
        .get("nr")
        .and_then(|v| v.as_str())
        .ok_or(format!("No 'nr' found for course {}", course_id))?;
    let course_name = format!(
        "{}. {}",
        course_nr,
        course_data
            .get("shortname")
            .and_then(|v| v.as_str())
            .ok_or(format!("No 'shortname' found for course {}", course_id))?
    );
    let sort_key = format!("{: >10}", course_nr);

    let num_max = course_data
        .get("max_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(25) as usize;
    let num_min = course_data
        .get("min_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    if num_max < num_min {
        return Err(format!(
            "Min participants > max participants for course '{}'",
            course_name
        ));
    }

    Ok((course_name, course_status, num_min, num_max, sort_key))
}

/**
 * Determine the room_factor and room_offset for a course from the course's JSON object
 *
 * # Arguments
 * - `course_name` -- name of the course for error logging output
 * - `course_data` -- The course object from the CdEDB JSON export
 * - `room_factor_field` -- Name of the CdEDB custom course field, containing the room size factor,
 *   if given by the user.
 * - `room_offset_field` -- Name of the CdEDB custom course field, containing the room size offset,
 *   if given by the user.
 *
 * # Return value
 * Returns a tuple (room_factor, room_offset).
 *
 * Each of the values will be set to default (1.0 resp. 0.0) if
 * - no field name is specified or
 * - the field is not present in this course's data
 * - the field contains data in a wrong data type.
 */
fn extract_room_factor_fields(
    course_data: &serde_json::Value,
    course_name: &str,
    room_factor_field: Option<&str>,
    room_offset_field: Option<&str>,
) -> Result<(f32, f32), String> {
    let fields = course_data
        .get("fields")
        .and_then(|v| v.as_object())
        .ok_or(format!("No 'fields' found for course {}", course_name))?;
    let room_factor = if let Some(field_name) = room_factor_field {
        match fields.get(field_name).and_then(|v| v.as_f64()) {
            Some(v) => v,
            None => {
                warn!("No numeric field '{}' as room_factor field found in course '{}'. Using the default value 1.0.", field_name, course_name);
                1.0
            }
        }
    } else {
        1.0
    };
    let room_offset = if let Some(field_name) = room_offset_field {
        match fields.get(field_name).and_then(|v| v.as_f64()) {
            Some(v) => v,
            None => {
                warn!("No numeric field '{}' as room_offset field found in course '{}'. Using the default value 0.0.", field_name, course_name);
                0.0
            }
        }
    } else {
        0.0
    };

    Ok((room_factor as f32, room_offset as f32))
}

enum ParticipationState {
    NotInvolved,
    Pending,
    Participant,
    Waitlist,
    Guest,
}

/**
 * Extract basic participant data (for one event part) from a registration object of the JSON data
 *
 * # Arguments
 * - `reg_id` -- CdEDB id of the registration (for error message output)
 * - `reg_data` -- The registration object from the CdEDB JSON export
 * - `part_id` -- The id of the event part for which the data shall be extracted
 *
 * # Return value
 * Returns a tuple `(participation_state, reg_name)`.
 *
 * `reg_name` is meant for stdout output and error messages. Thus, it is composed from the
 * given_names and last_name. (We do not consider the fancy first name selection logic from the
 * CdEDB here.)
 */
fn extract_participant_base_data(
    reg_id: u64,
    reg_data: &serde_json::Value,
    part_id: u64,
) -> Result<(ParticipationState, String), String> {
    let rp_data = reg_data
        .get("parts")
        .and_then(|v| v.as_object())
        .ok_or(format!("No 'parts' found in registration {}", reg_id))?
        .get(&format!("{}", part_id))
        .and_then(|v| v.as_object());

    let participation_state = if let Some(part) = rp_data {
        let status = part.get("status").and_then(|v| v.as_i64()).ok_or(format!(
            "Missing 'status' in registration_part record of reg {}",
            reg_id
        ))?;
        match status {
            1 => ParticipationState::Pending,
            2 => ParticipationState::Participant,
            3 => ParticipationState::Waitlist,
            4 => ParticipationState::Guest,
            _ => ParticipationState::NotInvolved,
        }
    } else {
        ParticipationState::NotInvolved
    };

    // Parse persona attributes
    let persona_data = reg_data
        .get("persona")
        .and_then(|v| v.as_object())
        .ok_or(format!("Missing 'persona' in registration {}", reg_id))?;
    let reg_name = format!(
        "{} {}",
        persona_data
            .get("given_names")
            .and_then(|v| v.as_str())
            .ok_or(format!("No 'given_name' found for registration {}", reg_id))?,
        persona_data
            .get("family_name")
            .and_then(|v| v.as_str())
            .ok_or(format!(
                "No 'family_name' found for registration {}",
                reg_id
            ))?
    );

    Ok((participation_state, reg_name))
}

/**
 * Extract course choice and assignment information from a registration object of the JSON data
 *
 * # Arguments
 * - `registration_name` -- Name of the participant (incl. id) for error message output
 * - `reg_data` -- The registration object from the CdEDB JSON export
 * - `track_id` -- The id of the event track for which the data shall be extracted
 * - `courses_by_id` -- A Map (CdEDB course id) -> (course index or None). Iff a course exists but
 *   ignored by the assignment algorithm, the map shall contain a None value for this course id.
 *
 * # Return value
 * Returns a tuple (assigned_course, instructed_course, [choices]).
 *
 * All courses are referenced by index according to `courses_by_id`.
 * assigned_course and instructed_course are None, iff no course is assigned/instructed or the
 * course is marked to be ignored.
 */
fn parse_participant_course_data(
    registration_name: &str,
    reg_data: &serde_json::Value,
    track_id: u64,
    courses_by_id: &HashMap<u64, Option<usize>>,
) -> Result<(Option<usize>, Option<usize>, Vec<Choice>), String> {
    let registration_track_data = reg_data
        .get("tracks")
        .and_then(|v| v.as_object())
        .ok_or(format!(
            "No 'tracks' found in registration {}",
            registration_name
        ))?
        .get(&format!("{}", track_id))
        .and_then(|v| v.as_object())
        .ok_or(format!(
            "Registration track data not present for registration {}",
            registration_name
        ))?;

    let assigned_course_id = registration_track_data
        .get("course_id")
        .ok_or(format!(
            "No 'course_id' found in registration track of {}",
            registration_name
        ))?
        .as_u64();

    let assigned_course_index = match assigned_course_id {
        Some(course_id) => {
            let course_index = courses_by_id.get(&course_id).ok_or(format!(
                "Assigned course {} of registration {} does not exist.",
                course_id, registration_name
            ))?;
            // If course_index is None, the course_id is valid, but the course is skipped/ignored
            course_index.as_ref().copied()
        }
        // No course assigned
        None => None,
    };

    let instructed_course_id = registration_track_data
        .get("course_instructor")
        .ok_or(format!(
            "No 'course_instructor' found in registration {}'s registration track",
            registration_name
        ))?
        .as_u64();

    let instructed_course_index = match instructed_course_id {
        Some(course_id) => {
            let course_index = courses_by_id.get(&course_id).ok_or(format!(
                "Instructed course {} of registration {} does not exist.",
                course_id, registration_name
            ))?;
            // If course_index is None, the course_id is valid, but the course is skipped/ignored
            course_index.as_ref().copied()
        }
        // No course instructed
        None => None,
    };

    let choices_data = registration_track_data
        .get("choices")
        .and_then(|v| v.as_array())
        .ok_or(format!(
            "No 'choices' found in registration track data of {}",
            registration_name
        ))?;

    let mut choices = Vec::<Choice>::with_capacity(choices_data.len());
    for (i, v) in choices_data.iter().enumerate() {
        let course_id = v
            .as_u64()
            .ok_or(format!("Course choice {:?} is no integer.", v))?;
        let course_index = courses_by_id.get(&course_id).ok_or(format!(
            "Course choice {} of registration {} does not exist.",
            course_id, registration_name
        ))?;
        if let Some(c) = course_index {
            choices.push(Choice {
                course_index: *c,
                penalty: i as u32,
            });
        }
    }

    if choices.is_empty() && !choices_data.is_empty() {
        info!(
            "Participant {}, only chose cancelled courses.",
            registration_name
        );
    }

    Ok((assigned_course_index, instructed_course_index, choices))
}

/// Adjust a Course to incorporate the "invisible" (ignored) participants in its size and offsets
///
/// "Invisible participants" are those, which are fix-assigned to a course and and thus ignored by
/// the assignment algorithm. I.e. they are not included in the list of participants, we provide
/// to the algorithm, nor in exported result. So, we need to adjust the course data accordingly, to
/// "save them a place" in the course.
///
/// The following modifications are made to the course:
/// * the fixed_course flag is set if there are any invisible participants (attendees + instructors)
/// * the min and max size of the course are reduced by the number of invisible attendees
/// * the room offset is increased by the number of invisible participants (attendees + instructors)
fn adapt_course_for_invisible_participants(
    course: &mut Course,
    invisible_instructors: usize,
    invisible_attendees: usize,
) {
    let total_invisible_course_participants = invisible_instructors + invisible_attendees;
    course.num_min = if invisible_attendees > course.num_min {
        0
    } else {
        course.num_min - invisible_attendees
    };
    course.num_max = if invisible_attendees > course.num_max {
        0
    } else {
        course.num_max - invisible_attendees
    };
    course.fixed_course = total_invisible_course_participants != 0;
    course.room_offset += total_invisible_course_participants as f32 * course.room_factor;
}

/// Write the calculated course assignment as a CdE Datenbank partial import JSON string to a Writer
/// (e.g. an output file).
pub fn write<W: std::io::Write>(
    writer: W,
    assignment: &Assignment,
    participants: &[Participant],
    courses: &Vec<Course>,
    ambience_data: ImportAmbienceData,
) -> Result<(), String> {
    // Calculate course sizes
    let mut course_size = vec![0usize; courses.len()];
    for (_p, course) in assignment.iter().enumerate() {
        if let Some(c) = course {
            course_size[*c] += 1;
        }
    }

    let registrations_json = assignment
        .iter()
        .enumerate()
        .filter(|(_pid, cid)| cid.is_some())
        .map(|(pid, cid)| {
            (
                format!("{}", participants[pid].dbid),
                json!({
                "tracks": {
                    format!("{}", ambience_data.track_id): {
                        "course_id": courses[cid.unwrap()].dbid
                    }
                }}),
            )
        })
        .collect::<serde_json::Map<String, serde_json::Value>>();

    let courses_json = course_size
        .iter()
        .enumerate()
        .map(|(cid, size)| {
            (
                format!("{}", courses[cid].dbid),
                json!({
                "segments": {
                    format!("{}", ambience_data.track_id): *size > 0 || courses[cid].fixed_course
                }}),
            )
        })
        .collect::<serde_json::Map<String, serde_json::Value>>();

    let data = json!({
        "EVENT_SCHEMA_VERSION": OUTPUT_EXPORT_VERSION,
        "kind": "partial",
        "id": ambience_data.event_id,
        "timestamp": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, false),
        "courses": courses_json,
        "registrations": registrations_json
    });
    serde_json::to_writer(writer, &data).map_err(|e| format!("{}", e))?;

    Ok(())
}

/// Helper function to find the specified course track or the single course track, if the event has
/// only one.
///
/// # Arguments
/// * parts_data: The JSON 'parts' object from the 'event' part of the export file
/// * track: The course track selected by the user (if any)
///
/// # Returns
/// part_id and track_id of the chosen course track or a user readable error string
fn find_track(
    parts_data: &serde_json::Map<String, serde_json::Value>,
    track: Option<u64>,
) -> Result<(u64, u64), String> {
    match track {
        // If a specific course track id is given, search for that id
        Some(t) => {
            for (part_id, part) in parts_data {
                let tracks_data = part
                    .get("tracks")
                    .and_then(|v| v.as_object())
                    .ok_or("Missing 'tracks' in event part.")?;
                for (track_id, _track) in tracks_data {
                    if track_id
                        .parse::<u64>()
                        .map_err(|e: std::num::ParseIntError| e.to_string())?
                        == t
                    {
                        return Ok((
                            part_id
                                .parse()
                                .map_err(|e: std::num::ParseIntError| e.to_string())?,
                            track_id
                                .parse()
                                .map_err(|e: std::num::ParseIntError| e.to_string())?,
                        ));
                    }
                }
            }

            Err(format!("Could not find course track with id {}.", t))
        }

        // Otherwise, check if there is only a single course track
        None => {
            let mut result: Option<(u64, u64)> = None;
            for (part_id, part) in parts_data {
                let tracks_data = part
                    .get("tracks")
                    .and_then(|v| v.as_object())
                    .ok_or("Missing 'tracks' in event part.")?;
                for (track_id, _track) in tracks_data {
                    if result.is_some() {
                        return Err(format!(
                            "Event has more than one course track. Please select one of the \
                             tracks:\n{}",
                            track_summary(parts_data)?
                        ));
                    }
                    result = Some((
                        part_id
                            .parse()
                            .map_err(|e: std::num::ParseIntError| e.to_string())?,
                        track_id
                            .parse()
                            .map_err(|e: std::num::ParseIntError| e.to_string())?,
                    ));
                }
            }

            result.ok_or("Event has no course track.".to_owned())
        }
    }
}

/// Helper function to generate a summary of the event's tracks and their IDs.
///
/// # Arguments
/// * parts_data: The JSON 'parts' object from the 'event' part of the export file
///
/// # Returns
/// A String containing a listing of the track ids and names to be printed to the command line
///
/// # Errors
/// Returns an error String, when
///
fn track_summary(
    parts_data: &serde_json::Map<String, serde_json::Value>,
) -> Result<String, String> {
    let mut tracks = Vec::new();
    let mut max_id_len = 0;

    for (_part_id, part) in parts_data {
        let tracks_data = part
            .get("tracks")
            .and_then(|v| v.as_object())
            .ok_or("Missing 'tracks' in event part.")?;
        for (track_id, track) in tracks_data {
            max_id_len = max(max_id_len, track_id.len());
            tracks.push((
                track_id,
                track
                    .get("title")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'title' in event track.")?,
                track
                    .get("sortkey")
                    .and_then(|v| v.as_i64())
                    .ok_or("Missing 'sortkey' in event track.")?,
            ));
        }
    }

    tracks.sort_by_key(|e| e.2);
    let result = tracks
        .iter()
        .map(|(id, title, _)| format!("{:>1$} : {2}", id, max_id_len, title))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(result)
}

#[cfg(test)]
mod tests {
    use crate::{choices_from_list, Assignment, Choice, Course, Participant};

    #[test]
    fn parse_testaka_sitzung() {
        let data = include_bytes!("test_ressources/TestAka_partial_export_event.json");
        let (participants, courses, import_ambience) =
            super::read(&data[..], Some(3), false, false, None, None).unwrap();

        super::super::assert_data_consitency(&participants, &courses);
        // Check courses
        // Course "γ. Kurz" is not offered in this track, thus it should not exist in the parsed data
        assert_eq!(courses.len(), 5);
        assert!(find_course_by_id(&courses, 3).is_none());
        assert_eq!(find_course_by_id(&courses, 5).unwrap().name, "ε. Backup");
        assert_eq!(find_course_by_id(&courses, 5).unwrap().instructors.len(), 0);
        assert_eq!(find_course_by_id(&courses, 1).unwrap().num_min, 2);
        assert_eq!(find_course_by_id(&courses, 1).unwrap().num_max, 10);
        assert_eq!(find_course_by_id(&courses, 1).unwrap().instructors.len(), 1);
        assert_eq!(
            find_course_by_id(&courses, 1).unwrap().instructors[0],
            find_participant_by_id(&participants, 2).unwrap().index
        );
        for c in courses.iter() {
            assert_eq!(
                c.room_offset, 0.0,
                "room_offset of course {} (dbid {}) is not 0.0",
                c.index, c.dbid
            );
            assert_eq!(
                c.fixed_course, false,
                "course {} (dbid {}) is fixed",
                c.index, c.dbid
            );
        }

        // Check participants
        assert_eq!(participants.len(), 5);
        assert_eq!(
            find_participant_by_id(&participants, 2).unwrap().name,
            "Emilia E. Eventis"
        );
        assert_eq!(
            find_participant_by_id(&participants, 2).unwrap().choices,
            vec![
                Choice {
                    course_index: find_course_by_id(&courses, 4).unwrap().index,
                    penalty: 0
                },
                Choice {
                    course_index: find_course_by_id(&courses, 2).unwrap().index,
                    penalty: 1
                }
            ]
        );

        // Check import_ambience
        assert_eq!(import_ambience.event_id, 1);
        assert_eq!(import_ambience.track_id, 3);
    }

    #[test]
    fn parse_testaka_other_tracks() {
        let data = include_bytes!("test_ressources/TestAka_partial_export_event.json");

        // Check that only participants are parsed (no not_applied, applied, waitlist, guest,
        // cancelled or rejected registration parts)
        // Morgenkreis
        let (participants, courses, _import_ambience) =
            super::read(&data[..], Some(1), false, false, None, None).unwrap();
        super::super::assert_data_consitency(&participants, &courses);
        assert_eq!(courses.len(), 5);
        assert_eq!(participants.len(), 2);
        assert!(find_participant_by_id(&participants, 3).is_some());

        // Kaffee
        let (participants, courses, _import_ambience) =
            super::read(&data[..], Some(2), false, false, None, None).unwrap();
        super::super::assert_data_consitency(&participants, &courses);
        assert_eq!(courses.len(), 5);
        assert_eq!(participants.len(), 2);
        assert!(find_participant_by_id(&participants, 3).is_some());
    }

    #[test]
    fn test_no_track_error() {
        let data = include_bytes!("test_ressources/TestAka_partial_export_event.json");

        // Check that only participants are parsed (no not_applied, applied, waitlist, guest,
        // cancelled or rejected registration parts)
        // Morgenkreis
        let result = super::read(&data[..], None, false, false, None, None);
        assert!(result.is_err());
        assert!(result.err().unwrap().find("Kaffeekränzchen").is_some());
    }

    #[test]
    fn test_single_track_event() {
        let data = include_bytes!("test_ressources/cyta_partial_export_event.json");

        let (participants, courses, _import_ambience) =
            super::read(&data[..], None, false, false, None, None).unwrap();
        super::super::assert_data_consitency(&participants, &courses);
        println!(
            "{:?}",
            participants
                .iter()
                .map(|p| &p.name)
                .collect::<Vec<&String>>()
        );
        assert_eq!(courses.len(), 3);
        // Garcia has no choices, so we only read 2 of 3 participants
        assert_eq!(participants.len(), 2);
    }

    #[test]
    fn test_ignore_assigned() {
        let data = include_bytes!("test_ressources/TestAka_partial_export_event.json");
        let (participants, courses, _import_ambience) =
            super::read(&data[..], Some(3), false, true, None, None).unwrap();
        super::super::assert_data_consitency(&participants, &courses);

        assert_eq!(courses.len(), 5);
        // Akira, Emilia and Inga are assigned to course 'α. Heldentum' (id=1), so it shall be fixed
        assert_eq!(find_course_by_id(&courses, 1).unwrap().fixed_course, true);
        assert_eq!(find_course_by_id(&courses, 1).unwrap().room_offset, 3.0);
        assert_eq!(find_course_by_id(&courses, 4).unwrap().fixed_course, false);
        assert_eq!(find_course_by_id(&courses, 4).unwrap().room_offset, 0.0);

        assert_eq!(participants.len(), 2);
        assert!(find_participant_by_id(&participants, 2).is_none());
        assert!(find_participant_by_id(&participants, 4).is_none());
    }

    #[test]
    fn test_course_room_factor_fields() {
        use assert_float_eq::*;

        let data = include_bytes!("test_ressources/TestAka_partial_export_event.json");

        // Modify JSON to insert some fields for courses
        let mut json_data = serde_json::from_reader::<&[u8], serde_json::Value>(&data[..]).unwrap();
        let c1_fields = json_data
            .as_object_mut()
            .unwrap()
            .get_mut("courses")
            .unwrap()
            .get_mut("1")
            .unwrap()
            .get_mut("fields")
            .unwrap()
            .as_object_mut()
            .unwrap();
        c1_fields.insert("my_offset_field".into(), serde_json::json!(2.0));
        c1_fields.insert("my_factor_field".into(), serde_json::json!(1.3));
        let c4_fields = json_data
            .as_object_mut()
            .unwrap()
            .get_mut("courses")
            .unwrap()
            .get_mut("4")
            .unwrap()
            .get_mut("fields")
            .unwrap()
            .as_object_mut()
            .unwrap();
        c4_fields.insert("my_offset_field".into(), serde_json::Value::Null);
        c4_fields.insert("my_factor_field".into(), serde_json::json!(0.5));
        let c13_fields = json_data
            .as_object_mut()
            .unwrap()
            .get_mut("courses")
            .unwrap()
            .get_mut("13")
            .unwrap()
            .get_mut("fields")
            .unwrap()
            .as_object_mut()
            .unwrap();
        c13_fields.insert("my_offset_field".into(), serde_json::json!(1.5));
        let modified_data = serde_json::to_vec(&json_data).unwrap();

        let (participants, courses, _import_ambience) = super::read(
            &modified_data[..],
            Some(3),
            false,
            true,
            Some("my_factor_field"),
            Some("my_offset_field"),
        )
        .unwrap();
        super::super::assert_data_consitency(&participants, &courses);

        assert_eq!(courses.len(), 5);
        // Akira, Emilia and Inga are assigned to course 'α. Heldentum' (id=1)
        assert_f32_near!(find_course_by_id(&courses, 1).unwrap().room_offset, 5.9); // 2.0 + 3 * 1.3
        assert_f32_near!(find_course_by_id(&courses, 1).unwrap().room_factor, 1.3);
        assert_f32_near!(find_course_by_id(&courses, 4).unwrap().room_offset, 0.0); // default
        assert_f32_near!(find_course_by_id(&courses, 4).unwrap().room_factor, 0.5);
        assert_f32_near!(find_course_by_id(&courses, 13).unwrap().room_offset, 1.5);
        assert_f32_near!(find_course_by_id(&courses, 13).unwrap().room_factor, 1.0);
        // default
    }

    #[test]
    fn test_ignore_cancelled() {
        let data = include_bytes!("test_ressources/TestAka_partial_export_event.json");
        let (participants, courses, _import_ambience) =
            super::read(&data[..], Some(3), true, false, None, None).unwrap();
        super::super::assert_data_consitency(&participants, &courses);

        // Course 'γ. Kurz' (id=3) has not been offered and course 'ε. Backup' (id=5) is cancelled
        // in track 'Sitzung' (id=3)
        assert_eq!(courses.len(), 4);
        assert!(find_course_by_id(&courses, 3).is_none());
        assert!(find_course_by_id(&courses, 5).is_none());
        assert_eq!(participants.len(), 5);
    }

    #[test]
    fn test_write_result() {
        let courses = vec![
            Course {
                index: 0,
                dbid: 1,
                name: String::from("α. Heldentum"),
                num_max: 10 - 1,
                num_min: 3 - 1,
                instructors: vec![2],
                room_factor: 1.0,
                room_offset: 0.0,
                fixed_course: false,
                hidden_participant_names: vec![],
            },
            Course {
                index: 1,
                dbid: 2,
                name: String::from("β. Kabarett"),
                num_max: 20,
                num_min: 10,
                instructors: vec![4],
                room_factor: 1.0,
                room_offset: 0.0,
                fixed_course: false,
                hidden_participant_names: vec![],
            },
            Course {
                index: 2,
                dbid: 4,
                name: String::from("δ. Lang"),
                num_max: 25,
                num_min: 0,
                instructors: vec![2],
                room_factor: 1.0,
                room_offset: 0.0,
                fixed_course: false,
                hidden_participant_names: vec![],
            },
            Course {
                index: 3,
                dbid: 5,
                name: String::from("ε. Backup"),
                num_max: 25,
                num_min: 0,
                instructors: vec![2],
                room_factor: 1.0,
                room_offset: 0.0,
                fixed_course: false,
                hidden_participant_names: vec![],
            },
        ];
        let participants = vec![
            Participant {
                index: 0,
                dbid: 1,
                name: String::from("Anton Armin A. Administrator"),
                choices: choices_from_list(&[0, 2]),
            },
            Participant {
                index: 1,
                dbid: 2,
                name: String::from("Emilia E. Eventis"),
                choices: choices_from_list(&[2, 1]),
            },
            Participant {
                index: 2,
                dbid: 3,
                name: String::from("Garcia G. Generalis"),
                choices: choices_from_list(&[1, 2]),
            },
            Participant {
                index: 3,
                dbid: 4,
                name: String::from("Inga Iota"),
                choices: choices_from_list(&[0, 1]),
            },
            Participant {
                index: 4,
                dbid: 5,
                name: String::from("Backup course instructor"),
                choices: vec![],
            },
        ];
        super::super::assert_data_consitency(&participants, &courses);
        let ambience_data = super::ImportAmbienceData {
            event_id: 1,
            track_id: 3,
        };
        let assignment: Assignment = vec![Some(0), Some(0), Some(2), Some(0), None];

        let mut buffer = Vec::<u8>::new();
        let result = super::write(
            &mut buffer,
            &assignment,
            &participants,
            &courses,
            ambience_data,
        );
        assert!(result.is_ok());

        // Parse buffer as JSON file
        let data: serde_json::Value = serde_json::from_reader(&buffer[..]).unwrap();

        // Check course segments (cancelled courses)
        let courses_data = data["courses"].as_object().unwrap();
        assert_eq!(courses_data.len(), 4);
        check_output_course(courses_data, "1", "3", true);
        check_output_course(courses_data, "2", "3", false);

        let registrations_data = data["registrations"].as_object().unwrap();
        // Backup course instructor (without assignment) should not be written to result
        assert_eq!(registrations_data.len(), 4);
        check_output_registration(registrations_data, "1", "3", 1);
        check_output_registration(registrations_data, "3", "3", 4);
    }

    fn find_course_by_id(courses: &Vec<Course>, dbid: usize) -> Option<&Course> {
        courses.iter().filter(|c| c.dbid == dbid).next()
    }

    fn find_participant_by_id(
        participants: &Vec<Participant>,
        dbid: usize,
    ) -> Option<&Participant> {
        participants.iter().filter(|c| c.dbid == dbid).next()
    }

    /// Helper function for test_write_result() to check a course entry in the resulting json data
    fn check_output_course(
        courses_data: &serde_json::Map<String, serde_json::Value>,
        course_id: &str,
        track_id: &str,
        active: bool,
    ) {
        let course_data = courses_data
            .get(course_id)
            .and_then(|v| v.as_object())
            .unwrap_or_else(|| panic!("Course id {} not found or not an object", course_id));
        assert_eq!(
            course_data.len(),
            1,
            "Course id {} has more than one data entry",
            course_id
        );
        let course_segments = course_data
            .get("segments")
            .and_then(|v| v.as_object())
            .unwrap_or_else(|| panic!("Course id {} has no 'segments' entry", course_id));
        assert_eq!(
            course_segments.len(),
            1,
            "Course id {} has more than one segment defined",
            course_id
        );
        assert_eq!(
            course_segments
                .get(track_id)
                .and_then(|v| v.as_bool())
                .unwrap_or_else(|| panic!(
                    "Course id {} has no segment id {} or it is not bool",
                    course_id, track_id
                )),
            active,
            "Course id {} segment has wrong active state",
            course_id
        );
    }

    /// Helper function for test_write_result() to check a registration entry in the resulting json
    /// data
    fn check_output_registration(
        registrations_data: &serde_json::Map<String, serde_json::Value>,
        reg_id: &str,
        track_id: &str,
        course_id: u64,
    ) {
        let reg_data = registrations_data
            .get(reg_id)
            .and_then(|v| v.as_object())
            .unwrap_or_else(|| panic!("Registration id {} not found or not an object", reg_id));
        assert_eq!(
            reg_data.len(),
            1,
            "Registration id {} has more than one data entry",
            reg_id
        );
        let reg_tracks = reg_data
            .get("tracks")
            .and_then(|v| v.as_object())
            .unwrap_or_else(|| panic!("Course id {} has no 'tracks' entry", reg_id));
        assert_eq!(
            reg_tracks.len(),
            1,
            "Registration id {} has more than one track defined",
            reg_id
        );
        let reg_track = reg_tracks
            .get(track_id)
            .and_then(|v| v.as_object())
            .unwrap_or_else(|| {
                panic!(
                    "Registration id {} has no track id {} or it is not an object",
                    reg_id, track_id
                )
            });
        assert_eq!(
            reg_track
                .get("course_id")
                .and_then(|v| v.as_u64())
                .unwrap_or_else(|| panic!(
                    "Registration id {} has no 'course_id' entry or it is not an uint",
                    reg_id
                )),
            course_id,
            "Registration id {} has a wrong course assignment",
            reg_id
        );
    }
}
