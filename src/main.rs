use cdecao::caobab;
use std::sync::Arc;

use log::{debug, info};

fn main() {
    // Setup logging
    env_logger::init();
    // TODO cli argument parsing (esp. for input file, output file, file format)

    // Read input file
    debug!("Opening input file {}\n", "export_event.json");
    let file = std::fs::File::open("export_event.json").unwrap();
    // TODO allow selecting simpler file format
    let (participants, courses) = cdecao::io::cdedb::read(file).unwrap();
    info!(
        "Read {} courses and {} participants\n",
        courses.len(),
        participants.len()
    );

    // Execute assignment algorithm
    let courses = Arc::new(courses);
    let participants = Arc::new(participants);
    let result = caobab::solve(courses.clone(), participants.clone());

    // TODO output in machine-readable format
    if let Some((assignment, _)) = result {
        print!(
            "{}",
            cdecao::io::format_assignment(&assignment, &*courses, &*participants)
        );
    }
}
