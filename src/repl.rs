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
                let cmd = format!(
                    "ls \"{}\" -lah {}",
                    client.current_directory.display(),
                    args.join(" ")
                );
                run_remote_command(client, &cmd)
            },
        },
    );

    commands.insert(
        "cat".to_string(),
        ReplCommand {
            name: "cat".to_string(),
            description: "Print file content".to_string(),
            function: |client, _, args| {
                if args.is_empty() {
                    eprintln!("Usage: cat <file>");
                    return Ok(());
                }
                let target = args[0];
                let path = client.current_directory.join(target);
                let cmd = format!("cat \"{}\"", path.display());
                run_remote_command(client, &cmd)
            },
        },
    );

    commands.insert(
        "cwd".to_string(),
        ReplCommand {
            name: "cwd".to_string(),
            description: "Prints effective working directory".to_string(),
            function: |client, _, _| {
                println!("{}", client.current_directory.display());
                Ok(())
            },
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
                let mut stderr = String::new();
                channel.read_to_string(&mut output)?;
                channel.stderr().read_to_string(&mut stderr)?;
                channel.wait_close()?;

                let exit_status = channel.exit_status()?;

                if exit_status == 0 && !output.trim().is_empty() {
                    let resolved_path = output.trim();
                    println!("Changed dir to {}", resolved_path);
                    client.current_directory = PathBuf::from(resolved_path);
                    return Ok(());
                }

                // Fallback for servers without 'cd' binary
                let combined_out = format!("{}{}", output, stderr);
                if combined_out.contains("exec: \"cd\"")
                    || combined_out.contains("cd: command not found")
                {
                    // Try SFTP first
                    if let Ok(sftp) = client.session.sftp() {
                        // Note: sftp.stat might fail for "~" if not expanded
                        if let Ok(stat) = sftp.stat(&new_path) {
                            if stat.is_dir() {
                                println!("(Local) Changed dir to {}", new_path.display());
                                client.current_directory = new_path;
                                return Ok(());
                            } else {
                                eprintln!("Error: Not a directory");
                                return Ok(());
                            }
                        }
                    }

                    // Fallback: ls -d
                    let ls_cmd = format!("ls -d \"{}\"", new_path.display());
                    let mut ls_channel = client.session.channel_session()?;
                    ls_channel.exec(&ls_cmd)?;

                    let mut tmp = String::new();
                    ls_channel.read_to_string(&mut tmp)?;

                    ls_channel.wait_close()?;
                    if ls_channel.exit_status()? == 0 {
                        println!("(Local-Force) Changed dir to {}", new_path.display());
                        client.current_directory = new_path;
                        return Ok(());
                    }
                }

                if !stderr.is_empty() {
                    eprintln!("Error changing directory: {}", stderr.trim());
                } else if !output.trim().is_empty() {
                    eprintln!("Error changing directory: {}", output.trim());
                } else {
                    eprintln!(
                        "Error changing directory: Unknown error (exit status {})",
                        exit_status
                    );
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

    commands.insert(
        "copy".to_string(),
        ReplCommand {
            name: "copy".to_string(),
            description: "Copy file/folder to local filesystem".to_string(),
            function: cmd_copy,
        },
    );

    commands
}

fn fetch_remote_resource(
    client: &mut SSHClient,
    remote_path: &PathBuf,
    local_path: &PathBuf,
) -> Result<bool, Box<dyn Error>> {
    println!("Fetching {}...", remote_path.display());

    let is_dir = if let Ok(sftp) = client.session.sftp() {
        match sftp.stat(remote_path) {
            Ok(file_stat) => file_stat.is_dir(),
            Err(_) => false,
        }
    } else {
        let cmd = format!("ls -ld \"{}\"", remote_path.display());
        let mut channel = client.session.channel_session()?;
        channel.exec(&cmd)?;
        let mut output = String::new();
        channel.read_to_string(&mut output)?;
        channel.wait_close()?;
        output.trim().starts_with('d')
    };

    if is_dir {
        // Directory: Use remote tar -> local tar
        // Remote: tar -cf - -C <parent> <dirname>
        let (parent, dirname) = if remote_path == &client.current_directory {
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

        fs::create_dir_all(local_path)?;

        let cmd_str = format!("tar -cf - -C {} {}", parent.display(), dirname);
        let mut channel = client.session.channel_session()?;
        channel.exec(&cmd_str)?;

        // Spawn local tar to extract reading from channel stdout
        let mut child = Command::new("tar")
            .arg("-xf")
            .arg("-")
            .arg("-C")
            .arg(local_path)
            .arg("--strip-components=1")
            .stdin(std::process::Stdio::piped())
            .spawn()?;

        let mut stdin = child.stdin.take().expect("Failed to open stdin");
        io::copy(&mut channel, &mut stdin)?;
        child.wait()?;
        channel.wait_close()?;
    } else {
        let (mut remote_file, _stat) = client.session.scp_recv(remote_path)?;
        let mut local_file = fs::File::create(local_path)?;
        io::copy(&mut remote_file, &mut local_file)?;
    }

    Ok(is_dir)
}

fn cmd_copy(client: &mut SSHClient, rl: &mut DefaultEditor, args: &[&str]) -> ReplResult {
    let target = if let Some(arg) = args.first() {
        arg.to_string()
    } else {
        let input = rl.readline("File/Dir to copy [default: .]: ")?;
        if input.trim().is_empty() {
            ".".to_string()
        } else {
            input.trim().to_string()
        }
    };

    let local_cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let local_cwd_str = local_cwd.to_string_lossy();

    let destination = if let Some(arg) = args.get(1) {
        arg.to_string()
    } else {
        let input = rl.readline(&format!(
            "Destination directory [default: {}]: ",
            local_cwd_str
        ))?;
        if input.trim().is_empty() {
            local_cwd_str.to_string()
        } else {
            input.trim().to_string()
        }
    };

    let remote_path = client.current_directory.join(&target);
    let local_path = PathBuf::from(&destination).join(remote_path.file_name().unwrap_or_default());

    fetch_remote_resource(client, &remote_path, &local_path)?;
    println!("Copied to {}", local_path.display());

    Ok(())
}

fn cmd_edit(client: &mut SSHClient, rl: &mut DefaultEditor, args: &[&str]) -> ReplResult {
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

    let is_dir = fetch_remote_resource(client, &remote_path, &local_path)?;

    let editor = env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    println!("Opening in {}...", editor);

    let edit_target = local_path.clone();

    let status = Command::new(&editor).arg(&edit_target).status()?;
    if !status.success() {
        eprintln!("Editor exited with error");
    }

    println!("Sync changes? [y/n]: ");
    let mut response = String::new();
    std::io::stdin()
        .read_line(&mut response)
        .expect("Failed to get input");

    if response.trim().eq_ignore_ascii_case("n") {
        println!("Not syncing changes.");
        return Ok(());
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

    let mut channel = client.session.channel_session()?;
    channel.exec(&full_cmd)?;
    let mut output = String::new();
    let mut stderr = String::new();

    // Read both stdout and stderr
    channel.read_to_string(&mut output)?;
    channel.stderr().read_to_string(&mut stderr)?;
    channel.wait_close()?;

    // Check for "cd" executable error
    let combined_output = format!("{}{}", output, stderr);
    if combined_output.contains("exec: \"cd\": executable file not found")
        || combined_output.contains("cd: command not found")
    {
        // Fallback: run command directly without cd prefix
        // println!("(Server doesn't support 'cd', running raw command)");
        let mut channel_retry = client.session.channel_session()?;
        channel_retry.exec(cmd)?;

        let mut output_retry = String::new();
        channel_retry.read_to_string(&mut output_retry)?;
        let mut stderr_retry = String::new();
        channel_retry.stderr().read_to_string(&mut stderr_retry)?;

        println!("{}", output_retry);
        if !stderr_retry.is_empty() {
            eprint!("{}", stderr_retry);
        }
        channel_retry.wait_close()?;
    } else {
        println!("{}", output);
        if !stderr.is_empty() {
            eprint!("{}", stderr);
        }
    }

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
                if parts.is_empty() {
                    continue;
                }

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
