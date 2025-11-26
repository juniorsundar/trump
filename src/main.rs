mod cli;
mod repl;

use ssh2::Session;
use std::{error::Error, io::Read, net::TcpStream, path::PathBuf};

pub struct SSHClient {
    pub session: Session,
    pub host_name: String,
    pub user: String,
    pub current_directory: PathBuf,
    pub port: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let matches = cli::cli().get_matches();

    match matches.subcommand() {
        Some(("ssh", sub_matches)) => {
            let user_hostname = sub_matches
                .get_one::<String>("USER@HOSTNAME")
                .expect("required");
            let user_hostname_vect: Vec<&str> = user_hostname.split("@").collect();
            if user_hostname_vect.len() != 2 {
                panic!("Incorrect SSH address: {}", user_hostname)
            } else {
                ssh_connect(user_hostname_vect[0], user_hostname_vect[1])?;
            }
        }
        _ => unreachable!(),
    }

    Ok(())
}

fn ssh_connect(user: &str, hostname: &str) -> Result<(), Box<dyn Error>> {
    let port = "22";
    let tcp = TcpStream::connect(format!("{hostname}:{port}"))?;
    let mut session = Session::new()?;
    session.set_tcp_stream(tcp);
    session.handshake()?;

    let password = rpassword::prompt_password("Password: ")?;
    session.userauth_password(user, &password)?;

    let mut pwd_channel = session.channel_session()?;
    pwd_channel.exec("pwd -P")?;

    let mut raw_pwd = String::new();
    pwd_channel.read_to_string(&mut raw_pwd)?;
    pwd_channel.wait_close()?;

    let cwd = PathBuf::from(raw_pwd.trim());

    let client = SSHClient {
        session,
        host_name: hostname.to_string(),
        user: user.to_string(),
        port: port.to_string(),
        current_directory: cwd,
    };

    repl::repl(client)?;

    Ok(())
}
