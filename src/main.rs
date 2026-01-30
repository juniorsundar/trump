mod cli;
mod config;
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
        Commands::Ssh { target, identity } => {
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
    println!("Connecting to {}:{}", hostname, port);
    let tcp = TcpStream::connect(format!("{hostname}:{port}"))?;
    tcp.set_nodelay(true)?;
    let mut session = Session::new()?;
    session.set_tcp_stream(tcp);
    session.set_blocking(true);
    session.handshake()?;

    if let Some(banner) = session.banner() {
        println!("Server Banner: {}", banner);
    }

    let mut config = config::load_config().unwrap_or_else(|e| {
        eprintln!("Could not load config: {}", e);
        config::Config::default()
    });
    let config_key = format!("{}@{}:{}", user, hostname, port);

    let mut authenticated = false;
    let mut used_identity_path: Option<PathBuf> = None;

    if !authenticated && let Some(raw_path) = &identity {
        let path = raw_path.canonicalize().unwrap_or(raw_path.clone());
        println!("Trying identity file: {:?}", path);

        if session
            .userauth_pubkey_file(user, None, &path, None)
            .is_ok()
        {
            println!("Authenticated with identity file");
            authenticated = true;
            used_identity_path = Some(path);
        } else {
            println!("Identity file authentication failed.");
        }
    }

    if !authenticated && let Some(auth_data) = config.targets.get(&config_key) {
        match &auth_data.auth_type {
            config::AuthType::Password => {
                println!("Found saved password. Attempting auto-login.");
                match config::decrypt(&auth_data.secret) {
                    Ok(password) => {
                        if session.userauth_password(user, &password).is_ok() {
                            println!("Auto-login successful.");
                            authenticated = true;
                        }
                    }
                    Err(e) => eprintln!("Faild to decrypt saved password: {}", e),
                }
            }
            config::AuthType::KeyPath => {
                let path = PathBuf::from(&auth_data.secret);
                println!("Found saved identity key: {:?}", path);
                if session
                    .userauth_pubkey_file(user, None, &path, None)
                    .is_ok()
                {
                    println!("Auto-login successful.");
                    authenticated = true;
                } else {
                    println!("Saved identity key failed.")
                }
            }
        }
    }

    if !authenticated {
        println!("Trying SSH Agent.");
        if session.userauth_agent(user).is_ok() {
            println!("Authenticated with SSH Agent.");
            authenticated = true;
        } else {
            println!("SSH Agent authentication failed.")
        }
    }

    let mut password_used = String::new();
    if !authenticated {
        println!("Falling back to interactive password.");
        if !session.authenticated() {
            let password = rpassword::prompt_password("Password: ")?;
            password_used = password.clone();
            if session.userauth_password(user, &password).is_err() {
                let mut prompter = SimplePasswordPrompter {
                    password: password.clone(),
                };
                if session
                    .userauth_keyboard_interactive(user, &mut prompter)
                    .is_err()
                {
                    return Err("Authentication failed. Please check your credentials.".into());
                }
            }
        }
    }

    if !config.targets.contains_key(&config_key) {
        if let Some(path) = used_identity_path {
            println!("Login successful with identity file: {:?}", path);
            println!(
                "Do you want to save this key as default for {}? [y/N]",
                config_key
            );
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if input.trim().eq_ignore_ascii_case("y") {
                config.targets.insert(
                    config_key.clone(),
                    config::AuthData {
                        auth_type: config::AuthType::KeyPath,
                        secret: path.to_string_lossy().to_string(),
                    },
                );
                config::save_config(&config)?;
                println!("Identity key saved.");
            }
        } else if !password_used.is_empty() {
            println!("Login successful with password.");
            println!("Do you want to save this password for auto-login? [y/N]");
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if input.trim().eq_ignore_ascii_case("y") {
                let encrypted = config::encrypt(&password_used)?;
                config.targets.insert(
                    config_key,
                    config::AuthData {
                        auth_type: config::AuthType::Password,
                        secret: encrypted,
                    },
                );
                config::save_config(&config)?;
                println!("Credentials saved.");
            }
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
