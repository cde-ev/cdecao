//! IO functionality for use of this program with the CdE Datenbank export and import file formats.

use crate::{Course, Participant};
use std::collections::{HashMap, HashSet};

// TODO documentation
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
        let course_id = course_data["id"].as_u64().ok_or("No course id found")?;
        let course_name = format!(
            "{}. {}",
            course_data["nr"].as_str().ok_or("No course number found")?,
            course_data["shortname"]
                .as_str()
                .ok_or("No course name found")?
        );
        courses.push(crate::Course {
            index: i,
            dbid: course_id as usize,
            name: course_name,
            num_max: course_data["max_size"]
                .as_u64()
                .unwrap_or(20) as usize,
            num_min: course_data["min_size"]
                .as_u64()
                .unwrap_or(1) as usize,
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
    course_choices_data.sort_by_key(|cd| {cd["rank"].as_u64().unwrap()});
    for choice_data in course_choices_data.iter() {
        let reg_id = choice_data["registration_id"].as_u64().ok_or("No reg_id in choice found")?;
        regs_with_choices.insert(reg_id);
    }

    // Parse Registration parts to decide on registration status later
    let mut active_regs = HashSet::new();
    let reg_parts_data = data["event.registration_parts"]
        .as_object()
        .ok_or("No 'event.registration_parts' object found in data.")?;
    for rp_data in reg_parts_data.values() {
        if rp_data["part_id"].as_u64().ok_or("no part id found")? == part_id
            && rp_data["status"].as_u64().ok_or("no part status found")? == 2
        {
            active_regs.insert(
                rp_data["registration_id"]
                    .as_u64()
                    .ok_or("no part reg_id found")?,
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
        let reg_id = reg_data["id"].as_u64().ok_or("No registration id found")?;
        if !active_regs.contains(&reg_id) || !regs_with_choices.contains(&reg_id) {
            continue;
        }
        let persona_data = data["core.personas"][format!(
            "{}",
            reg_data["persona_id"]
                .as_u64()
                .ok_or("No persona id found")?
        )]
        .as_object()
        .ok_or("No matching persona found")?;
        let reg_name = format!(
            "{} {}",
            persona_data["given_names"]
                .as_str()
                .ok_or("No given name found")?,
            persona_data["family_name"]
                .as_str()
                .ok_or("No given name found")?
        );
        registrations.push(crate::Participant {
            index: i,
            dbid: reg_id as usize,
            name: reg_name,
            choices: Vec::new(),
        });
        i += 1;
    }
    let mut regs_by_id: HashMap<u64, &mut crate::Participant> =
        registrations.iter_mut().map(|r| (r.dbid as u64, r)).collect();

    // Add instructors
    let reg_track_data = data["event.registration_tracks"]
        .as_object()
        .ok_or("No 'event.registration_tracks' object found in data.")?;
    for rt_data in reg_track_data.values() {
        let opt_registration = rt_data["registration_id"].as_u64().as_ref().and_then(|reg_id| regs_by_id.get(reg_id));
        if let Some(registration) = opt_registration {
            if let Some(instructed_course) = rt_data["course_instructor"].as_u64() {
                courses_by_id.get_mut(&instructed_course)
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

    Ok((registrations, courses))
}
