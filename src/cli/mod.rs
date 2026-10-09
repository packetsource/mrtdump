//! The router-style command line shared by every front end.
//!
//! * `commands`: grammar, parsing, completion and `?` help
//! * `editor`:   readline-style line editing for terminals
//! * `pager`:    `--More--` state and keys
//! * `sink`:     bridge from synchronous handlers to the async session
//! * `handlers`: query handlers and the MOTD
//! * `shell`:    the session task tying the above together, over a `Transport`
//! * `local`:    the transport for the local terminal (`mrtdump -i`)
//!
//! The SSH server (`crate::sshserver`) provides the other transport.

pub mod commands;
pub mod editor;
pub mod handlers;
pub mod local;
pub mod pager;
pub mod shell;
pub mod sink;
