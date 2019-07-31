use super::BABNode;
use crate::bab::NodeResult;
use crate::{Assignment, Course, Participant};
use std::sync::Arc;

fn create_simple_problem() -> (Vec<Participant>, Vec<Course>) {
    // Idea: Course 1 or 2 must be cancelled, b/c otherwise, we don't have enough participants to fill all courses.
    // Course 1 will win due to Participant 5's choices, so Course 2 will be cancelled.
    (
        vec![
            Participant {
                index: 0,
                dbid: 0,
                name: String::from("Participant 0"),
                choices: vec![1, 2],
            },
            Participant {
                index: 1,
                dbid: 1,
                name: String::from("Participant 1"),
                choices: vec![0, 2],
            },
            Participant {
                index: 2,
                dbid: 2,
                name: String::from("Participant 2"),
                choices: vec![0, 1],
            },
            Participant {
                index: 3,
                dbid: 3,
                name: String::from("Participant 3"),
                choices: vec![0, 1],
            },
            Participant {
                index: 4,
                dbid: 4,
                name: String::from("Participant 4"),
                choices: vec![0, 2],
            },
            Participant {
                index: 5,
                dbid: 5,
                name: String::from("Participant 5"),
                choices: vec![1, 2],
            },
        ],
        vec![
            Course {
                index: 0,
                dbid: 0,
                name: String::from("Wanted Course 0"),
                num_max: 2,
                num_min: 2,
                instructors: vec![0],
            },
            Course {
                index: 1,
                dbid: 1,
                name: String::from("Okay Course 1"),
                num_max: 8,
                num_min: 2,
                instructors: vec![1],
            },
            Course {
                index: 2,
                dbid: 2,
                name: String::from("Boring Course 2"),
                num_max: 10,
                num_min: 2,
                instructors: vec![2],
            },
        ],
    )
}

#[test]
fn test_precompute_problem() {
    let (participants, courses) = create_simple_problem();

    let problem = super::precompute_problem(&courses, &participants, Some(&vec![8, 10]));

    // check vector sizes
    let m = courses.iter().fold(0, |acc, c| acc + c.num_max);
    let num_instructors = courses.iter().fold(0, |acc, c| acc + c.instructors.len());
    let n = m + num_instructors;
    assert_eq!(problem.adjacency_matrix.dim().0, n);
    assert_eq!(problem.adjacency_matrix.dim().1, m);
    assert_eq!(problem.dummy_x.dim(), n);
    assert_eq!(problem.course_map.dim(), m);
    assert_eq!(problem.inverse_course_map.len(), courses.len());

    // Check references of courses
    for (i, c) in courses.iter().enumerate() {
        for j in 0..c.num_max {
            let base_column = problem.inverse_course_map[i];
            assert_eq!(
                problem.course_map[base_column + j],
                i,
                "Column {} should be mapped to course {}, as it is within {} columns after {}",
                base_column + j,
                i,
                c.num_max,
                base_column
            );
        }
    }

    // check adjacency matrix
    const WEIGHTS: [u16; 3] = [
        super::WEIGHT_OFFSET,
        super::WEIGHT_OFFSET - 1,
        super::WEIGHT_OFFSET - 2,
    ];
    for (x, p) in participants.iter().enumerate() {
        for y in 0..m {
            let choice = p.choices.iter().position(|c| *c == problem.course_map[y]);
            assert_eq!(
                problem.adjacency_matrix[(x, y)],
                match choice {
                    Some(c) => WEIGHTS[c],
                    None => 0,
                },
                "Edge weigth for participant {} with course place {} is not expected.",
                x,
                y
            );
        }
    }
    for x in participants.len()..n {
        for y in 0..m {
            assert_eq!(
                problem.adjacency_matrix[(x, y)],
                0,
                "Edge weigth for dummy participant {} with course place {} is not zero.",
                x,
                y
            );
        }
    }

    // check dummy_x
    for x in 0..participants.len() {
        assert!(!problem.dummy_x[x]);
    }
    for x in participants.len()..n {
        assert!(problem.dummy_x[x]);
    }

    // Check room_sizes
    assert_eq!(problem.room_sizes, Some(vec![10, 8, 0]));

    // A second try, without rooms given
    let problem = super::precompute_problem(&courses, &participants, None);
    assert_eq!(problem.room_sizes, None);
}

#[test]
fn test_babnode_sorting() {
    let node0 = BABNode {
        cancelled_courses: vec![],
        enforced_courses: vec![],
        shrinked_courses: vec![],
        no_more_shrinking: vec![],
    };
    let node1 = BABNode {
        cancelled_courses: vec![0],
        enforced_courses: vec![],
        shrinked_courses: vec![],
        no_more_shrinking: vec![],
    };
    assert!(node0 < node1);
    let node2 = BABNode {
        cancelled_courses: vec![],
        enforced_courses: vec![2],
        shrinked_courses: vec![],
        no_more_shrinking: vec![],
    };
    assert!(node0 < node2);
    let node3 = BABNode {
        cancelled_courses: vec![1, 2],
        enforced_courses: vec![],
        shrinked_courses: vec![],
        no_more_shrinking: vec![],
    };
    assert!(node1 < node3);
    assert!(node2 < node3);
    let node4 = BABNode {
        cancelled_courses: vec![],
        enforced_courses: vec![0, 1, 2],
        shrinked_courses: vec![],
        no_more_shrinking: vec![],
    };
    assert!(node2 < node4);
    let node5 = BABNode {
        cancelled_courses: vec![0, 1],
        enforced_courses: vec![0, 1],
        shrinked_courses: vec![],
        no_more_shrinking: vec![],
    };
    assert!(node4 < node5);
    let node6 = BABNode {
        cancelled_courses: vec![],
        enforced_courses: vec![0, 1, 2],
        shrinked_courses: vec![(0, 10), (1, 20)],
        no_more_shrinking: vec![],
    };
    assert!(node4 < node6);
    assert!(node5 < node6);
    let node7 = BABNode {
        cancelled_courses: vec![0, 1],
        enforced_courses: vec![0],
        shrinked_courses: vec![(0, 10), (1, 20), (0, 8)],
        no_more_shrinking: vec![],
    };
    assert!(node5 < node7);
    assert!(node6 < node7);
}

#[test]
fn test_check_feasibility() {
    let (participants, courses) = create_simple_problem();

    // A feasible assignment
    let assignment: Assignment = vec![0, 1, 1, 0, 0, 1];
    let course_instructors =
        ndarray::Array1::from_vec(vec![true, true, false, false, false, false]);
    let node = BABNode {
        cancelled_courses: vec![2],
        enforced_courses: vec![],
        shrinked_courses: vec![],
        no_more_shrinking: vec![],
    };
    assert_eq!(
        super::check_feasibility(
            &courses,
            &participants,
            &assignment,
            &node,
            &course_instructors
        ),
        (true, false, None)
    );

    // Courses 1 & 2 have to few participants. Course 2 lacks more than Course 1.
    let assignment: Assignment = vec![0, 1, 2, 0, 0, 1];
    let course_instructors = ndarray::Array1::from_vec(vec![true, true, true, false, false, false]);
    let node = BABNode {
        cancelled_courses: vec![],
        enforced_courses: vec![],
        shrinked_courses: vec![],
        no_more_shrinking: vec![],
    };
    assert_eq!(
        super::check_feasibility(
            &courses,
            &participants,
            &assignment,
            &node,
            &course_instructors
        ),
        (false, false, Some(2))
    );

    // Let's ignore that Participant 4 chose course 0. Participant 5 has been assigned to wrong course 0. We should
    // cancel course 2 to fill course 0 with its instructor.
    let assignment: Assignment = vec![0, 1, 2, 0, 1, 0];
    let course_instructors = ndarray::Array1::from_vec(vec![true, true, true, false, false, false]);
    let node = BABNode {
        cancelled_courses: vec![],
        enforced_courses: vec![0],
        shrinked_courses: vec![],
        no_more_shrinking: vec![],
    };
    assert_eq!(
        super::check_feasibility(
            &courses,
            &participants,
            &assignment,
            &node,
            &course_instructors
        ),
        (false, true, Some(2))
    );
}

/// Testing helper function to check correctness of a feasible solution for the full branch and bound problem or a
/// single subproblem. To test a subproblem, simply pass the BABNode. In this case we will check, that exactly the
/// `cancelled_courses` have no assigned participants.
fn check_assignment(
    courses: &Vec<Course>,
    participants: &Vec<Participant>,
    assignment: &Assignment,
    node: Option<&BABNode>,
) {
    // Calculate course sizes
    let mut course_size = vec![0usize; courses.len()];
    for c in assignment.iter() {
        course_size[*c] += 1;
    }

    // Check course sizes
    for (c, size) in course_size.iter().enumerate() {
        let course = &courses[c];
        assert!(
            *size <= course.num_max + course.instructors.len(),
            "Maximum size violation for course {}: {} places, {} participants",
            c,
            course.num_max,
            size - course.instructors.len()
        );
        // Feasible solutions must not have too few participants
        if let Some(n) = node {
            if !n.cancelled_courses.contains(&c) {
                assert!(
                    *size >= course.num_min + course.instructors.len(),
                    "Minimum size violation for course {}: {} required, {} assigned",
                    c,
                    course.num_min,
                    size - course.instructors.len()
                );
            } else {
                assert_eq!(
                    course_size[c], 0,
                    "Cancelled course {} has {} participants",
                    c, course_size[c]
                );
            }
        } else {
            assert!(
                *size == 0 || *size >= course.num_min + course.instructors.len(),
                "Minimum size violation for course {}: {} required, {} assigned",
                c,
                course.num_min,
                size - course.instructors.len()
            );
        }
    }
    if let Some(n) = node {
        // Check shrinked courses' sizes
        for (c, s) in n.shrinked_courses.iter() {
            let course = &courses[*c];
            assert!(
                course_size[*c] <= *s + course.instructors.len(),
                "Dynamic size constraint for course {} not satisfied: {} > {}",
                *c,
                course_size[*c] - course.instructors.len(),
                *s
            )
        }
    }

    // Check course instructor assignment
    let mut course_instructors = vec![false; participants.len()];
    for (c, course) in courses.iter().enumerate() {
        if course_size[c] != 0 {
            for i in course.instructors.iter() {
                assert_eq!(
                    assignment[*i], c,
                    "Instructor {} of course {} is assigned to {}",
                    *i, c, assignment[*i]
                );
                course_instructors[*i] = true;
            }
        }
    }

    // Feasible solutions must not have wrong assigned participants
    for (p, participant) in participants.iter().enumerate() {
        if !course_instructors[p] {
            assert!(
                participant.choices.contains(&assignment[p]),
                "Course {} of participant {} is none of their choices ({:?})",
                assignment[p],
                p,
                participant.choices
            );
        }
    }
}

#[test]
fn test_bab_node_simple() {
    // This test depends on `precompute_problem()`, `check_feasibility()` and `hungarian::hungarian_algorithm()`,
    // so if it fails, please check their test results first.

    let (participants, courses) = create_simple_problem();
    let problem = super::precompute_problem(&courses, &participants, None);

    // Let's get a feasible solution
    let node = BABNode {
        cancelled_courses: vec![1],
        enforced_courses: vec![],
        shrinked_courses: vec![],
        no_more_shrinking: vec![],
    };
    let result = super::run_bab_node(&courses, &participants, &problem, node.clone());
    match result {
        NodeResult::Feasible(assignment, score) => {
            print!("assignment: {:?}\n", assignment);
            check_assignment(&courses, &participants, &assignment, Some(&node));
            assert!(score > participants.len() as u32 * (super::WEIGHT_OFFSET as u32 - 1));
        }
        x => panic!("Expected feasible result, got {:?}", x),
    };

    // This should also work out
    let node = BABNode {
        cancelled_courses: vec![2],
        enforced_courses: vec![1],
        shrinked_courses: vec![],
        no_more_shrinking: vec![],
    };
    let result = super::run_bab_node(&courses, &participants, &problem, node.clone());
    match result {
        NodeResult::Feasible(assignment, score) => {
            print!("assignment: {:?}\n", assignment);
            check_assignment(&courses, &participants, &assignment, Some(&node));
            assert!(score > participants.len() as u32 * (super::WEIGHT_OFFSET as u32 - 1));
        }
        x => panic!("Expected feasible result, got {:?}", x),
    };

    // This way, we should not get any solution
    let node = BABNode {
        cancelled_courses: vec![1, 2],
        enforced_courses: vec![],
        shrinked_courses: vec![],
        no_more_shrinking: vec![],
    };
    let result = super::run_bab_node(&courses, &participants, &problem, node);
    match result {
        NodeResult::NoSolution => (),
        x => panic!("Expected no result, got {:?}", x),
    };

    // This should give us an infeasible solution (too few participants in course 1, 2)
    let node = BABNode {
        cancelled_courses: vec![],
        enforced_courses: vec![],
        shrinked_courses: vec![],
        no_more_shrinking: vec![],
    };
    let result = super::run_bab_node(&courses, &participants, &problem, node);
    match result {
        NodeResult::Infeasible(_, _) => (), // TODO check new nodes and score
        x => panic!("Expected infeasible result, got {:?}", x),
    };
}

#[test]
fn test_bab_node_large() {
    const NUM_COURSES: usize = 20;
    const MAX_PLACES_PER_COURSE: usize = 10;
    const MIN_PLACES_PER_COURSE: usize = 6;
    const NUM_PARTICIPANTS: usize = 200;

    let mut courses = Vec::<Course>::new();
    for c in 0..NUM_COURSES {
        courses.push(Course {
            index: c,
            dbid: c,
            name: format!("Course {}", c),
            num_min: MIN_PLACES_PER_COURSE,
            num_max: MAX_PLACES_PER_COURSE,
            instructors: Vec::new(),
        });
    }

    let mut participants = Vec::<Participant>::new();
    for p in 0..NUM_PARTICIPANTS {
        let mut participant = Participant {
            index: p,
            dbid: p,
            name: format!("Participant {}", p),
            choices: Vec::new(),
        };
        for i in 0..3 {
            participant.choices.push((p + i) % NUM_COURSES);
        }
        participants.push(participant);
        if p < NUM_COURSES {
            if p == 0 {
                courses[NUM_COURSES - 1].instructors.push(p);
            } else {
                courses[p - 1].instructors.push(p);
            }
        }
    }

    let problem = super::precompute_problem(&courses, &participants, None);
    let node = BABNode {
        cancelled_courses: vec![],
        enforced_courses: vec![],
        shrinked_courses: vec![],
        no_more_shrinking: vec![],
    };

    let result = super::run_bab_node(&courses, &participants, &problem, node.clone());

    match result {
        NodeResult::Feasible(assignment, score) => {
            print!("assignment: {:?}\n", assignment);
            check_assignment(&courses, &participants, &assignment, Some(&node));
            assert!(score > participants.len() as u32 * (super::WEIGHT_OFFSET as u32 - 1));
        }
        x => panic!("Expected feasible result, got {:?}", x),
    }
}

// TODO test bab_node with shrinked courses

#[test]
fn test_caobab_simple() {
    // This test depends on `precompute_problem()`, `check_feasibility()`, `run_bab_node()`,
    // `hungarian::hungarian_algorithm()`, and `bab::solve()`; so if it fails, please check their test results first.
    let (participants, courses) = create_simple_problem();
    let courses = Arc::new(courses);
    let participants = Arc::new(participants);
    let (result, _statistics) = super::solve(courses.clone(), participants.clone(), None);

    match result {
        Some((assignment, score)) => {
            print!("assignment: {:?}\n", assignment);
            check_assignment(&*courses, &*participants, &assignment, None);
            assert!(score > participants.len() as u32 * (super::WEIGHT_OFFSET as u32 - 1));
            assert!(
                assignment == vec![0, 1, 0, 1, 0, 1] || assignment == vec![0, 1, 1, 0, 0, 1],
                "Unexpected assignment: {:?}",
                assignment
            );
        }
        _ => panic!("Expected to get a result."),
    };
}

// TODO test solve with large problem

// TODO test solve with room list
