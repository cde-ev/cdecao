
# CdE Course Assignment Optimization (cdecao)

This project contains a Rust implementation of the optimal course assignment algorithm for CdE events, developed by
Gabriel Guckenbiehl <gabriel.guckenbiehl@gmx.de> in his Master's thesis. This optimized implementation is mainly programmend and maintained by
Michael Thies <mail@mhthies.de>.

The algorithm combines a Branch and Bound approach for deciding which courses to constrain in their maximum size to fit
the available rooms and which courses to cancel (in contrast to enforcing their minimal participant number) with the
hungarian algorithm to do the actual matching of participants with courses.


## Usage

The course assignment application is a command line only application: It takes a few command line arguments, reads its
input data from a JSON file and outputs the calculated assignment as a JSON file and/or in the terminal. The only
mandatory command line parameter is the input file. Additionally you should add `--print` to show the calculated
course assignment on the terminal or specify an output file to save the assignment as a JSON file:
```sh
cdecao --print data.json
```
or
```sh
cdecao data.json assignment.json
```


### Course Room Fitting

The implemented course assignment algorithm includes an (experimental) extension for considering constraints on
**available course rooms**. To use this functionality, simple give a list of available course room sizes (incl. course
instructors):
```sh
cdecao --rooms "20,20,20,10,10,10,10,10,10,8,8" --print data.json
```
This works with both data file formats. For more control about course room matching, the "effective size" of each course
can be defined as an affine function of the course's actual number of participants. For this purpose, each course has
two attributes `room_factor` and `room_offset`, where

*effective_size = room_offset + room_factor * (num_participants + num_instructors)*.

The algorithm will automatically reduce the number of participants of some courses and cancel courses if required, such
that all courses can find room with at least their effective size. Different combinations (not all possible – for
complexity reasons) of "shrunk" and cancelled courses are computed to find the one which allows the best course
assignment.


### CdE Datenbank Export format options

By default, the application uses a very simple JSON format for input of course and participant lists and output of the
calculated course assignment. To use the **CdE Datenbank's partial export/import** format instead, add the `--cde` option:
```sh
cdecao --cde pa19_partial_event_export.json
```
In this case, the resulting output file can be imported into the CdE Datenbank using the "Partial Import" feature.

For CdE events with more than one course track, the algorithm can only assign participants in one of the course tracks per execution.
Therefore, the relevant track's id has to be given via the `--track` parameter.
If not `--track` is specified and the given event input file contains multiple tracks, the program outputs an overview of available tracks and their ids and exits. 

If using the --cde data format, you can optionally select to **ignore already cancelled courses** (instead of considering
them for assignment and probably un-cancelling them) and/or to **ignore already assigned participants** (instead of
re-assigning them). To do so, use `--ignore-cancelled` resp. `--ignore-assigned`. Attention: Ignoring assigned
participants prevents their assigned courses from being cancelled (unless they are already cancelled and
`--ignore-cancelled` is given). *This might impair the solution's quality or even make the problem unsolvable.*

The *room_factor* and *room_offset* for course room fitting can be specified for each course via data fields in the
CdE Datenbank. With the command line options `--room-factor-field` and `--room-offset-field` the name of the respective
fields can be specified. Both fields needs to be a numeric (float or integer) data field.


### Logging options

If you want to see more log output (e.g. about the program's solving progress), you can set the loglevel to 'debug' or
'trace' via the `RUST_LOG` environment variable:
```sh
RUST_LOG=debug cdecao --print data.json
```
For more information, take a look at `env_logger`'s documentation: https://docs.rs/env_logger/0.6.2/env_logger/

A special command line flat can help with debugging unsolvable or hardly solvable course assignment problems: With
`--report-no-solution`, additional INFO log messages are printed for (some kinds of) unsolvable subproblems. This
includes branches which are infeasible due to unfulfillable course choices or fixed courses.


### Simple Data Format

The default input format for courses and participants data looks like this:
```json
{
    "courses": [
        {
            "name": "1. Example Course",
            "num_min": 5,
            "num_max": 15,
            "instructors": [0, 1],
            "hidden_participant_names": ["Mister X"]
        },
        {
            "name": "2. Another Course",
            "num_min": 6,
            "num_max": 10,
            "instructors": [5],
            "room_factor": 1.5,
            "room_offset": 2.0,
            "fixed_course": true
        },
        ...
    ],
    "participants": [
        {
            "name": "Anton Administrator",
            "choices": [
                {"course": 1, "penalty": 0},
                {"course": 0, "penalty": 1},
                {"course": 6, "penalty": 2}
            ]
        },
        {
            "name": "Bertalottå Beispiel",
            "choices": [
                {"course": 6, "penalty": 0},
                {"course": 5, "penalty": 1},
                {"course": 1, "penalty": 2}
            ]
        },
        ...
    ]
}
```
The `instructors` entry of each course is a list of indices of participants in the `participants` list. In the example,
Anton (index 0) and Bertalottå (index 1) are the course instructors of "Example Course". The `choices` entry of
each participant is an ordered list of course choices of this participant, represented by the courses' index in the
`courses` list. In the example, Anton chose "Another Course" as his first choice, "Example Course" as his second choice
(which is a nonsense-example, since he is instructor of that course) and the (not shown) seventh course in the list as
his third choice.

`room_factor`, `room_offset` and `fixed_course` are optional values for each course. They default to `1.0` resp. `0.0`
resp. `false`. `room_factor` and `room_offset` are only required when course room fitting is used. They are used to
calculate the "effective size" of the course, in the sense of how big of a room the course will require with a given
number of participants, as described above.

A course with `fixed_course = true` will always take place; the algorithm is not allowed to
consider cancelling it (of course, this might impair the optimal solution's quality or even make the problem
infeasible).

The `hidden_participant_names` entry can be used to add additional entries to the result output, which are not part of
the optimization. This can be used to show attendees which are already fix-assigned (and thus removed from the input
dataset) in a pre-processing step.

The default output format of `cdecao` is a very simple JSON file, which contains the index of the course of each
participant in the order of the participants' appearance in the input file:

```
{
    "assignment": [
        0,
        0,
        1,
        0,
        ...
    ]
}
```
In this example, Anton and Bertalottå are assigned to their own course "Example Course", the third participant (not
shown above) is assigned to "Another Course", the fourth will participate in "Example Course" again.


## Building from source

To build the binary for your platform from source, you'll need the Rust compiler (`rustc`) and the Rust package manager
`cargo`. See https://www.rust-lang.org/tools/install for detailed instructions for installing rust on your platform.

If everything is setup, you can run
```sh
cargo build --release
```
to fetch all the dependencies and build a performance-optimized binary of the application. You can also run the program
directly via cargo:
```sh
cargo run --release -- --cde pa19_partial_export_event.json
```


## Development

### Project Structure

The implementation consists of several parts, that are provided as separate Rust modules:

* A generic parallelized implementation of the Branch and Bound algorithm (`bab`)
* An implementation of the hungarian algorithm (`hungarian`)
* The specialization of the Branch and Bound algorithm for calculating course assignment using the hungarian algorithm
  (`caobab`)
* Data input/output via JSON files (`io`)


### Debugging and Testing

Wide parts of the application code are covered with unit tests (`io` and room constraints are not covered yet). To run
them, execute
```sh
cargo test
```
in the project directory.

If you make changes to the code, please ensure, all the tests are still passing and your code is formatted according to
the Rust code formatter's rules. Simply run `cargo fmt` before committing your changes.
