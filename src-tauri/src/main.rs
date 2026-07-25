// Keep the console window from appearing behind the interface on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    audiomirror_lib::run();
}
