use cdecao::caobab;
use std::fs::File;
use std::sync::Arc;

use log::{debug, error, info, warn};

fn main() {
    // Setup logging & parse command line arguments
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = parse_cli_args();

    if !args.is_present("OUTPUT") && !args.is_present("print") {
        warn!(
            "No OUTPUT file and no --print option given. Assignment will not be exported anywhere."
        );
    }

    // Parse rooms list
    let rooms = args.value_of("rooms").map(|rooms_raw| {
        rooms_raw
            .split(",")
            .map(|r| r.parse::<usize>())
            .collect::<Result<Vec<usize>, std::num::ParseIntError>>()
            .unwrap_or_else(|e| {
                error!("Could not parse room sizes: {}", e);
                std::process::exit(exitcode::DATAERR)
            })
    });

    // Read input file
    let inpath = args.value_of("INPUT").unwrap();
    debug!("Opening input file {} ...", inpath);
    let file = std::fs::File::open(inpath).unwrap_or_else(|e| {
        error!("Could not open input file {}: {}", inpath, e);
        std::process::exit(exitcode::NOINPUT)
    });
    let (participants, courses) = if args.is_present("cde") {
        cdecao::io::cdedb::read(file, None)  // TODO get requested course track from cli
    } else {
        cdecao::io::simple::read(file)
    }
    .unwrap_or_else(|e| {
        error!("Could not read input file: {}", e);
        std::process::exit(exitcode::DATAERR)
    });
    info!(
        "Read {} courses and {} participants.",
        courses.len(),
        participants.len()
    );

    // Execute assignment algorithm
    let courses = Arc::new(courses);
    let participants = Arc::new(participants);
    let (result, statistics) = caobab::solve(courses.clone(), participants.clone(), rooms.as_ref());
    info!("Finished solving course assignment. {}", statistics);

    if let Some((assignment, _)) = result {
        if let Some(outpath) = args.value_of("OUTPUT") {
            debug!("Opening output file {} ...", outpath);
            match File::create(outpath) {
                Err(e) => error!("Could not open output file {}: {}.", outpath, e),
                Ok(file) => {
                    let res = if args.is_present("cde") {
                        // TODO
                        Err("Not implemented".to_owned())
                    } else {
                        cdecao::io::simple::write(file, &assignment, &*participants, &*courses)
                    };
                    match res {
                        Ok(_) => debug!("Assignment written to {}.", outpath),
                        Err(e) => error!("Could not write assignment to {}: {}.", outpath, e),
                    }
                }
            }
        }

        if args.is_present("print") {
            print!(
                "The assignment is:\n{}",
                cdecao::io::format_assignment(&assignment, &*courses, &*participants)
            );
        }
    } else {
        warn!("No feasible solution found.");
    }
}

fn parse_cli_args() -> clap::ArgMatches<'static> {
    clap::App::new("CdE Course Assignment Optimization")
        .version(clap::crate_version!())
        .author(clap::crate_authors!())
        .about(clap::crate_description!())
        .arg(
            clap::Arg::with_name("cde")
                .short("c")
                .long("cde")
                .help("Use CdE Datenbank format for input and output files"),
        )
        .arg(
            clap::Arg::with_name("rooms")
                .short("r")
                .long("rooms")
                .help("Comma-separated list of available course room sizes, e.g. 15,10,10,8")
                .value_name("ROOMS")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("print").short("p").long("print").help(
                "Print the caluclated course assignment to stdout in a human readable format",
            ),
        )
        .arg(
            clap::Arg::with_name("INPUT")
                .help("Sets the input file to use")
                .required(true)
                .index(1),
        )
        .arg(
            clap::Arg::with_name("OUTPUT")
                .help("Sets the output file to use")
                .index(2),
        )
        .get_matches()
}
