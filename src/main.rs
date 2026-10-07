fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(hail::run(std::env::args().collect()))
}
