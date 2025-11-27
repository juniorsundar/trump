use crate::SSHClient;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::{collections::HashMap, error::Error, io::Read, path::PathBuf};

type ReplResult = Result<(), Box<dyn Error>>;
type CommandFunc = fn(&mut SSHClient, &mut DefaultEditor, &[&str]) -> ReplResult;

struct ReplCommand {
    name: String,
    description: String,
    function: CommandFunc,
}

fn get_commands() -> HashMap<String, ReplCommand> {
    let mut commands = HashMap::new();

    commands.insert(
        "list".to_string(),
        ReplCommand {
            name: "list".to_string(),
            description: "Aliases to remote 'ls'".to_string(),
            function: |client, _, args| {
                let cmd = format!("ls {}", args.join(" "));
                run_remote_command(client, &cmd)
            },
        },
    );

    commands.insert(
        "cwd".to_string(),
        ReplCommand {
            name: "cwd".to_string(),
            description: "Prints remote working directory".to_string(),
            function: |client, _, _| run_remote_command(client, "pwd"),
        },
    );

    commands.insert(
        "cd".to_string(),
        ReplCommand {
            name: "cd".to_string(),
            description: "Change directory".to_string(),
            function: |client, _, args| {
                let target = args.first().unwrap_or(&"~");
                println!("Changing dir to {}", target);
                client.current_directory = PathBuf::from(target);
                Ok(())
            },
        },
    );

    commands
}

fn run_remote_command(client: &mut SSHClient, cmd: &str) -> ReplResult {
    let full_cmd = format!("cd {} && {}", client.current_directory.display(), cmd);
    // println!("(Executing: {})", full_cmd);

    let mut channel = client.session.channel_session()?;
    channel.exec(&full_cmd)?;
    let mut output = String::new();
    Read::read_to_string(&mut channel, &mut output)?;
    println!("{}", output);
    channel.wait_close()?;
    Ok(())
}

pub fn repl(mut shell_client: SSHClient) -> Result<(), Box<dyn Error>> {
    let mut rl = DefaultEditor::new()?;
    let commands = get_commands();

    loop {
        let prompt = format!(
            "trump > {}@{}:{} > ",
            shell_client.user, shell_client.host_name, shell_client.port
        );
        let readline = rl.readline(prompt.as_str());

        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str())?;
                let parts: Vec<&str> = line.split_whitespace().collect();

                let cmd_name = parts[0];
                let args = &parts[1..];

                match cmd_name {
                    "exit" => break,
                    "help" => {
                        println!("\n--- TRUMP Commands ---");
                        for cmd in commands.values() {
                            println!("  {:<8}: {}", cmd.name, cmd.description);
                        }
                    }
                    _ => {
                        if let Some(command) = commands.get(cmd_name) {
                            if let Err(e) = (command.function)(&mut shell_client, &mut rl, args) {
                                eprintln!("Command Error: {}", e);
                            }
                        } else if let Some(stripped_prefix) = cmd_name.strip_prefix("!") {
                            run_remote_command(
                                &mut shell_client,
                                format!("{} {}", stripped_prefix, &args.join(" ")).as_str(),
                            )?;
                        } else {
                            println!("Unknown command. Try 'help'");
                        }
                    }
                }
            }
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => break,
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }
    Ok(())
}
