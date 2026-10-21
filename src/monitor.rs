// Copyright 2026 git-next contributors. MIT license.
//! Read-only Linux telemetry collected over one SSH channel, without an agent.
use crate::remote::{self, Host, OwnedProcess};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

const SCRIPT: &str = r#"export LC_ALL=C
test -r /proc/stat -a -r /proc/meminfo || { printf '@unsupported\n'; exit 2; }
while :; do
printf '@os\n'; if test -r /etc/os-release; then . /etc/os-release; printf '%s\n' "$PRETTY_NAME"; else uname -s; fi
printf '@timezone\n'; date '+%z %Z'
printf '@uptime\n'; cat /proc/uptime
printf '@cpu\n'; cat /proc/stat
printf '@memory\n'; cat /proc/meminfo
printf '@network\n'; cat /proc/net/dev
printf '@disk\n'; for device in /sys/block/*; do case "${device##*/}" in loop*|ram*) continue;; esac; test -r "$device/stat" || continue; printf '0 0 %s ' "${device##*/}"; cat "$device/stat"; done
printf '@space\n'; df -PkP 2>/dev/null
printf '@end\n'
sleep INTERVAL
done"#;

#[derive(Clone, Default, Debug, Serialize)]
pub struct Cpu {
    pub name: String,
    pub used: Option<f64>,
    #[serde(skip)]
    total: u64,
    #[serde(skip)]
    idle: u64,
}
#[derive(Clone, Default, Debug, Serialize)]
pub struct Space {
    pub device: String,
    pub mount: String,
    pub total: u64,
    pub used: u64,
}
#[derive(Clone, Default, Debug, Serialize)]
pub struct Disk {
    pub name: String,
    pub read_per_second: Option<f64>,
    pub write_per_second: Option<f64>,
    #[serde(skip)]
    read: u64,
    #[serde(skip)]
    write: u64,
}
#[derive(Clone, Default, Debug, Serialize)]
pub struct Snapshot {
    pub os: String,
    pub timezone: String,
    pub uptime: f64,
    pub cpus: Vec<Cpu>,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_cache: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_per_second: Option<f64>,
    pub tx_per_second: Option<f64>,
    pub disks: Vec<Disk>,
    pub spaces: Vec<Space>,
}

fn value(s: Option<&str>) -> u64 {
    s.and_then(|s| s.parse().ok()).unwrap_or(0)
}

pub fn parse(text: &str, previous: Option<(&Snapshot, f64)>) -> Result<Snapshot> {
    let mut result = Snapshot::default();
    let mut section = "";
    let mut memory = BTreeMap::new();
    for line in text.lines() {
        if line.starts_with('@') {
            section = line;
            continue;
        }
        match section {
            "@os" => result.os = line.into(),
            "@timezone" => result.timezone = line.into(),
            "@uptime" => {
                result.uptime = line
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.)
            }
            "@cpu" => {
                let mut words = line.split_whitespace();
                let Some(name) = words.next().filter(|s| s.starts_with("cpu")) else {
                    continue;
                };
                let counters: Vec<u64> = words.take(8).map(|v| v.parse().unwrap_or(0)).collect();
                ensure!(counters.len() >= 4, "Incomplete CPU sample");
                let total = counters.iter().sum::<u64>();
                let idle = counters[3] + counters.get(4).copied().unwrap_or(0);
                let used = previous
                    .and_then(|(p, _)| p.cpus.iter().find(|c| c.name == name))
                    .filter(|p| total > p.total && idle >= p.idle)
                    .map(|p| {
                        (100. * (1. - (idle - p.idle) as f64 / (total - p.total) as f64))
                            .clamp(0., 100.)
                    });
                result.cpus.push(Cpu {
                    name: name.into(),
                    used,
                    total,
                    idle,
                });
            }
            "@memory" => {
                if let Some((name, v)) = line.split_once(':') {
                    memory.insert(name, value(v.split_whitespace().next()) * 1024);
                }
            }
            "@network" => {
                if let Some((name, v)) = line.split_once(':') {
                    if name.trim() == "lo" {
                        continue;
                    }
                    let counters: Vec<_> = v.split_whitespace().collect();
                    result.rx_bytes = result
                        .rx_bytes
                        .saturating_add(value(counters.first().copied()));
                    result.tx_bytes = result
                        .tx_bytes
                        .saturating_add(value(counters.get(8).copied()));
                }
            }
            "@disk" => {
                let fields: Vec<_> = line.split_whitespace().collect();
                let Some(device) = fields.get(2) else {
                    continue;
                };
                if device.starts_with("loop") || device.starts_with("ram") {
                    continue;
                }
                result.disks.push(Disk {
                    name: (*device).into(),
                    read: value(fields.get(5).copied()) * 512,
                    write: value(fields.get(9).copied()) * 512,
                    ..Disk::default()
                });
            }
            "@space" => {
                let fields: Vec<_> = line.split_whitespace().collect();
                if fields.len() < 6
                    || fields[0] == "Filesystem"
                    || fields[0] == "tmpfs"
                    || fields[0] == "shm"
                {
                    continue;
                }
                if result.spaces.len() < 32 {
                    result.spaces.push(Space {
                        device: fields[0].into(),
                        mount: fields[5..].join(" "),
                        total: value(fields.get(1).copied()) * 1024,
                        used: value(fields.get(2).copied()) * 1024,
                    });
                }
            }
            _ => {}
        }
    }
    ensure!(
        !result.cpus.is_empty() && memory.contains_key("MemTotal"),
        "Linux telemetry is unavailable or incomplete"
    );
    result.memory_total = *memory.get("MemTotal").unwrap();
    let available = memory
        .get("MemAvailable")
        .copied()
        .unwrap_or_else(|| memory.get("MemFree").copied().unwrap_or(0));
    result.memory_used = result.memory_total.saturating_sub(available);
    result.memory_cache = memory.get("Cached").copied().unwrap_or(0)
        + memory.get("SReclaimable").copied().unwrap_or(0);
    if let Some((previous, seconds)) = previous.filter(|(_, seconds)| *seconds > 0.) {
        let rate = |now: u64, old: u64| now.checked_sub(old).map(|v| v as f64 / seconds);
        result.rx_per_second = rate(result.rx_bytes, previous.rx_bytes);
        result.tx_per_second = rate(result.tx_bytes, previous.tx_bytes);
        for disk in &mut result.disks {
            if let Some(old) = previous.disks.iter().find(|old| old.name == disk.name) {
                disk.read_per_second = rate(disk.read, old.read);
                disk.write_per_second = rate(disk.write, old.write);
            }
        }
    }
    Ok(result)
}

pub fn watch(
    host: &Host,
    ssh: &Path,
    cancel: Arc<AtomicBool>,
    mut sample: impl FnMut(Snapshot) -> bool,
) -> Result<()> {
    let mut command = remote::process(host, ssh)?;
    let script = SCRIPT.replace("INTERVAL", &host.monitor_interval.clamp(1, 60).to_string());
    let quoted = format!("sh -c '{}'", script.replace('\'', "'\\''"));
    command.args(["-T", "--", &host.destination, &quoted]);
    let process = OwnedProcess::spawn(command, cancel.clone(), Duration::ZERO)?;
    let output = process.child.lock().stdout.take().unwrap();
    let mut reader = BufReader::new(output);
    let mut previous: Option<(Snapshot, Instant)> = None;
    let mut text = String::new();
    loop {
        let mut line = String::new();
        let count = reader.by_ref().take(65537).read_line(&mut line)?;
        ensure!(count != 0, "SSH monitor disconnected");
        ensure!(
            line.len() <= 65536 && text.len() + line.len() <= 2 * 1024 * 1024,
            "Oversized telemetry sample"
        );
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(());
        }
        if line.trim_end() == "@end" {
            let now = Instant::now();
            let result = parse(
                &text,
                previous
                    .as_ref()
                    .map(|(p, at)| (p, now.duration_since(*at).as_secs_f64())),
            )?;
            text.clear();
            if !sample(result.clone()) {
                return Ok(());
            }
            previous = Some((result, now));
        } else {
            ensure!(
                line.trim_end() != "@unsupported",
                "Monitoring currently requires Linux /proc; terminal and SFTP remain available"
            );
            text.push_str(&line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counters_use_deltas_and_first_sample_is_unknown() {
        let text = "@cpu\ncpu 10 0 20 70 0 0 0 0\ncpu0 10 0 20 70 0 0 0 0\n@memory\nMemTotal: 1000 kB\nMemAvailable: 400 kB\nCached: 100 kB\n@network\neth0: 1000 0 0 0 0 0 0 0 2000 0 0 0 0 0 0 0\n@disk\n8 0 sda 0 0 10 0 0 0 20 0\n";
        let first = parse(text, None).unwrap();
        assert_eq!(first.cpus[0].used, None);
        assert_eq!(first.memory_used, 600 * 1024);
        let second = parse(
            &text
                .replace("10 0 20 70", "20 0 30 150")
                .replace("eth0: 1000", "eth0: 1200"),
            Some((&first, 2.)),
        )
        .unwrap();
        assert_eq!(second.cpus[0].used.unwrap().round(), 20.);
        assert_eq!(second.rx_per_second, Some(100.));
        assert!(parse("@os\nLinux", None).is_err());
    }
}
