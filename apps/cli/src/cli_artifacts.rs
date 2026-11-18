//! On-demand artifacts generated from the `bot` command definitions.

use std::io::{self, Write};
use std::path::Path;

use clap::CommandFactory;
use clap_complete::{Generator, Shell};

use crate::Cli;

pub(crate) fn completions(shell: Shell) -> io::Result<()> {
    let mut command = Cli::command().bin_name("bot");
    command.build();

    let mut stdout = io::stdout().lock();
    shell.try_generate(&command, &mut stdout)?;
    stdout.flush()
}

pub(crate) fn man_pages(output_dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(output_dir)?;
    clap_mangen::generate_to(Cli::command(), output_dir)
}
