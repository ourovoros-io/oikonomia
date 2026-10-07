//! Desktop binary entrypoint.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Runs the app; everything lives in the library so that it can be tested.
fn main() {
    oikonomia_lib::run();
}
