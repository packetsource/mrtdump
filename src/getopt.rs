/*
 *   Chappell's lightweight getopt() for rust. Sharp edges and no warranty.
 *
 * - Define the fields you want in the struct
 * - Define the default() implementation to return the default values
 * - Write a nice usage message
 * - modify the parse logic to suit
 *
 * - from main() call getopt::getopt() to obtain a Getopt structure from argv, OR,
 * - or use lazy_static! { static ref GETOPT: getopt::Getopt = getopt::getopt(); }
 *   for a global static. dbg!(& *GETOPT);
 */

use crate::*;

// pub const DEFAULT_INTERVAL: u64 = 60;                   // baked-in default for -i
// pub const DEFAULT_ADDRESS: &str = "0.0.0.0:80";         // baked-in default for -s
pub const DEFAULT_POSITIONAL: &str = "rib.mrt";  // default positional argument

// Define your command line arguments here: name and type
#[derive(Debug)]
pub struct Getopt {
    pub verbose: bool,
    pub juniper_output: bool,   // JUNOS style output
    pub terse_output: bool,     // pipe-separated CSV
    pub interactive: bool,  // interactive query post-load
    // pub interval: u64,
    // pub addr: String,
    pub filter: Vec<Filter>,
    pub args: Vec<String>,  // there are positional arguments
}

// Same here, setting the default values
impl Default for Getopt {
    fn default() -> Getopt {
        Getopt {
            verbose: false,
            juniper_output: false,
            terse_output: false,
            interactive: false,
            // interval: DEFAULT_INTERVAL,
            // addr: DEFAULT_ADDRESS.to_string(),
            filter: vec![],
            args: vec![],
        }
    }
}


pub fn getopt() -> Getopt {

    let mut getopt = Getopt::default();

    let mut args = env::args();

    args.next(); // blow off the first argument (it's the process name)

    /*
     * Iterate through the arguments. Check for each '-X' case of interest
     * using next()/expect() to take the next argument as required.
     * continue or break to carry on or stop respectively
     *
     * Remaining positionals end up in Getopt::args
     */
    while let Some(arg) = args.next() {

        getopt.args.push(match arg.as_str() {

            /* boolean flag example */
            "-v" => {
                getopt.verbose = true;
                // getopt.verbose = ! getopt.verbose; // toggle
                continue;
            },
            "-j" => {
                getopt.juniper_output = true;
                // getopt.verbose = ! getopt.verbose; // toggle
                continue;
            },
            "-t" => {
                getopt.terse_output = true;
                // getopt.verbose = ! getopt.verbose; // toggle
                continue;
            },

            "-f" => {
                getopt.filter.push(
                    Filter::from_str(&args
                        .next()
                        .expect("expected query expression")
                    ).expect("query expression not valid")
                );
                continue;
            },
            "-i" => {
                getopt.interactive = true;
                continue;
            },

            // usage text
            "-h" => { crate::usage(); break; },
            "-?" => { crate::usage(); break; }

            // If nothing matches, collect it up as positional
            _ => arg,
        })
    }

    // You can add an optional default positional here
    if getopt.args.len()==0 {
        getopt.args.push(String::from(DEFAULT_POSITIONAL));
    }
    getopt
}
