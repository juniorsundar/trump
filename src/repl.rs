use crate::SSHClient;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::{
    collections::HashMap,
    env,
    error::Error,
    fs,
    io::{self, Read},
    path::PathBuf,
    process::Command,
    time::SystemTime,
};

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
                let cmd = format!("ls {} -lah", args.join(" "));
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

                let new_path = if target.starts_with("/") || target.starts_with("~") {
                    PathBuf::from(target)
                } else {
                    client.current_directory.join(target)
                };

                let cmd = format!("cd \"{}\" && pwd", new_path.display());
                let mut channel = client.session.channel_session()?;
                channel.exec(&cmd)?;

                let mut output = String::new();
                Read::read_to_string(&mut channel, &mut output)?;
                channel.wait_close()?;

                let exit_status = channel.exit_status()?;
                if exit_status != 0 {
                    let mut stderr = String::new();
                    channel.stderr().read_to_string(&mut stderr)?;
                    eprintln!("Error changing directory: {}", stderr.trim());
                } else {
                    let resolved_path = output.trim();
                    if !resolved_path.is_empty() {
                        println!("Changed dir to {}", resolved_path);
                        client.current_directory = PathBuf::from(resolved_path);
                    }
                }
                Ok(())
            },
        },
    );

    commands.insert(
        "edit".to_string(),
        ReplCommand {
            name: "edit".to_string(),
            description: "Edit locally".to_string(),
            function: cmd_edit,
        },
    );
    commands
}

fn cmd_edit(client: &mut SSHClient, rl: &mut DefaultEditor, args: &[&str]) -> ReplResult {
    // 1. Resolve Target
    let target = if let Some(arg) = args.first() {
        arg.to_string()
    } else {
        let input = rl.readline("File/Dir to edit [default: .]: ")?;
        if input.trim().is_empty() {
            ".".to_string()
        } else {
            input.trim().to_string()
        }
    };

    let remote_path = client.current_directory.join(&target);
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_nanos();
    let temp_base = env::temp_dir().join(format!("trump_edit_{}", timestamp));
    fs::create_dir_all(&temp_base)?;

    let local_name = if target == "." {
        "cwd".to_string()
    } else {
        PathBuf::from(&target)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    };
    let local_path = temp_base.join(&local_name);

    println!("Fetching {}...", remote_path.display());

    // Check Remote Type (File vs Dir)
    let sftp = client.session.sftp()?;
    let file_stat = sftp.stat(&remote_path)?;
    let is_dir = file_stat.is_dir();

    if is_dir {
        // Directory: Use remote tar -> local tar
        // Remote: tar -cf - -C <parent> <dirname>
        let (parent, dirname) = if target == "." {
            (remote_path.clone(), ".".to_string())
        } else {
            (
                remote_path
                    .parent()
                    .unwrap_or(&PathBuf::from("."))
                    .to_path_buf(),
                remote_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            )
        };

        fs::create_dir_all(&local_path)?;

        let cmd_str = format!("tar -cf - -C {} {}", parent.display(), dirname);
        let mut channel = client.session.channel_session()?;
        channel.exec(&cmd_str)?;

        // Spawn local tar to extract reading from channel stdout
        let mut child = Command::new("tar")
            .arg("-xf")
            .arg("-")
            .arg("-C")
            .arg(if target == "." {
                &local_path
            } else {
                &temp_base
            })
            .arg("--strip-components=0")
            .stdin(std::process::Stdio::piped())
            .spawn()?;

        let mut stdin = child.stdin.take().expect("Failed to open stdin");
        io::copy(&mut channel, &mut stdin)?;
        child.wait()?;
        channel.wait_close()?;
    } else {
        let (mut remote_file, _stat) = client.session.scp_recv(&remote_path)?;
        let mut local_file = fs::File::create(&local_path)?;
        io::copy(&mut remote_file, &mut local_file)?;
    }

    let editor = env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    println!("Opening in {}...", editor);

    let edit_target = local_path.clone();

    let status = Command::new(&editor).arg(&edit_target).status()?;
    if !status.success() {
        eprintln!("Editor exited with error");
    }

    // Upload (Copy Back)
    println!("Syncing back...");
    if is_dir {
        let (local_parent, local_dirname, remote_dest) = if target == "." {
            (
                local_path.clone(),
                ".".to_string(),
                client.current_directory.clone(),
            )
        } else {
            (
                temp_base.clone(),
                local_name.clone(),
                remote_path.parent().unwrap().to_path_buf(),
            )
        };

        // Tar local
        let mut tar_cmd = Command::new("tar")
            .arg("-cf")
            .arg("-")
            .arg("-C")
            .arg(&local_parent)
            .arg(&local_dirname)
            .stdout(std::process::Stdio::piped())
            .spawn()?;

        let mut tar_stdout = tar_cmd.stdout.take().expect("Failed to take stdout");

        // Remote extract
        let remote_tar_cmd = format!("tar -xf - -C {} --overwrite", remote_dest.display());
        let mut channel = client.session.channel_session()?;
        channel.exec(&remote_tar_cmd)?;

        io::copy(&mut tar_stdout, &mut channel)?;
        channel.send_eof()?;

        let mut output = String::new();
        channel.read_to_string(&mut output)?;
        if !output.is_empty() {
            eprintln!("Remote tar output: {}", output);
        }

        channel.wait_close()?;
        tar_cmd.wait()?;
    } else {
        // File: scp
        let mut local_file = fs::File::open(&local_path)?;
        let metadata = fs::metadata(&local_path)?;
        let mut remote_file = client
            .session
            .scp_send(&remote_path, 0o644, metadata.len(), None)?;
        io::copy(&mut local_file, &mut remote_file)?;
    }

    fs::remove_dir_all(&temp_base).ok();
    println!("Done.");

    Ok(())
}

fn run_remote_command(client: &mut SSHClient, cmd: &str) -> ReplResult {
    let full_cmd = format!("cd \"{}\" && {}", client.current_directory.display(), cmd);
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
