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

use crate::{Assignment, Course, Participant};
use std::collections::HashMap;

use chrono::{SecondsFormat, Utc};
use serde_json::json;
use std::cmp::max;

use log::{info, warn};

const MINIMUM_EXPORT_VERSION: (u64, u64) = (7, 0);
const MAXIMUM_EXPORT_VERSION: (u64, u64) = (15, std::u64::MAX);

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

    // Check export type and version
    let export_kind = data
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or("No 'kind' field found in data. Is this a correct CdEdb export file?")?;
    if export_kind != "partial" {
        return Err("The given JSON file is no 'Partial Export' of the CdE Datenbank".to_owned());
    }
    let export_version =
        if let Some(version_tag) = data.get("EVENT_SCHEMA_VERSION") {
            version_tag.as_array()
                .ok_or("'EVENT_SCHEMA_VERSION' is not an array!")
                .and_then(|v|
                    if v.len() == 2
                    {Ok(v)}
                    else {Err("'EVENT_SCHEMA_VERSION' does not have 2 entries.")})
                .and_then(|v| v.iter().map(
                    |x| x.as_u64()
                        .ok_or("Entry of 'EVENT_SCHEMA_VERSION' is not an u64 value."))
                    .collect::<Result<Vec<u64>, &str>>())
                .and_then(|v| Ok((v[0], v[1])))
        } else if let Some(version_tag) = data.get("CDEDB_EXPORT_EVENT_VERSION") {
            // Support for old export schema version field
            version_tag.as_u64()
                .ok_or("'CDEDB_EXPORT_EVENT_VERSION' is not an u64 value")
                .and_then(|v| Ok((v, 0)))
        } else {
            Err("No 'EVENT_SCHEMA_VERSION' field found in data. Is this a correct CdEdb \
            export file?")
        }?;
    if export_version < MINIMUM_EXPORT_VERSION || export_version > MAXIMUM_EXPORT_VERSION {
        return Err(format!(
            "The given CdE Datenbank Export is not within the supported version range \
            [{}.{},{}.{}]",
            MINIMUM_EXPORT_VERSION.0, MINIMUM_EXPORT_VERSION.1, MAXIMUM_EXPORT_VERSION.0,
            MAXIMUM_EXPORT_VERSION.1));
    }

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
    let mut skipped_course_ids = Vec::new();  // Used to ignore KeyErrors for those later
    let courses_data = data
        .get("courses")
        .and_then(|v| v.as_object())
        .ok_or("No 'courses' object found in data.".to_owned())?;
    let mut i = 0;
    for (course_id, course_data) in courses_data.iter() {
        let course_id: usize = course_id
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        let course_segments_data = course_data
            .get("segments")
            .and_then(|v| v.as_object())
            .ok_or(format!(
                "No 'segments' object found for course {}",
                course_id
            ))?;
        // Skip courses without segment in the relevant track
        if !course_segments_data.contains_key(&format!("{}", track_id)) {
            continue;
        }
        // Skip already cancelled courses (if wanted). Only add their id to `skipped_course_ids`
        if ignore_inactive_courses
            && !(course_segments_data
                .get(&format!("{}", track_id))
                .and_then(|v| v.as_bool())
                .ok_or(format!("Segment of course {} is not a boolean.", course_id))?)
        {
            skipped_course_ids.push(course_id);
            continue;
        }

        let course_name = format!(
            "{}. {}",
            course_data
                .get("nr")
                .and_then(|v| v.as_str())
                .ok_or(format!("No 'nr' found for course {}", course_id))?,
            course_data
                .get("shortname")
                .and_then(|v| v.as_str())
                .ok_or(format!("No 'shortname' found for course {}", course_id))?
        );

        // Analyze fields to extract room_factor and room_offset
        let fields = course_data
            .get("fields")
            .and_then(|v| v.as_object())
            .ok_or(format!("No 'fields' found for course {}", course_id))?;
        let room_factor = if let Some(field_name) = room_factor_field {
            match fields.get(field_name).and_then(|v| v.as_f64()) {
                Some(v) => v,
                None => {
                    warn!("No numeric field '{}' as room_factor field found in course {}. Using the default value 1.0.", field_name, course_name);
                    1.0
                }
            }
        } else {
            1.0
        };
        let room_offset = if let Some(field_name) = room_offset_field {
            match fields.get(field_name).and_then(|v| v.as_u64()) {
                Some(v) => v,
                None => {
                    warn!("No integer field '{}' as room_offset field found in course {}. Using the default value 0.", field_name, course_name);
                    0
                }
            }
        } else {
            0
        };

        courses.push(crate::Course {
            index: i,
            dbid: course_id as usize,
            name: course_name,
            num_max: course_data
                .get("max_size")
                .and_then(|v| v.as_u64())
                .unwrap_or(25) as usize,
            num_min: course_data
                .get("min_size")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize,
            instructors: Vec::new(),
            room_factor: room_factor as f32,
            room_offset: room_offset as usize,
            fixed_course: false,
        });
        i += 1;
    }
    // Store, how many instructors attendees are already set for each course (only relevant if
    // ignore_assigned == true). The vector holds a tuple
    // (num_hidden_instructors, num_hidden_attendees) for each course in the same order as the
    // `courses` vector.
    let mut invisible_course_participants = vec![(0usize, 0usize); courses.len()];
    let mut courses_by_id: HashMap<u64, &mut crate::Course> =
        courses.iter_mut().map(|r| (r.dbid as u64, r)).collect();

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
        // Check registration status to skip irrelevant registrations
        let rp_data = reg_data
            .get("parts")
            .and_then(|v| v.as_object())
            .ok_or(format!("No 'parts' found in registration {}", reg_id))?
            .get(&format!("{}", part_id))
            .and_then(|v| v.as_object());
        if let None = rp_data {
            continue;
        }
        let rp_data = rp_data.unwrap();
        if rp_data
            .get("status")
            .and_then(|v| v.as_i64())
            .ok_or(format!(
                "Missing 'status' in registration_part record of reg {}",
                reg_id
            ))?
            != 2
        {
            continue;
        }

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

        // Get registration track data
        let rt_data = reg_data
            .get("tracks")
            .and_then(|v| v.as_object())
            .ok_or(format!("No 'tracks' found in registration {}", reg_id))?
            .get(&format!("{}", track_id))
            .and_then(|v| v.as_object())
            .ok_or(format!(
                "Registration track data not present for registration {}",
                reg_id
            ))?;

        // Skip already assigned participants (if wanted)
        if ignore_assigned {
            // Check if course_id is an integer and get this integer
            if let Some(course_id) = rt_data.get("course_id").and_then(|v| v.as_u64()) {
                // Add participant to the invisible_course_participants of this course ...
                if let Some(course) = courses_by_id.get(&course_id) {
                    match rt_data.get("course_instructor").and_then(|v| v.as_u64()) {
                        // In case, they are (invisible) instructor of the course ...
                        Some(c) if c == course_id => {
                            invisible_course_participants[course.index].0 += 1;
                        },
                        // In case, they are (invisible) attendee of the course ...
                        _ => {
                            invisible_course_participants[course.index].1 += 1;
                        }
                    }
                }
                continue;
            }
        }

        // Parse course choices
        let choices_data = rt_data
            .get("choices")
            .and_then(|v| v.as_array())
            .ok_or(format!(
                "No 'choices' found in registration {}'s track data",
                reg_id
            ))?;

        let mut choices = Vec::<usize>::new();
        for v in choices_data {
            let course_id = v.as_u64().ok_or("Course choice is no integer.")?;
            if let Some(course) = courses_by_id.get(&course_id) {
                choices.push(course.index);
            } else if !skipped_course_ids.contains(&(course_id as usize)) {
                return Err(format!(
                    "Course choice {} of registration {} does not exist.", course_id, reg_id));
            }
        }

        // Filter out registrations without choices
        if choices.len() == 0 {
            if choices_data.len() > 0 {
                info!("Ignoring participant {} (id {}), who only chose cancelled courses.",
                      reg_name, reg_id);
            }
            continue;
        }

        // Add course instructors to courses
        if let Some(instructed_course) = rt_data
                .get("course_instructor")
                .and_then(|v| v.as_u64()) {
            if let Some(course) = courses_by_id.get_mut(&instructed_course) {
                course.instructors.push(i);
            } else if !skipped_course_ids.contains(&(instructed_course as usize)) {
                return Err(format!(
                    "Instructed course {} of registration {} does not exist.",
                    instructed_course,
                    reg_id));
            }
        }

        registrations.push(crate::Participant {
            index: i,
            dbid: reg_id as usize,
            name: reg_name,
            choices,
        });
        i += 1;
    }

    // Subtract invisible course attendees from course participant bounds
    // Prevent courses with invisible course participants from being cancelled and add
    // invisible course participants to room_offset
    for mut course in courses.iter_mut() {
        let invisible_course_attendees = invisible_course_participants[course.index].1;
        let total_invisible_course_participants = invisible_course_participants[course.index].0 + invisible_course_participants[course.index].1;
        course.num_min = if invisible_course_attendees > course.num_min
        {
            0
        } else {
            course.num_min - invisible_course_attendees
        };
        course.num_max = if invisible_course_attendees > course.num_max
        {
            0
        } else {
            course.num_max - invisible_course_attendees
        };
        course.fixed_course = total_invisible_course_participants != 0;
        course.room_offset += total_invisible_course_participants;
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

/// Write the calculated course assignment as a CdE Datenbank partial import JSON string to a Writer
/// (e.g. an output file).
pub fn write<W: std::io::Write>(
    writer: W,
    assignment: &Assignment,
    participants: &Vec<Participant>,
    courses: &Vec<Course>,
    ambience_data: ImportAmbienceData,
) -> Result<(), String> {
    // Calculate course sizes
    let mut course_size = vec![0usize; courses.len()];
    for (_p, c) in assignment.iter().enumerate() {
        course_size[*c] += 1;
    }

    let registrations_json = assignment
        .iter()
        .enumerate()
        .map(|(pid, cid)| {
            (
                format!("{}", participants[pid].dbid),
                json!({
                "tracks": {
                    format!("{}", ambience_data.track_id): {
                        "course_id": courses[*cid].dbid
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
                    format!("{}", ambience_data.track_id): *size > 0
                }}),
            )
        })
        .collect::<serde_json::Map<String, serde_json::Value>>();

    let data = json!({
        "EVENT_SCHEMA_VERSION": [15, 4],
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
                    if let Some(_) = result {
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
    return Ok(result);
}

#[cfg(test)]
mod tests {
    use crate::{Assignment, Course, Participant};

    #[test]
    fn parse_testaka_sitzung() {
        let data = include_bytes!("test_ressources/TestAka_partial_export_event.json");
        let (participants, courses, import_ambience) =
            super::read(&data[..], Some(3), false, false, None, None).unwrap();

        super::super::assert_data_consitency(&participants, &courses);
        // Check courses
        // Course "γ. Kurz" is cancelled in this track, thus it should not exist in the parsed data
        assert_eq!(courses.len(), 4);
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
                c.room_offset, 0,
                "room_offset of course {} (dbid {}) is not 0",
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
                find_course_by_id(&courses, 4).unwrap().index,
                find_course_by_id(&courses, 2).unwrap().index
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
        assert_eq!(courses.len(), 4);
        assert_eq!(participants.len(), 2);
        assert!(find_participant_by_id(&participants, 3).is_some());

        // Kaffee
        let (participants, courses, _import_ambience) =
            super::read(&data[..], Some(2), false, false, None, None).unwrap();
        super::super::assert_data_consitency(&participants, &courses);
        assert_eq!(courses.len(), 4);
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
    fn test_ignore_assigned() {
        let data = include_bytes!("test_ressources/TestAka_partial_export_event.json");
        let (participants, courses, _import_ambience) =
            super::read(&data[..], Some(3), false, true, None, None).unwrap();
        super::super::assert_data_consitency(&participants, &courses);

        assert_eq!(courses.len(), 4);
        assert_eq!(find_course_by_id(&courses, 1).unwrap().fixed_course, true);
        assert_eq!(find_course_by_id(&courses, 1).unwrap().room_offset, 3);
        assert_eq!(find_course_by_id(&courses, 4).unwrap().fixed_course, false);
        assert_eq!(find_course_by_id(&courses, 4).unwrap().room_offset, 0);

        assert_eq!(participants.len(), 2);
        assert!(find_participant_by_id(&participants, 2).is_none());
        assert!(find_participant_by_id(&participants, 4).is_none());
    }

    #[test]
    fn test_ignore_cancelled() {
        let data = include_bytes!("test_ressources/TestAka_partial_export_event.json");
        let (participants, courses, _import_ambience) =
            super::read(&data[..], Some(3), true, false, None, None).unwrap();
        super::super::assert_data_consitency(&participants, &courses);

        assert_eq!(courses.len(), 3);
        assert!(find_course_by_id(&courses, 3).is_none());
        assert!(find_course_by_id(&courses, 5).is_none());
        assert_eq!(participants.len(), 5);
    }

    // TODO test parsing single track event

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
                room_offset: 0,
                fixed_course: false,
            },
            Course {
                index: 1,
                dbid: 2,
                name: String::from("β. Kabarett"),
                num_max: 20,
                num_min: 10,
                instructors: vec![],
                room_factor: 1.0,
                room_offset: 0,
                fixed_course: false,
            },
            Course {
                index: 2,
                dbid: 4,
                name: String::from("δ. Lang"),
                num_max: 25,
                num_min: 0,
                instructors: vec![2],
                room_factor: 1.0,
                room_offset: 0,
                fixed_course: false,
            },
            Course {
                index: 3,
                dbid: 5,
                name: String::from("ε. Backup"),
                num_max: 25,
                num_min: 0,
                instructors: vec![2],
                room_factor: 1.0,
                room_offset: 0,
                fixed_course: false,
            },
        ];
        let participants = vec![
            Participant {
                index: 0,
                dbid: 1,
                name: String::from("Anton Armin A. Administrator"),
                choices: vec![0, 2],
            },
            Participant {
                index: 1,
                dbid: 2,
                name: String::from("Emilia E. Eventis"),
                choices: vec![2, 1],
            },
            Participant {
                index: 2,
                dbid: 3,
                name: String::from("Garcia G. Generalis"),
                choices: vec![1, 2],
            },
            Participant {
                index: 3,
                dbid: 4,
                name: String::from("Inga Iota"),
                choices: vec![0, 1],
            },
        ];
        super::super::assert_data_consitency(&participants, &courses);
        let ambience_data = super::ImportAmbienceData {
            event_id: 1,
            track_id: 3,
        };
        let assignment: Assignment = vec![0, 0, 2, 0];

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
