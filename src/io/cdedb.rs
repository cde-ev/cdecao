//! IO functionality for use of this program with the CdE Datenbank export and import file formats.

use crate::{Assignment, Course, Participant};
use std::collections::HashMap;

use chrono::{SecondsFormat, Utc};
use serde_json::json;
use std::cmp::max;

const MINIMUM_EXPORT_VERSION: u64 = 3;
const MAXIMUM_EXPORT_VERSION: u64 = 4;

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
) -> Result<(Vec<Participant>, Vec<Course>, ImportAmbienceData), String> {
    let data: serde_json::Value = serde_json::from_reader(reader).map_err(|err| err.to_string())?;

    // Check export type and version
    let export_kind = data["kind"]
        .as_str()
        .ok_or("No 'kind' field found in data. Is this a correct CdEdb export file?")?;
    if export_kind != "partial" {
        return Err("The given JSON file is no 'Partial Export' of the CdE Datenbank".to_owned());
    }
    let export_version = data["CDEDB_EXPORT_EVENT_VERSION"].as_u64().ok_or(
        "No 'CDEDB_EXPORT_EVENT_VERSION' field found in data. Is this a correct CdEdb export file?",
    )?;
    if export_version < MINIMUM_EXPORT_VERSION || export_version > MAXIMUM_EXPORT_VERSION {
        return Err(format!(
            "The given given CdE Datenbank Export is not within the supported version range [{},{}]",
            MINIMUM_EXPORT_VERSION, MAXIMUM_EXPORT_VERSION));
    }

    // Find part and track ids
    let parts_data = data["event"]
        .as_object()
        .ok_or("No 'event' object found in data.")?["parts"]
        .as_object()
        .ok_or("No 'parts' object found in event.")?;
    let (part_id, track_id) = find_track(parts_data, track)?;

    // Parse courses
    let mut courses = Vec::new();
    let courses_data = data["courses"]
        .as_object()
        .ok_or("No 'courses' object found in data.".to_owned())?;
    let mut i = 0;
    for (course_id, course_data) in courses_data.iter() {
        let course_id: usize = course_id
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        let course_segments_data = course_data["segments"].as_object().ok_or(format!(
            "No 'segments' object found for course {}",
            course_id
        ))?;
        // Skip courses without segment in the relevant track
        if !course_segments_data.contains_key(&format!("{}", track_id)) {
            continue;
        }
        // Skip already cancelled courses (if wanted)
        if ignore_inactive_courses
            && !(course_segments_data[&format!("{}", track_id)]
                .as_bool()
                .ok_or(format!("Segment of course {} is not a boolean.", course_id))?)
        {
            continue;
        }

        let course_name = format!(
            "{}. {}",
            course_data["nr"]
                .as_str()
                .ok_or(format!("No 'nr' found for course {}", course_id))?,
            course_data["shortname"]
                .as_str()
                .ok_or(format!("No 'shortname' found for course {}", course_id))?
        );
        courses.push(crate::Course {
            index: i,
            dbid: course_id as usize,
            name: course_name,
            num_max: course_data["max_size"].as_u64().unwrap_or(25) as usize,
            num_min: course_data["min_size"].as_u64().unwrap_or(0) as usize,
            instructors: Vec::new(),
            room_offset: 0,
            fixed_course: false,
        });
        i += 1;
    }
    // Store, how many participants are already set for each course (only relevant if
    //   ignore_assigned == true)
    let mut invisible_course_attendees = vec![0; courses.len()];
    let mut courses_by_id: HashMap<u64, &mut crate::Course> =
        courses.iter_mut().map(|r| (r.dbid as u64, r)).collect();

    // Parse Registrations
    let mut registrations = Vec::new();
    let registrations_data = data["registrations"]
        .as_object()
        .ok_or("No 'registrations' object found in data.".to_owned())?;
    let mut i = 0;
    for (reg_id, reg_data) in registrations_data {
        let reg_id: u64 = reg_id
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        // Check registration status to skip irrelevant registrations
        let rp_data = reg_data["parts"]
            .as_object()
            .ok_or(format!("No 'parts' found in registration {}", reg_id))?
            [&format!("{}", part_id)]
            .as_object();
        if let None = rp_data {
            continue;
        }
        let rp_data = rp_data.unwrap();
        if rp_data["status"].as_i64().ok_or(format!(
            "Missing 'status' in registration_part record of reg {}",
            reg_id
        ))? != 2
        {
            continue;
        }

        // Parse persona attributes
        let persona_data = reg_data["persona"]
            .as_object()
            .ok_or(format!("Missing 'persona' in registration {}", reg_id))?;
        let reg_name = format!(
            "{} {}",
            persona_data["given_names"]
                .as_str()
                .ok_or(format!("No 'given_name' found for registration {}", reg_id))?,
            persona_data["family_name"].as_str().ok_or(format!(
                "No 'family_name' found for registration {}",
                reg_id
            ))?
        );

        // Get registration track data
        let rt_data = reg_data["tracks"]
            .as_object()
            .ok_or(format!("No 'tracks' found in registration {}", reg_id))?
            [&format!("{}", track_id)]
            .as_object()
            .ok_or(format!(
                "Registration track data not present for registration {}",
                reg_id
            ))?;

        // Skip already assigned participants (if wanted)
        if ignore_assigned {
            // Check if course_id is an integer and get this integer
            if let Some(course_id) = rt_data["course_id"].as_u64() {
                // Add participant to the invisible_course_attendees of this course
                if let Some(course) = courses_by_id.get(&course_id) {
                    invisible_course_attendees[course.index] += 1;
                }
                continue;
            }
        }

        // Parse course chcoices
        let choices_data = rt_data["choices"].as_array().ok_or(format!(
            "No 'choices' found in registration {}'s track data",
            reg_id
        ))?;
        let choices = choices_data
            .iter()
            .map(|v: &serde_json::Value| -> Result<usize, String> {
                let course_id = v.as_u64().ok_or("Course choice is no integer.")?;
                let course = courses_by_id.get(&course_id).ok_or(format!(
                    "Course with dbid {}, choice of reg. {}, not found",
                    course_id, reg_id
                ))?;
                Ok(course.index)
            })
            .collect::<Result<Vec<usize>, String>>()?;

        // Filter out registrations without choices
        if choices.len() == 0 {
            continue;
        }

        // Add course instructors to courses
        if let Some(instructed_course) = rt_data["course_instructor"].as_u64() {
            courses_by_id
                .get_mut(&instructed_course)
                .ok_or(format!(
                    "Course with dbid {}, instructed by reg. {}, not found",
                    instructed_course, reg_id
                ))?
                .instructors
                .push(i);
        }

        registrations.push(crate::Participant {
            index: i,
            dbid: reg_id as usize,
            name: reg_name,
            choices,
        });
        i += 1;
    }

    // Subtract course instructors and invisible_course_attendees from course participant bounds
    // (course participant bounds in the CdEDB include instructors)
    // Prevent courses with invisible_course_attendees from being cancelled and add
    // invisible_course_attendees to room_offset
    for mut course in courses.iter_mut() {
        course.num_min = if course.instructors.len() + invisible_course_attendees[course.index]
            > course.num_min
        {
            0
        } else {
            course.num_min - course.instructors.len() - invisible_course_attendees[course.index]
        };
        course.num_max = if course.instructors.len() + invisible_course_attendees[course.index]
            > course.num_max
        {
            0
        } else {
            course.num_max - course.instructors.len() - invisible_course_attendees[course.index]
        };
        course.fixed_course = invisible_course_attendees[course.index] != 0;
        course.room_offset += invisible_course_attendees[course.index];
    }

    Ok((
        registrations,
        courses,
        ImportAmbienceData {
            event_id: data["id"].as_u64().ok_or("No event 'id' found in data")?,
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
        "CDEDB_EXPORT_EVENT_VERSION": 4,
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
                let tracks_data = part["tracks"]
                    .as_object()
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
                let tracks_data = part["tracks"]
                    .as_object()
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

fn track_summary(
    parts_data: &serde_json::Map<String, serde_json::Value>,
) -> Result<String, String> {
    let mut tracks = Vec::new();
    let mut max_id_len = 0;

    for (_part_id, part) in parts_data {
        let tracks_data = part["tracks"]
            .as_object()
            .ok_or("Missing 'tracks' in event part.")?;
        for (track_id, track) in tracks_data {
            max_id_len = max(max_id_len, track_id.len());
            tracks.push((
                track_id,
                track["title"]
                    .as_str()
                    .ok_or("Missing 'title' in event track.")?,
                track["sortkey"]
                    .as_i64()
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
