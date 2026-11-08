// Without this a release build on Windows opens a console window next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    gapir_lib::run()
}
