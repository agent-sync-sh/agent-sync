use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let vars: Vec<(String, String)> = std::env::vars().collect();

    let stdout = io::stdout();
    let stderr = io::stderr();
    let mut out = stdout.lock();
    let mut err = stderr.lock();

    let code = agent_sync::run(&args, &vars, &mut out, &mut err);

    let _ = out.flush();
    let _ = err.flush();

    ExitCode::from(code as u8)
}
