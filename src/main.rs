#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    let code = lock::run();
    std::process::ExitCode::from(code)
}
