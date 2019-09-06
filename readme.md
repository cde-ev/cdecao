
# CdE Course Assignment Optimization (cdecao)

This project contains a Rust implementation of the optimal course assignment algorithm for CdE events, developed by
Gabriel Guckenbiehl in his Master's thesis. This optimized implementation is mainly programmend and maintained by
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

By default the application uses a very simple json format for input of course and participant lists and output of the
calculated course assignment. To use the CdE Datenbank's partial export format instead, give the `--cde` option:
```sh
cdecao --cde --print event_export_pa19.json
```
In this case, the resulting output file can be imported in to the CdE Datenbank using the "Partial Import" feature.

The implemented course assignment algorithm includes an (experimental) extension for considering constraints on
available course rooms. To use this functionality, simple give a list of available course room sizes. Attention: This
problem might get computationally *really* complex and may not be solved within an reasonable time:
```sh
cdecao --rooms "20,20,20,10,10,10,10,10,10,8,8" --print data.json
```

If you want to see more log output (e.g. about the program's solving progress), you can set the loglevel to 'debug' or
'trace' via the `RUST_LOG` environment variable:
```sh
RUST_LOG=debug cdecao --print data.json
```
For more information, take a look at `env_logger`'s documentation: https://docs.rs/env_logger/0.6.2/env_logger/


### Data formats

The default input format for courses and participants data looks like this:
```json
{
    "courses": [
        {
            "name": "1. Example Course",
            "num_min": 5,
            "num_max": 15,
            "instructors": [0, 1],
        },
        {
            "name": "2. Another Course",
            "num_min": 6,
            "num_max": 10,
            "instructors": [5],
        },
        ...
    ],
    "participants": [
        {
            "name": "Anton Administrator",
            "choices": [1,0,6]
        },
        {
            "name": "Bertalottå Beispiel",
            "choices": [6,5,1]
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
cargo run --release -- -c export_event.json
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

The overall application code is covered with tests quite well. To run them, execute
```sh
cargo test
```
in the project directory.

If you make changes to the code, please ensure, all the tests are still passing and your code is formatted according to
the Rust code formatter's rules. Simply run `cargo fmt` before committing your changes.
