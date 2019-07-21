use cdecao::caobab;
use std::sync::Arc;

use log::{info};

fn main() {
    env_logger::init();
    // TODO cli argument parsing (esp. for input file, file format)

    let file = std::fs::File::open("export_event.json").unwrap();
    // TODO allow selecting simpler file format
    let (participants, courses) = cdecao::io::cdedb::read(file).unwrap();
    info!("Read {} courses and {} participants\n", courses.len(), participants.len());

    let courses = Arc::new(courses);
    let participants = Arc::new(participants);
    let result = caobab::solve(courses.clone(), participants.clone());

    // TODO output in machine-readable format
    if let Some((assignment, _)) = result {
        for c in courses.iter() {
            print!("\n===== {} =====\n", c.name);
            for (ap, ac) in assignment.iter().enumerate() {
                if *ac == c.index {
                    print!("{}{}\n", participants[ap].name, if c.instructors.contains(&ap) {" (instr)"} else {""});
                }
            }
        }
    }
}
