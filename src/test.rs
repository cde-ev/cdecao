

fn main() {
    // Read input file
    let file = std::fs::File::open("export_event.json").unwrap();
    let (participants, courses) = cdecao::io::cdedb::read(file).unwrap();
    print!(
        "Read {} courses and {} participants\n",
        courses.len(),
        participants.len()
    );
    let participants = participants.into_iter().filter(|p| p.choices.len() > 0).collect();

    let file = std::fs::File::create("simple_data.json").unwrap();
    cdecao::io::simple::write(file, &participants, &courses);
}