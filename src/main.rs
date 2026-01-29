mod cli;
mod repl;

use clap::Parser;
use cli::{Cli, Commands};
use ssh2::{KeyboardInteractivePrompt, Prompt, Session};
use std::{error::Error, io::Read, net::TcpStream, path::PathBuf};

struct SimplePasswordPrompter {
    password: String,
}

impl KeyboardInteractivePrompt for SimplePasswordPrompter {
    fn prompt<'a>(
        &mut self,
        _username: &str,
        instructions: &str,
        prompts: &[Prompt<'a>],
    ) -> Vec<String> {
        println!("Debug: Instructions: {}", instructions);
        prompts
            .iter()
            .map(|p| {
                println!("Debug: Prompt: '{}' (echo: {})", p.text, p.echo);
                if p.echo {
                    String::new()
                } else {
                    self.password.clone()
                }
            })
            .collect()
    }
}

pub struct SSHClient {
    pub session: Session,
    pub host_name: String,
    pub user: String,
    pub current_directory: PathBuf,
    pub port: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();

    match args.command {
        Commands::Ssh { target } => {
            let user_hostname = target;
            let user_hostname_vect: Vec<&str> = user_hostname.split("@").collect();
            if user_hostname_vect.len() != 2 {
                eprintln!("Misformatted USER@HOSTNAME[:PORT]!");
                return Err("Incorrect SSH address formatting!".into());
            } else {
                let user = user_hostname_vect[0];
                let hostname_port: Vec<&str> = user_hostname_vect[1].split(":").collect();
                let (hostname, port) = match hostname_port.len() {
                    1 => (hostname_port[0], None),
                    2 => (hostname_port[0], Some(hostname_port[1])),
                    _ => {
                        eprintln!("Misformatted USER@HOSTNAME:PORT!");
                        return Err("Incorrect SSH address formatting!".into());
                    }
                };
                ssh_connect(user, hostname, port)?;
            }
        }
    }

    Ok(())
}

fn ssh_connect(user: &str, hostname: &str, port: Option<&str>) -> Result<(), Box<dyn Error>> {
    let password = rpassword::prompt_password("Password: ")?;

    let port = port.unwrap_or("22");
    println!("{:?}", format!("{hostname}:{port}"));
    let tcp = TcpStream::connect(format!("{hostname}:{port}"))?;
    tcp.set_nodelay(true)?;
    let mut session = Session::new()?;
    session.set_tcp_stream(tcp);
    session.set_blocking(true);
    session.handshake()?;

    if let Some(banner) = session.banner() {
        println!("Server Banner: {}", banner);
    }
    println!(
        "Authenticated immediately after handshake: {}",
        session.authenticated()
    );

    let methods = session.auth_methods(user)?;
    println!("Supported authentication methods: '{:?}'", methods);
    if methods.is_empty() {
        println!("Warning: No authentication methods returned.");
    }

    if !session.authenticated()
        && let Err(e) = session.userauth_password(user, &password)
    {
        println!("Password authentication failed: {}", e);
        let mut prompter = SimplePasswordPrompter {
            password: password.clone(),
        };
        if let Err(e_ki) = session.userauth_keyboard_interactive(user, &mut prompter) {
            println!("Keyboard-interactive authentication failed: {}", e_ki);
            println!(
                "Checking authentication state after failures: {}",
                session.authenticated()
            );
            return Err(Box::new(e_ki));
        }
    }

    let mut pwd_channel = session.channel_session()?;
    pwd_channel.exec("pwd -P")?;

    let mut raw_pwd = String::new();
    pwd_channel.read_to_string(&mut raw_pwd)?;
    pwd_channel.wait_close()?;
    println!("Initial PWD response: '{}'", raw_pwd.trim());

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
