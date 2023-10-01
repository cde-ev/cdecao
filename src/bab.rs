// Copyright 2019 by Michael Thies <mail@mhthies.de>
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

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
use num_traits::bounds::Bounded;
use std::collections::BinaryHeap;
use std::sync::{Arc, Condvar, Mutex};
use std::{fmt, thread, time};

/// Struct to hold the synchronization information for the parallel execution. It contains a mutex-ed SharedState object
/// And a Candvar to allow worker threads to sleep-wait for new subproblems to solve.
struct BranchAndBound<SubProblem: Ord + Send, Solution: Send, Score: Ord> {
    shared_state: Mutex<SharedState<SubProblem, Solution, Score>>,
    condvar: Condvar,
}

/// The shared state of the worker threads of the parallel branch and bound execution
struct SharedState<SubProblem: Ord, Solution, Score: Ord> {
    /// The prioritized queue of pending subproblems (and the parent node's score, for bounding)
    pending_nodes: BinaryHeap<PendingProblem<SubProblem, Score>>,
    /// The number of currently busy worker threads. It is used to determine the end of execution (no pending problems
    /// and no busy workers left)
    busy_threads: u32,
    /// The best solution, found so far
    best_result: Option<Solution>,
    /// The score of the best solution, found so far
    best_score: Score,
    /// Solver Statistics
    statistics: Statistics,
}

#[derive(PartialOrd, Ord, PartialEq, Eq)]
struct PendingProblem<SubProblem, Score>(SubProblem, Score);

/// A struct to collect statistics about the branch and bound execution.
///
/// It is held in the SharedState while execution and returned afterwards.
#[derive(Default)]
pub struct Statistics {
    /// Number of calls to the subproblem solver function
    pub num_executed_subproblems: u32,
    /// Number of subproblems that returned without solution
    pub num_no_solution: u32,
    /// Number of infeasible subproblems encountered during solving
    pub num_infeasible: u32,
    /// Number of feasible subproblems encountered during solving
    pub num_feasible: u32,
    /// Number of times the pior best result has been updated with a better result
    pub num_new_best: u32,
    /// Number of subproblems skipped because of their (infeasible) parent's score (i.e. number of
    /// bound branches)
    pub num_bound_subproblems: u32,
    /// Total time for executing the branch and bound algorithm
    pub total_time: time::Duration,
    /// Cummulated exeuction time of the subproblem solver function
    /// Heads up! Due to parallelism this will be multiple times `total_time`.
    pub total_subproblem_time: time::Duration,
}

impl fmt::Display for Statistics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Solving statistics:
Executed subproblems:  {: >6}
    ... no solution:   {: >6}
    ... infeasible:    {: >6}
    ... feasible:      {: >6}
         ... new best: {: >6}
Bound branches:        {: >6}

Total time: {:.3}s
Average subproblem solver time: {:.3}s\n",
            self.num_executed_subproblems,
            self.num_no_solution,
            self.num_infeasible,
            self.num_feasible,
            self.num_new_best,
            self.num_bound_subproblems,
            self.total_time.as_millis() as f32 / 1000f32,
            (self.total_subproblem_time / self.num_executed_subproblems).as_millis() as f32
                / 1000f32
        )
    }
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
///
/// This function takes a callback function, which is executed for each single node in the branch and bound tree and
/// returns either a feasible solution to be considered for the result or a `Vec` of new subproblems to try (see
/// `NodeResult` type). The type of the subproblems must implement `Ord` where p1 > p2 means, p1 is
/// in a deeper layer of the branch and bound tree. This is property is used to perform a
/// pseudo-depth-first search in the tree. Within one layer, nodes are ordered by the parent node's
/// solution score and their order of appearing. I.e. subproblems with higher probability for good
/// scores should be put first in the NodeResult::Infeasible's vector.
///
/// When all branches of the branch and bound tree are evaluated (or bound), the best result is
/// returned. It may be possible, that no result is found at all.
///
/// # Result
///
/// Returns the best solution and its score (if one has been found) and some statistics about the solving process.
pub fn solve<
    SubProblem: 'static + Ord + Send + fmt::Debug,
    Solution: 'static + Send,
    Score: 'static + Ord + Bounded + Send + Copy + fmt::Display,
    F: 'static,
>(
    node_solver: F,
    base_problem: SubProblem,
    num_threads: u32,
) -> (Option<(Solution, Score)>, Statistics)
where
    F: (Fn(SubProblem) -> NodeResult<SubProblem, Solution, Score>) + Send + Sync,
{
    // Create shared data structure with base problem
    let mut pending_nodes = BinaryHeap::new();
    pending_nodes.push(PendingProblem(base_problem, Score::max_value()));
    let bab = Arc::new(BranchAndBound {
        shared_state: Mutex::new(SharedState {
            pending_nodes,
            busy_threads: 0,
            best_result: None,
            best_score: Score::min_value(),
            statistics: Statistics::default(),
        }),
        condvar: Condvar::new(),
    });

    let tic = time::Instant::now();

    // Spawn worker threads
    let mut workers = Vec::<thread::JoinHandle<()>>::new();
    let node_solver = Arc::new(node_solver);
    for i in 0..num_threads {
        let bab_clone = bab.clone();
        let node_solver_clone = node_solver.clone();
        let thread = thread::Builder::new()
            .name(format!("BaB Worker {}", i))
            .spawn(move || worker(bab_clone, node_solver_clone))
            .unwrap();
        workers.push(thread);
    }

    // Wait for worker threads to finish
    for worker in workers {
        worker.join().unwrap();
    }

    let total_time = tic.elapsed();

    // Unwrap and return result
    let mut shared_state = Arc::try_unwrap(bab)
        .map_err(|_| ())
        .expect("Could not unwrap Arc to Bab object.")
        .shared_state
        .into_inner()
        .expect("Could not move SharedState out of mutex.");
    shared_state.statistics.total_time = total_time;
    
    (
        match shared_state.best_result {
            None => None,
            Some(x) => Some((x, shared_state.best_score)),
        },
        shared_state.statistics,
    )
}

/// Worker thread entry point for the parallel branch and bound solving
fn worker<SubProblem: Ord + Send + fmt::Debug, Solution: Send, Score: Ord + Copy + fmt::Display>(
    bab: Arc<BranchAndBound<SubProblem, Solution, Score>>,
    node_solver: Arc<dyn Fn(SubProblem) -> NodeResult<SubProblem, Solution, Score>>,
) {
    let mut shared_state = bab.shared_state.lock().unwrap();
    loop {
        // In case of pending subproblems, get one and solve it
        if let Some(PendingProblem(subproblem, parent_score)) = shared_state.pending_nodes.pop() {
            // Only consider this subproblem, if the parent node's solution was better then best solution known so
            // far. I.e. bound branch if score will be worse then best known feasible solution.
            if parent_score > shared_state.best_score {
                shared_state.busy_threads += 1;

                // Unlock shared_state and solve subproblem
                std::mem::drop(shared_state);
                let subproblem_formatted = format!("{:?}", subproblem);
                debug!("Solving subproblem: {}", subproblem_formatted);
                let tic = time::Instant::now();
                let result = node_solver(subproblem);
                let consumed_time = tic.elapsed();

                // Reacquire shared_state lock and interpret subproblem result
                shared_state = bab.shared_state.lock().unwrap();
                shared_state.busy_threads -= 1;
                shared_state.statistics.num_executed_subproblems += 1;
                shared_state.statistics.total_subproblem_time += consumed_time;
                match result {
                    NodeResult::NoSolution => {
                        shared_state.statistics.num_no_solution += 1;
                    }

                    NodeResult::Feasible(solution, score) => {
                        shared_state.statistics.num_feasible += 1;
                        debug!("Yes! We found a feasible solution with score {}: {}", score, subproblem_formatted);
                        if score > shared_state.best_score {
                            debug!(
                                "Wow, this is the best solution, we found so far. Let's store it."
                            );
                            shared_state.statistics.num_new_best += 1;
                            shared_state.best_result = Some(solution);
                            shared_state.best_score = score;
                        }
                    }

                    NodeResult::Infeasible(new_problems, score) => {
                        shared_state.statistics.num_infeasible += 1;
                        debug!("We found an infeasible solution with score {}: {}", score, subproblem_formatted);
                        // Add new subproblems to queue
                        for (i, new_problem) in new_problems.into_iter().enumerate() {
                            shared_state
                                .pending_nodes
                                .push(PendingProblem(new_problem, score));
                            // Wake up n-1 other threads to solve the new subproblems
                            if i != 0 {
                                bab.condvar.notify_one();
                            }
                        }
                    }
                }
            } else {
                shared_state.statistics.num_bound_subproblems += 1;
                debug!(
                    "Bounding this branch, since score {} is already worse then best known feasible solution: {:?}",
                    parent_score,
                    subproblem,
                );
            }

            // check if we are finished, awake other threads and exit
            if shared_state.pending_nodes.is_empty() && shared_state.busy_threads == 0 {
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

// =============================================================================
// Tests
#[cfg(test)]
mod tests {
    use super::NodeResult;
    use ordered_float::NotNan;
    use std::collections::BTreeMap;

    #[test]
    fn test_bab_rounding() {
        // This test tries to find the closest integer vector to a given vector in a rather stupid
        // way: We branch over each vector entry and calculate the negated distance as score.

        #[derive(Clone, Debug)]
        struct SubProblem(BTreeMap<usize, i32>);
        impl Ord for SubProblem {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.0.len().cmp(&other.0.len())
            }
        }
        impl PartialOrd for SubProblem {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }
        impl Eq for SubProblem {}
        impl PartialEq for SubProblem {
            fn eq(&self, other: &Self) -> bool {
                self.0.len() == other.0.len()
            }
        }

        fn solver(
            mut node: SubProblem,
            target: ndarray::Array1<f32>,
        ) -> NodeResult<SubProblem, ndarray::Array1<i32>, NotNan<f32>> {
            // Otherwise calculate score
            let mut result = ndarray::Array1::<i32>::zeros(target.dim());
            let mut score_squared = 0f32;
            let mut missing_entry = None;
            for x in 0..target.dim() {
                match node.0.get(&x) {
                    None => missing_entry = Some(x),
                    Some(y) => {
                        result[x] = *y;
                        score_squared += (target[x] - *y as f32).powf(2.0);
                    }
                }
            }

            match missing_entry {
                None => {
                    NodeResult::Feasible(result, NotNan::new(-score_squared.powf(0.5)).unwrap())
                }
                Some(x) => {
                    let mut n1 = node.clone();
                    n1.0.insert(x, target[x] as i32);
                    node.0.insert(x, target[x] as i32 + 1);
                    NodeResult::Infeasible(
                        vec![n1, node],
                        NotNan::new(-score_squared.powf(0.5)).unwrap(),
                    )
                }
            }
        }

        let (result, statistics) = super::solve(
            move |node| solver(node, ndarray::arr1(&[0.51, 0.46, 3.7, 0.56, 0.6])),
            SubProblem(BTreeMap::new()),
            1,
        );
        match result {
            None => panic!("Expected to get a solution"),
            Some((solution, _)) => assert_eq!(solution, ndarray::arr1(&[1, 0, 4, 1, 1])),
        }
        assert!(statistics.num_executed_subproblems > 0);
        assert!(
            statistics.num_executed_subproblems < 2u32.pow(6) - 1,
            "Number of executed subproblems should be < 2^6-1, due to bounding."
        );
        assert!(statistics.num_bound_subproblems > 0);

        // Unfortunately, there's no good (platform independent) check, if parallelism works. :(
        let (result, _statistics) = super::solve(
            move |node| solver(node, ndarray::arr1(&[0.51, 6.46, 0.7, 0.56, 0.6])),
            SubProblem(BTreeMap::new()),
            4,
        );
        match result {
            None => panic!("Expected to get a solution"),
            Some((solution, _)) => assert_eq!(solution, ndarray::arr1(&[1, 6, 1, 1, 1])),
        }
    }
}
