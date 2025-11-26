use clap::{Command, arg};

pub fn cli() -> Command {
    Command::new("trump")
        .about("Transparent Remote Utility, Multiple Protocols")
        .subcommand_required(true)
        .subcommands([Command::new("ssh")
            .about("Connect to filesystem over ssh")
            .arg_required_else_help(true)
            .arg(arg!(<"USER@HOSTNAME">))])
}
