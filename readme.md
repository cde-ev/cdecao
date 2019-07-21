
# CdE Course Assignment Optimization

This project contains a Rust implementation of the optimal course assignment algorithm for CdE events, developed by
Gabriel Guckenbiehl in his Master's thesis. This optimized implementation is mainly programmend and maintained by
Michael Thies <mail@mhthies.de>.

The algorithm combines a Branch and Bound approach for deciding which courses (in contrast to enforcing their minimal
participant number) with the hungarian algorithm to do the actual matching of participants with courses.


## Structure

The implementation consists of several parts, that are provided as separate Rust modules:

* A generic parallelized implementation of the Branch and Bound algorithm (`bab`)
* An implementation of the hungarian algorithm (`hungarian`)
* The specialization of the Branch and Bound algorithm for calculating course assignment using the hungarian algorithm
  (`caobab`)
* Data input/output via JSON files (`io`)


## Getting started

Currently, major parts of the `io` module and the main application are still missing. To do some testing with real world
data, the main application currently tries to read an event export of the CdE Datenbank from the file
`./export_event.json`, which must include exactly one course track.

To execute this real world scenario, download an export file from the CdE Datenbank, store it in the project directory
as "export_event.json" and execute
```
cargo run --release
```
in the project directory. To get more output about the algorithm's progress, set the loglevel to "debug":
```
RUST_LOG=debug cargo run --release
```


### Debugging and Testing

The overall application code is covered with tests quite well. To run them, execute
```
cargo test
```
in the project directory.
