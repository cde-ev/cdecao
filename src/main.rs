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

use cdecao::caobab;
use std::fs::File;
use std::sync::Arc;

use log::{debug, error, info, warn};

fn main() {
    // Setup logging & parse command line arguments
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    info!(
        "This is the CdE Course Assignment Optimizer (cdecao), version {}",
        option_env!("CARGO_PKG_VERSION").unwrap_or("unknown")
    );
    let args = parse_cli_args();

    if args.get_one::<String>("OUTPUT").is_none() && !args.get_flag("print") {
        warn!(
            "No OUTPUT file and no --print option given. Assignment will not be exported anywhere."
        );
    }

    // Parse rooms list
    let rooms = args.get_one("rooms").map(|rooms_raw: &String| {
        rooms_raw
            .split(',')
            .map(|r| r.parse::<usize>())
            .collect::<Result<Vec<usize>, std::num::ParseIntError>>()
            .unwrap_or_else(|e| {
                error!("Could not parse room sizes: {}", e);
                std::process::exit(exitcode::DATAERR)
            })
    });

    // Open input file
    let inpath: &String = args.get_one("INPUT").unwrap();
    debug!("Opening input file {} ...", inpath);
    let file = std::fs::File::open(inpath).unwrap_or_else(|e| {
        error!("Could not open input file {}: {}", inpath, e);
        std::process::exit(exitcode::NOINPUT)
    });
    // Read input file
    let (participants, courses, import_ambience) = if args.get_flag("cde") {
        // --cde file format
        let track_id: Option<u64> = args.get_one("track").map(|t: &String| {
            t.parse().unwrap_or_else(|e| {
                error!("Could not parse track id: {}", e);
                std::process::exit(exitcode::DATAERR)
            })
        });
        cdecao::io::cdedb::read(
            file,
            track_id,
            args.get_flag("ignore_cancelled"),
            args.get_flag("ignore_assigned"),
            args.get_one::<String>("room_factor_field").map(|x| &**x),
            args.get_one::<String>("room_offset_field").map(|x| &**x),
        )
        .map(|(p, c, a)| (p, c, Some(a)))
    } else {
        // simple file format
        cdecao::io::simple::read(file).map(|(p, c)| (p, c, None))
    }
    .unwrap_or_else(|e| {
        error!("Could not read input file: {}", e);
        std::process::exit(exitcode::DATAERR)
    });

    // In debug build: Check consistency of imported data
    if cfg!(debug_assertions) {
        cdecao::io::assert_data_consitency(&participants, &courses);
    }

    info!(
        "Found {} courses and {} participants for course assignment.",
        courses.len(),
        participants.len()
    );

    debug!("Courses:\n{}", cdecao::io::debug_list_of_courses(&courses));

    if participants.is_empty() {
        error!("Calculating course assignments is only possible with 1 or more participants.");
        std::process::exit(exitcode::DATAERR);
    }

    // Execute assignment algorithm
    let courses = Arc::new(courses);
    let participants = Arc::new(participants);
    let (result, statistics) = caobab::solve(
        courses.clone(),
        participants.clone(),
        rooms.as_ref(),
        args.get_flag("report_no_solution"),
        *args
            .get_one("num_threads")
            .unwrap_or(&(num_cpus::get() as u32)),
    );
    info!("Finished solving course assignment. {}", statistics);

    if let Some((assignment, score)) = result {
        info!("Solution found with score {}.", score);
        info!(
            "(Higher is better. Perfect fit would be {}.)",
            cdecao::caobab::solution_score::theoretical_max_score(&participants, &courses)
        );
        info!(
            "Solution quality (lower is better, 0.0 is perfect): {}.",
            cdecao::caobab::solution_score::solution_quality(score, &participants)
        );
        if let Some(external_assignment_quality_data) = import_ambience
            .as_ref()
            .and_then(|a| a.external_assignment_quality_info.as_ref())
        {
            info!(
                "Overall Solution quality (including already assigned): {}.",
                cdecao::caobab::solution_score::combined_quality(
                    score,
                    &participants,
                    external_assignment_quality_data
                )
            );
        }
        if let Some(outpath) = args.get_one::<String>("OUTPUT") {
            debug!("Opening output file {} ...", outpath);
            match File::create(outpath) {
                Err(e) => error!("Could not open output file {}: {}.", outpath, e),
                Ok(file) => {
                    let res = if args.get_flag("cde") {
                        cdecao::io::cdedb::write(
                            file,
                            &assignment,
                            &participants,
                            &courses,
                            import_ambience.unwrap(),
                        )
                    } else {
                        cdecao::io::simple::write(file, &assignment)
                    };
                    match res {
                        Ok(_) => debug!("Assignment written to {}.", outpath),
                        Err(e) => error!("Could not write assignment to {}: {}.", outpath, e),
                    }
                }
            }
        }

        if args.get_flag("print") {
            print!(
                "The assignment is:\n{}",
                cdecao::io::format_assignment(&assignment, &courses, &participants)
            );
        }
    } else {
        warn!("No feasible solution found.");
        std::process::exit(1);
    }
}

/// Helper function to construct and execute parser for command line options
fn parse_cli_args() -> clap::ArgMatches {
    clap::command!()
        .arg(
            clap::Arg::new("cde")
                .short('c')
                .long("cde")
                .help("Use CdE Datenbank format for input and output files")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("track")
                .short('t')
                .long("track")
                .help(
                    "Specify CdE-Datenbank id of the course track to assign courses in. Only \
                     useful in combination with --cde input data format.",
                )
                .value_name("TRACK_ID"),
        )
        .arg(
            clap::Arg::new("ignore_cancelled")
                .short('i')
                .long("ignore-cancelled")
                .help(
                    "Ignore already cancelled courses. Otherwise, they are considered for \
                     assignment and might be un-cancelled. Only possible with --cde data \
                     format.",
                )
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("ignore_assigned")
                .short('j')
                .long("ignore-assigned")
                .help(
                    "Ignore already assigned participants. Otherwise all participants are \
                     considered for re-assigned and course assignments are overwritten. Only \
                     possible with --cde data format. If present, courses with assigned \
                     participants will not be cancelled. Attention: This might impair the \
                     solution's quality or even make the problem unsolvable.",
                )
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("room_factor_field")
                .long("room-factor-field")
                .value_name("FIELD_NAME")
                .help(
                    "The name of a course-associated data field from the CdE Datenbank, which \
                     stores a fixed offset to be added to the course size when comparing the \
                     course size with the awailable rooms. Only useful for the --cde data format \
                     and with --rooms given. If not present, the default offset of 0 is used for \
                     all courses.",
                ),
        )
        .arg(
            clap::Arg::new("room_offset_field")
                .long("room-offset-field")
                .value_name("FIELD_NAME")
                .help(
                    "The name of a course-associated data field from the CdE Datenbank, which \
                     stores a scaling factor to be multiplied with the course size (before adding \
                     the offset) when comparing the course size with the awailable rooms. Only \
                     useful for the --cde data format and with --rooms given. If not present, the \
                     default factor of 1.0 is used for all courses.",
                ),
        )
        .arg(
            clap::Arg::new("report_no_solution")
                .long("report-no-solution")
                .help(
                    "Log some unsolvable Branch-and-Bound nodes with INFO log level. This will \
                    be a great help with debugging unsolvable course assignement problems.",
                )
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("rooms")
                .short('r')
                .long("rooms")
                .help("Comma-separated list of available course room sizes, e.g. 15,10,10,8")
                .value_name("ROOMS"),
        )
        .arg(
            clap::Arg::new("num_threads")
                .long("num-threads")
                .help(
                    "Number of worker threads to spawn. Defaults to number of detected CPU cores.",
                )
                .value_name("THREADS")
                .value_parser(clap::value_parser!(u32)),
        )
        .arg(
            clap::Arg::new("print")
                .short('p')
                .long("print")
                .help("Print the caluclated course assignment to stdout in a human readable format")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("INPUT")
                .help("Sets the input file to use")
                .required(true)
                .index(1),
        )
        .arg(
            clap::Arg::new("OUTPUT")
                .help("Sets the output file to use")
                .index(2),
        )
        .get_matches()
}
