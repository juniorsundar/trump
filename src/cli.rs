use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "trump")]
#[command(about = "Transparent Remote Utility, Multiple Protocols")]
#[command(subcommand_required = true)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Connect to filesystem over ssh
    #[command(arg_required_else_help = true)]
    Ssh {
        #[arg(value_name = "USER@HOSTNAME[:PORT]")]
        target: String,
    },
}
