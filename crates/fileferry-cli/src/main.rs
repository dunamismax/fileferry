use clap::Parser;
use fileferry_cli::{Cli, run_with_error_output};

fn main() {
    let cli = Cli::parse();
    let (output, exit_code) = run_with_error_output(cli);

    print!("{}", output.stdout);
    eprint!("{}", output.stderr);

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}
