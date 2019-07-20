//! This module provides a generic implementation of the branch and bound algorithm using a parallel pseudo-depth-first
//! search.
//!
//! The basic idea is to spawn a number of worker threads to solve the subproblems in parallel. The pending subproblems
//! (nodes in the Branch and Bound tree) are stored on a heap (priority queue), ordered by their depth in the tree. This
//! way, the worker threads can work in parallel, while preferring to dig into the depth of the Branch and Bound tree,
//! which will give good lower bounds for bounding the branches sooner.
//!
//! The best feasible solution, found so far, is kept with the subproblem queue in a shared data structure. Its score is
//! used as a lower bound for branches' scores.
//!
//! The worker threads are stopped, as soon as no pending subproblems are left *and* no thread is still busy (and could
//! produce new pending subproblems).

use log::debug;
use std::collections::BinaryHeap;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use num_traits::bounds::Bounded;

/// Struct to hold the synchronization information for the parallel execution. It contains a mutex-ed SharedState object
/// And a Candvar to allow worker threads to sleep-wait for new subproblems to solve.
struct BranchAndBound<SubProblem: Ord + Send, Solution: Send, Score> {
    shared_state: Mutex<SharedState<SubProblem, Solution, Score>>,
    condvar: Condvar,
}

/// The shared state of the worker threads of the parallel branch and bound execution
struct SharedState<SubProblem: Ord, Solution, Score> {
    /// The prioritized queue of pending subproblems
    pending_nodes: BinaryHeap<SubProblem>,
    /// The number of currently busy worker threads. It is used to determine the end of execution (no pending problems
    /// and no busy workers left)
    busy_threads: u32,
    /// The best solution, found so far
    best_result: Option<Solution>,
    /// The score of the best solution, found so far
    best_score: Score,
}

/// Result type for solving a single branch and bound node.
#[derive(Debug)]
pub enum NodeResult<SubProblem, Solution, Score> {
    /// No solution at all (subproblem was infeasible)
    NoSolution,
    /// An infeasible solution for the main problem with an iterable of more restricted SubProblems ("branches") to try
    /// and the solution's score to bound the branches by comparing the solution with the current best solution.
    Infeasible(Vec<SubProblem>, Score),
    /// A feasible solution for the main problem (including the solution's score to compare to other solutions)
    Feasible(Solution, Score),
}

/// Main function of this module to solve a generic problem by doing pseudo-depth-first parallel branch and bound
/// optimization.
/// This function takes a callback function, which is executed for each single node in the branch and bound tree and
/// returns either a feasible solution to be considered for the result or a `Vec` of new subproblems to try (see
/// `NodeResult` type). When all branches of the branch and bound tree are evaluated (or bound), the best result is
/// returned. It may be possible, that no result is found at all.
pub fn solve<
    SubProblem: 'static + Ord + Send,
    Solution: 'static + Send,
    Score: 'static + PartialOrd + Bounded + Send + Copy,
    F: 'static,
>(
    node_solver: F,
    base_problem: SubProblem,
    num_threads: u32,
) -> Option<(Solution, Score)>
where
    F: (Fn(SubProblem) -> NodeResult<SubProblem, Solution, Score>) + Send + Sync,
{
    // Create shared data structure with base problem
    let mut pending_nodes = BinaryHeap::<SubProblem>::new();
    pending_nodes.push(base_problem);
    let bab = Arc::new(BranchAndBound {
        shared_state: Mutex::new(SharedState {
            pending_nodes: pending_nodes,
            busy_threads: 0,
            best_result: None,
            best_score: Score::min_value(),
        }),
        condvar: Condvar::new(),
    });

    // Spawn worker threads
    let mut workers = Vec::<thread::JoinHandle<()>>::new();
    let node_solver = Arc::new(node_solver);
    for _i in 0..num_threads {
        let bab_clone = bab.clone();
        let node_solver_clone = node_solver.clone();
        workers.push(thread::spawn(move || worker(bab_clone, node_solver_clone)));
    }

    // Wait for worker threads to finish
    for worker in workers {
        worker.join().unwrap();
    }

    // Unwrap and return result
    let mut shared_state = bab.shared_state.lock().unwrap();
    return match shared_state.best_result.take() {
        None => None,
        Some(x) => Some((x, shared_state.best_score)),
    };
}

/// Worker thread entry point for the parallel branch and bound solving
fn worker<SubProblem: Ord + Send, Solution: Send, Score: PartialOrd>(
    bab: Arc<BranchAndBound<SubProblem, Solution, Score>>,
    node_solver: Arc<Fn(SubProblem) -> NodeResult<SubProblem, Solution, Score>>,
) {
    let mut shared_state = bab.shared_state.lock().unwrap();
    loop {
        // In case of pending subproblems, get one and solve it
        if let Some(subproblem) = shared_state.pending_nodes.pop() {
            shared_state.busy_threads += 1;

            // Unlock shared_state and solve subproblem
            std::mem::drop(shared_state);
            let result = node_solver(subproblem);

            // Reacquire shared_state lock and interpret subproblem result
            shared_state = bab.shared_state.lock().unwrap();
            shared_state.busy_threads -= 1;
            match result {
                NodeResult::NoSolution => (),

                NodeResult::Feasible(solution, score) => {
                    if score > shared_state.best_score {
                        debug!("Wow, this is the best solution, we found so far. Let's store it.");
                        shared_state.best_result = Some(solution);
                        shared_state.best_score = score;
                    }
                }

                NodeResult::Infeasible(new_problems, score) => {
                    // Only consider more restricted new_problems, if solution is better then best solution known so
                    // far. I.e. bound branch if score is worse then best known feasible solution
                    // TODO store score with new subproblems and bound, when starting solving the new subproblem. (We
                    //   might have a better lower bound by then)
                    if score > shared_state.best_score {
                        for (i, new_problem) in new_problems.into_iter().enumerate() {
                            shared_state.pending_nodes.push(new_problem);
                            // Wake up n-1 other threads to solve the new subproblems
                            if i != 0 {
                                bab.condvar.notify_one();
                            }
                        }
                    } else {
                        debug!("Bounding this branch, since score is already worse then best known feasible solution.");
                    }
                }
            }

            // check if we are finished, awake other threads and exit
            if shared_state.pending_nodes.len() == 0 && shared_state.busy_threads == 0 {
                bab.condvar.notify_all();
                break;
            }

        // Otherwise wait for new subproblems
        } else if shared_state.busy_threads > 0 {
            // Wait for notification by other threads. CondVar.wait() automatically handels the mutex unlock and re-lock
            // for us.
            shared_state = bab.condvar.wait(shared_state).unwrap();

        // If no work is left to do, exit
        } else {
            break;
        }
    }
}
