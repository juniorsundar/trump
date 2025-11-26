use crate::SSHClient;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::{error::Error, io::Read, path::PathBuf};

pub fn repl(mut shell_client: SSHClient) -> Result<(), Box<dyn Error>> {
    let mut rl = DefaultEditor::new()?;
    loop {
        let readline = rl.readline(
            format!(
                "trump > {}@{}:{} > ",
                shell_client.user, shell_client.host_name, shell_client.port
            )
            .as_str(),
        );
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str())?;
                let commands = line.as_str().split(" ").collect::<Vec<&str>>();
                if commands.is_empty() {
                    continue;
                }

                match commands[0] {
                    "help" => println!("Available commands: help, cd <path>, exit"),
                    "exit" => break,
                    "cd" => {
                        let target = commands.get(1).unwrap_or(&"~");

                        let probe_cmd = format!(
                            "cd {} && cd {} && pwd",
                            shell_client.current_directory.display(),
                            target
                        );

                        let mut channel = shell_client.session.channel_session()?;
                        channel.exec(&probe_cmd)?;

                        let mut output = String::new();
                        channel.read_to_string(&mut output)?;
                        channel.wait_close()?;

                        if channel.exit_status()? == 0 {
                            let new_path = output.trim();
                            shell_client.current_directory = PathBuf::from(new_path);
                        } else {
                            eprintln!("cd: {}: No such file or directory", target);
                        }
                    }
                    "ls" => {
                        let full_cmd = format!(
                            "cd {} && {}",
                            shell_client.current_directory.display(),
                            line
                        );
                        println!("(Executing: {})", full_cmd);
                        let mut channel = shell_client.session.channel_session()?;
                        channel.exec(&full_cmd)?;

                        let mut output = String::new();
                        channel.read_to_string(&mut output)?;
                        println!("{}", output);
                        channel.wait_close()?;
                    }
                    _ => todo!(),
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }
    Ok(())
}
