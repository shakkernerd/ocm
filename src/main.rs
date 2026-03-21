use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;

use ocm::cli::Cli;

fn main() {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let env_map = env::vars().collect::<BTreeMap<_, _>>();
    let args = env::args().skip(1).collect::<Vec<_>>();

    let cli = Cli { env: env_map, cwd };
    std::process::exit(cli.run(args));
}
