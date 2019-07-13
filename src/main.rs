use cdecao::{caobab, Course, Participant};
use std::sync::Arc;

fn main() {
    let courses = Arc::new(Vec::<Course>::new());
    let participants = Arc::new(Vec::<Participant>::new());
    caobab::solve(courses, participants);
}
