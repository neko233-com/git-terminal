// Copyright 2026 git-next contributors. MIT license.
use crate::{
    monitor,
    remote::{self, Host},
    sftp::Sftp,
};
use anyhow::{Context, Result, ensure};
use clap::Subcommand;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

#[derive(Clone, Subcommand)]
pub enum RemoteAction {
    Hosts,
    Add {
        name: String,
        destination: String,
        #[arg(long, default_value = "")]
        group: String,
        #[arg(long, default_value = "")]
        user: String,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long, default_value = "")]
        identity: String,
        #[arg(long, default_value = "")]
        config: String,
    },
    Remove {
        name: String,
    },
    Import {
        path: PathBuf,
    },
    List {
        host: String,
        #[arg(default_value = ".")]
        path: String,
    },
    Get {
        host: String,
        remote: String,
        local: PathBuf,
        #[arg(long)]
        overwrite: bool,
    },
    Put {
        host: String,
        local: PathBuf,
        remote: String,
        #[arg(long)]
        overwrite: bool,
    },
    Monitor {
        host: String,
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(1..=60))]
        count: u32,
    },
}

pub fn resolve(name: &str) -> Result<Host> {
    remote::load_hosts()?
        .into_iter()
        .find(|host| host.name == name)
        .context("SSH host is not configured")
}
pub fn run(action: &RemoteAction, git: Option<&Path>) -> Result<()> {
    match action {
        RemoteAction::Hosts => println!("{}", serde_json::to_string(&remote::load_hosts()?)?),
        RemoteAction::Add {
            name,
            destination,
            group,
            user,
            port,
            identity,
            config,
        } => {
            let host = Host {
                name: name.clone(),
                destination: destination.clone(),
                group: group.clone(),
                user: user.clone(),
                port: *port,
                identity: identity.clone(),
                config: config.clone(),
                monitor_interval: 2,
            };
            let mut hosts = remote::load_hosts()?;
            ensure!(
                !hosts.iter().any(|h| h.name == *name),
                "SSH host already exists"
            );
            hosts.push(host);
            remote::save_hosts(&hosts)?;
            println!("{{\"saved\":true}}");
        }
        RemoteAction::Remove { name } => {
            let mut hosts = remote::load_hosts()?;
            let before = hosts.len();
            hosts.retain(|h| h.name != *name);
            ensure!(before != hosts.len(), "SSH host is not configured");
            remote::save_hosts(&hosts)?;
            println!("{{\"removed\":true}}");
        }
        RemoteAction::Import { path } => {
            let mut hosts = remote::load_hosts()?;
            let before = hosts.len();
            for host in remote::import_config(path)? {
                if !hosts.iter().any(|h| h.name == host.name) {
                    hosts.push(host);
                }
            }
            remote::save_hosts(&hosts)?;
            println!("{}", serde_json::json!({"imported":hosts.len()-before}));
        }
        RemoteAction::Monitor { host, count } => {
            let host = resolve(host)?;
            let ssh = remote::ssh_executable(git)?;
            let mut received = 0;
            monitor::watch(&host, &ssh, Arc::new(AtomicBool::new(false)), |snapshot| {
                println!("{}", serde_json::to_string(&snapshot).unwrap());
                received += 1;
                received < *count
            })?;
        }
        action => {
            let name = match action {
                RemoteAction::List { host, .. }
                | RemoteAction::Get { host, .. }
                | RemoteAction::Put { host, .. } => host,
                _ => unreachable!(),
            };
            let ssh = remote::ssh_executable(git)?;
            let mut client =
                Sftp::connect(&resolve(name)?, &ssh, Arc::new(AtomicBool::new(false)))?;
            let mut bytes = 0u64;
            match action {
                RemoteAction::List { path, .. } => {
                    let path = if path == "." {
                        client.home()?
                    } else {
                        path.clone()
                    };
                    println!(
                        "{}",
                        serde_json::json!({"path":path,"entries":client.list(&path)?})
                    );
                }
                RemoteAction::Get {
                    remote,
                    local,
                    overwrite,
                    ..
                } => {
                    client.download(remote, local, *overwrite, &mut |n| bytes += n)?;
                    println!("{}", serde_json::json!({"downloaded":true,"bytes":bytes}));
                }
                RemoteAction::Put {
                    remote,
                    local,
                    overwrite,
                    ..
                } => {
                    client.upload(local, remote, *overwrite, &mut |n| bytes += n)?;
                    println!("{}", serde_json::json!({"uploaded":true,"bytes":bytes}));
                }
                _ => unreachable!(),
            }
        }
    }
    Ok(())
}
