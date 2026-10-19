mod help;

use bootleg_smuggler::{
    config::{self, Overrides, Startup},
    network, process,
};
use std::{ffi::OsString, time::Duration};

fn main() {
    let code = match start() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("smuggler: {message}");
            78
        }
    };
    std::process::exit(code);
}
fn start() -> Result<i32, &'static str> {
    let mut arguments = std::env::args_os().skip(1);
    let mut flags = Overrides::default();
    let mut command = Vec::<OsString>::new();
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--") => {
                command.extend(arguments);
                break;
            }
            Some("--name") => {
                flags.name = Some(
                    arguments
                        .next()
                        .and_then(|s| s.into_string().ok())
                        .ok_or("--name needs a value")?,
                )
            }
            Some("--name-env") => {
                flags.name_env = Some(
                    arguments
                        .next()
                        .and_then(|s| s.into_string().ok())
                        .ok_or("--name-env needs a value")?,
                )
            }
            Some("--help") => {
                help::print(false);
                return Ok(0);
            }
            _ => return Err("expected --name, --name-env or -- COMMAND"),
        }
    }
    if command.is_empty() {
        help::print(true);
        return Ok(78);
    }
    let env = std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .collect();
    let hostname = hostname::get().ok().and_then(|s| s.into_string().ok());
    let startup = config::resolve(
        &env,
        &flags,
        hostname.as_deref(),
        bootleg_core::ContainerId::new(),
    )?;
    match startup {
        Startup::ExecThrough => {
            let error = process::exec_through(&command);
            eprintln!("smuggler: cannot execute application ({:?})", error.kind());
            Ok(if error.kind() == std::io::ErrorKind::NotFound {
                127
            } else {
                126
            })
        }
        Startup::Participate(configuration) => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|_| "cannot initialize runtime")?;
            runtime.block_on(async move {
                let supervisor = match process::Supervisor::launch(&command) {
                    Ok(supervisor) => supervisor,
                    Err(error) => {
                        eprintln!("smuggler: cannot launch application ({:?})", error.kind());
                        return Ok(if error.kind() == std::io::ErrorKind::NotFound {
                            127
                        } else {
                            126
                        });
                    }
                };
                let (shutdown, receiver) = tokio::sync::watch::channel(false);
                let mut connection = tokio::spawn(network::run(*configuration, receiver));
                let result = supervisor
                    .wait()
                    .await
                    .map_err(|_| "application supervision failed");
                let _ = shutdown.send(true);
                if tokio::time::timeout(Duration::from_millis(300), &mut connection)
                    .await
                    .is_err()
                {
                    connection.abort();
                }
                result
            })
        }
    }
}
