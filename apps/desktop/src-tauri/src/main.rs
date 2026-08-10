//! Desktop binary entrypoint.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    oikonomia_lib::run();
}
