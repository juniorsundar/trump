mod cli;
mod config;
mod repl;

use clap::Parser;
use cli::{Cli, Commands};
use colored::*;
use ssh2::{KeyboardInteractivePrompt, Prompt, Session};
use std::{error::Error, io::Read, io::Write, net::TcpStream, path::PathBuf};

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
        if !instructions.is_empty() {
            println!("{} {}", "Instructions:".dimmed(), instructions);
        }

        prompts
            .iter()
            .map(|p| {
                if !p.text.is_empty() {
                    println!("{} {}", "Server Prompt:".dimmed(), p.text);
                }

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
        Commands::Ssh { target, identity } => {
            let user_hostname = target;
            let user_hostname_vect: Vec<&str> = user_hostname.split("@").collect();
            if user_hostname_vect.len() != 2 {
                eprintln!("{}", "Misformatted USER@HOSTNAME[:PORT]!".red().bold());
                return Err("Incorrect SSH address formatting!".into());
            } else {
                let user = user_hostname_vect[0];
                let hostname_port: Vec<&str> = user_hostname_vect[1].split(":").collect();
                let (hostname, port) = match hostname_port.len() {
                    1 => (hostname_port[0], None),
                    2 => (hostname_port[0], Some(hostname_port[1])),
                    _ => {
                        eprintln!("{}", "Misformatted USER@HOSTNAME:PORT!".red().bold());
                        return Err("Incorrect SSH address formatting!".into());
                    }
                };
                ssh_connect(user, hostname, port, identity)?;
            }
        }
    }

    Ok(())
}

fn ssh_connect(
    user: &str,
    hostname: &str,
    port: Option<&str>,
    identity: Option<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    let port = port.unwrap_or("22");

    let config = config::load_config().unwrap_or_else(|e| {
        eprintln!("{} {}.", "Warning: Could not load config:".yellow(), e);
        config::Config::default()
    });
    let config_key = format!("{}@{}:{}", user, hostname, port);

    let mut session_opt: Option<Session> = None;

    {
        println!("{} {}:{}.", "Connecting to".cyan(), hostname, port);
        let tcp = TcpStream::connect(format!("{hostname}:{port}"))?;
        tcp.set_nodelay(true)?;
        let mut session = Session::new()?;
        session.set_tcp_stream(tcp);
        session.set_blocking(true);
        session.handshake()?;

        if let Some(banner) = session.banner() {
            println!("{} {}", "Server Banner:".dimmed(), banner.trim());
        }

        let _ = session.auth_methods(user);

        let mut authenticated = session.authenticated();
        if authenticated {
            println!(
                "{}",
                "✔ Authenticated (No credentials required).".green().bold()
            );
        }

        if !authenticated && let Some(raw_path) = &identity {
            let path = raw_path.canonicalize().unwrap_or(raw_path.clone());
            println!("{} {:?}.", "Trying identity file:".blue(), path);
            if session
                .userauth_pubkey_file(user, None, &path, None)
                .is_ok()
            {
                println!("{}", "✔ Authenticated with identity file.".green().bold());
                authenticated = true;
            } else {
                println!("{}", "✖ Identity file authentication failed!".red());
            }
        }

        if !authenticated && let Some(auth_data) = config.targets.get(&config_key) {
            match &auth_data.auth_type {
                config::AuthType::Password => {
                    println!("{}", "Found saved password. Attempting auto-login.".cyan());
                    match config::decrypt(&auth_data.secret) {
                        Ok(password) => {
                            if session.userauth_password(user, &password).is_ok() {
                                println!("{}", "✔ Auto-login successful.".green().bold());
                                authenticated = true;
                            } else {
                                println!("{}", "✖ Saved password failed!".red());
                            }
                        }
                        Err(e) => eprintln!("{} {}!", "Failed to decrypt saved password:".red(), e),
                    }
                }
                config::AuthType::KeyPath => {
                    let path = PathBuf::from(&auth_data.secret);
                    println!("{} {:?}.", "Found saved identity key:".cyan(), path);
                    if session
                        .userauth_pubkey_file(user, None, &path, None)
                        .is_ok()
                    {
                        println!("{}", "✔ Auto-login successful.".green().bold());
                        authenticated = true;
                    } else {
                        println!("{}", "✖ Saved identity key failed!".red());
                    }
                }
            }
        }

        if authenticated {
            session_opt = Some(session);
        }
    }

    let session = if let Some(s) = session_opt {
        s
    } else {
        println!("{}", "Falling back to interactive password.".yellow());
        let password = rpassword::prompt_password("Password: ")?;

        println!("{} {}:{}.", "Reconnecting to".cyan(), hostname, port);
        let tcp = TcpStream::connect(format!("{hostname}:{port}"))?;
        tcp.set_nodelay(true)?;
        let mut session = Session::new()?;
        session.set_tcp_stream(tcp);
        session.set_blocking(true);
        session.handshake()?;

        if let Some(banner) = session.banner() {
            println!("{} {}", "Server Banner:".dimmed(), banner.trim());
        }

        let _ = session.auth_methods(user);

        if let Err(e) = session.userauth_password(user, &password) {
            println!("Password auth failed: {}.", e);
            let mut prompter = SimplePasswordPrompter {
                password: password.clone(),
            };
            if let Err(e_ki) = session.userauth_keyboard_interactive(user, &mut prompter) {
                println!("Keyboard-interactive auth failed: {}.", e_ki);
                return Err(format!(
                    "{}",
                    "Authentication failed. Please check your credentials!"
                        .red()
                        .bold()
                )
                .into());
            }
        }

        if !config.targets.contains_key(&config_key) {
            print!(
                "{} ",
                "Do you want to save this password for auto-login? [y/N]"
                    .yellow()
                    .bold()
            );
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if input.trim().eq_ignore_ascii_case("y")
                && let Ok(encrypted) = config::encrypt(&password)
            {
                let mut new_config = config;
                new_config.targets.insert(
                    config_key,
                    config::AuthData {
                        auth_type: config::AuthType::Password,
                        secret: encrypted,
                    },
                );
                config::save_config(&new_config)?;
                println!("{}", "✔ Credentials saved.".green().bold());
            }
        }

        session
    };

    let mut pwd_channel = session.channel_session()?;

    pwd_channel.exec("pwd -P")?;

    let mut raw_pwd = String::new();
    pwd_channel.read_to_string(&mut raw_pwd)?;
    pwd_channel.wait_close()?;
    // println!("Initial PWD response: '{}'", raw_pwd.trim()); // Hiding debug output for cleaner UI

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
