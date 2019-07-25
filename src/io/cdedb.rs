//! IO functionality for use of this program with the CdE Datenbank export and import file formats.

use crate::{Course, Participant};
use std::collections::{HashMap, HashSet};

/// Read course and participant data from an JSON event export of the CdE Datenbank
///
/// This function takes a Reader (e.g. an open filehandle), reads its contents and interprets them
/// as a full event export from the CdE Datenbank 2. The export file format is the same as used for
/// initializing an offline deployed instances of the CdEDB. At the time of writing, this is the
/// only fully implemented export format of the CdEDB, but unfortunately it's quite hard to parse.
///
/// Existing course assignments and cancelled courses are ignored.
/// Minimum and maximum participant numbers of courses are interpreted as total number of attendees,
/// **including** course instructors.
/// If no maximum size is given for a course, we assume num_max = 25 (incl. instructors).
/// If no minimum size is given for a course, we assume num_min = 0 (excl. instructors).
///
/// We take info from the following database tables (= JSON keys)
///   * event.course_tracks – to get the relevant track and part ids
///   * event.courses
///   * event.registrations
///   * event.registration_parts – to check state of the registrations in the relevant part
///   * event.registration_tracks – to find course instructors
///   * event.course_choices
// TODO adapt for new export/import format as soon as it is released
pub fn read<R: std::io::Read>(reader: R) -> Result<(Vec<Participant>, Vec<Course>), String> {
    let data: serde_json::Value = serde_json::from_reader(reader).map_err(|err| err.to_string())?;

    // Get part and track id
    let tracks = data["event.course_tracks"].as_object().ok_or(
        "No 'event.course_tracks' object found in data. Is this a correct CdEdb export file?",
    )?;
    // TODO allow selecting track (by id)?
    if tracks.len() != 1 {
        return Err(
            "This program is only applicable for events with exactly exactly 1 course track"
                .to_owned(),
        );
    }
    let track_data = tracks.values().next().unwrap();
    let part_id = track_data["part_id"]
        .as_u64()
        .ok_or("No part_id found for the track_id")?;

    // Parse courses
    // TODO filter courses by active tracks
    let mut courses = Vec::new();
    let courses_data = data["event.courses"]
        .as_object()
        .ok_or("No 'event.courses' object found in data.".to_owned())?;
    for (i, course_data) in courses_data.values().enumerate() {
        let course_id = course_data["id"]
            .as_u64()
            .ok_or("Missing 'course_id' in course record")?;
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
        });
    }
    let mut courses_by_id: HashMap<u64, &mut crate::Course> =
        courses.iter_mut().map(|r| (r.dbid as u64, r)).collect();

    // Parse courses choices to filter out registrations without choices
    let mut regs_with_choices = HashSet::new();
    let mut course_choices_data: Vec<&serde_json::Value> = data["event.course_choices"]
        .as_object()
        .ok_or("No 'event.course_choices' object found in data.")?
        .values()
        .collect();
    course_choices_data.sort_by_key(|cd| cd["rank"].as_u64().unwrap_or(0));
    for choice_data in course_choices_data.iter() {
        let reg_id = choice_data["registration_id"]
            .as_u64()
            .ok_or("Missing 'reg_id' in course choice record")?;
        regs_with_choices.insert(reg_id);
    }

    // Parse Registration parts to decide on registration status later
    let mut active_regs = HashSet::new();
    let reg_parts_data = data["event.registration_parts"]
        .as_object()
        .ok_or("No 'event.registration_parts' object found in data.")?;
    for rp_data in reg_parts_data.values() {
        if rp_data["part_id"]
            .as_u64()
            .ok_or("Missing 'part_id' in registration_part record")?
            == part_id
            && rp_data["status"]
                .as_u64()
                .ok_or("Missing 'status' in registration_part record")?
                == 2
        {
            active_regs.insert(
                rp_data["registration_id"]
                    .as_u64()
                    .ok_or("Missing 'reg_id' in registration_part record")?,
            );
        }
    }

    // Parse Registrations
    let mut registrations = Vec::new();
    let registrations_data = data["event.registrations"]
        .as_object()
        .ok_or("No 'event.registrations' object found in data.".to_owned())?;
    let mut i = 0;
    for reg_data in registrations_data.values() {
        let reg_id = reg_data["id"]
            .as_u64()
            .ok_or("Missing 'reg_id' in registration record")?;
        if !active_regs.contains(&reg_id) || !regs_with_choices.contains(&reg_id) {
            continue;
        }
        let persona_id = reg_data["persona_id"]
            .as_u64()
            .ok_or(format!("Missing 'persona_id' in registration {}", reg_id))?;
        let persona_data = data["core.personas"][format!("{}", persona_id)]
            .as_object()
            .ok_or("No matching persona found")?;
        let reg_name = format!(
            "{} {}",
            persona_data["given_names"]
                .as_str()
                .ok_or(format!("No 'given_name' found for persona {}", persona_id))?,
            persona_data["family_name"]
                .as_str()
                .ok_or(format!("No 'family_name' found for persona {}", persona_id))?
        );
        registrations.push(crate::Participant {
            index: i,
            dbid: reg_id as usize,
            name: reg_name,
            choices: Vec::new(),
        });
        i += 1;
    }
    let mut regs_by_id: HashMap<u64, &mut crate::Participant> = registrations
        .iter_mut()
        .map(|r| (r.dbid as u64, r))
        .collect();

    // Add instructors
    let reg_track_data = data["event.registration_tracks"]
        .as_object()
        .ok_or("No 'event.registration_tracks' object found in data.")?;
    for rt_data in reg_track_data.values() {
        let opt_registration = rt_data["registration_id"]
            .as_u64()
            .as_ref()
            .and_then(|reg_id| regs_by_id.get(reg_id));
        if let Some(registration) = opt_registration {
            if let Some(instructed_course) = rt_data["course_instructor"].as_u64() {
                courses_by_id
                    .get_mut(&instructed_course)
                    .ok_or(format!("Course with dbid {} not found", instructed_course))?
                    .instructors
                    .push(registration.index);
            }
        }
    }

    // Insert course choices
    for choice_data in course_choices_data {
        let opt_registration = choice_data["registration_id"]
            .as_u64()
            .as_ref()
            .and_then(|reg_id| regs_by_id.get_mut(reg_id));
        let opt_course = choice_data["course_id"]
            .as_u64()
            .as_ref()
            .and_then(|c_id| courses_by_id.get_mut(&c_id));
        if let Some(registration) = opt_registration {
            if let Some(course) = opt_course {
                registration.choices.push(course.index);
            }
        }
    }

    // Subtract course instructors from course participant bounds
    // (course participant bounds in the CdEDB include instructors)
    for mut course in courses.iter_mut() {
        course.num_min = if course.instructors.len() > course.num_min {
            0
        } else {
            course.num_min - course.instructors.len()
        };
        course.num_max = if course.instructors.len() > course.num_max {
            0
        } else {
            course.num_max - course.instructors.len()
        };
    }

    Ok((registrations, courses))
}
