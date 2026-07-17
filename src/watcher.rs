use crate::{config::Config, error::Error, process};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    process::Command,
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceInstance {
    pub appid: u32,
    pub key: String,
}

pub fn parse_service_instances(text: &str) -> Vec<ServiceInstance> {
    let mut base = HashMap::new();
    let mut instances: HashMap<u32, Vec<u64>> = HashMap::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(service) = fields.next() else {
            continue;
        };
        let Some(rest) = service.strip_prefix("com.steampowered.App") else {
            continue;
        };
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let Ok(appid) = rest[..digits].parse::<u32>() else {
            continue;
        };
        let suffix = &rest[digits..];
        if let Some(instance) = suffix
            .strip_prefix(".Instance")
            .and_then(|v| v.parse().ok())
        {
            instances.entry(appid).or_default().push(instance);
        } else if suffix.is_empty()
            && let Some(pid) = fields.next().and_then(|v| v.parse::<u64>().ok())
        {
            base.insert(appid, pid);
        }
    }
    let mut appids: Vec<_> = base.keys().chain(instances.keys()).copied().collect();
    appids.sort_unstable();
    appids.dedup();
    let mut result = Vec::new();
    for appid in appids {
        if let Some(values) = instances.get_mut(&appid) {
            values.sort_unstable();
            values.dedup();
            result.extend(values.iter().map(|instance| ServiceInstance {
                appid,
                key: format!("{appid}:{instance}"),
            }));
        } else if let Some(pid) = base.get(&appid) {
            result.push(ServiceInstance {
                appid,
                key: format!("{appid}:{pid}"),
            });
        }
    }
    result
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Readiness {
    Ready,
    Waiting,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    None,
    Waiting(bool),
    Unavailable(bool),
    AlreadyRunning,
    LaunchReady,
    LaunchFallback,
}

#[derive(Default)]
struct InstanceState {
    claimed: bool,
    waiting_reported: bool,
    unavailable: u32,
}

pub struct WatchState {
    failure_limit: u32,
    instances: HashMap<String, InstanceState>,
    launched_appids: HashSet<u32>,
}

impl WatchState {
    pub fn new(failure_limit: u32) -> Self {
        Self {
            failure_limit: failure_limit.max(1),
            instances: HashMap::new(),
            launched_appids: HashSet::new(),
        }
    }

    pub fn observe(
        &mut self,
        key: &str,
        installed: bool,
        already_running: bool,
        readiness: Readiness,
    ) -> Decision {
        let entry = self.instances.entry(key.to_owned()).or_default();
        if entry.claimed {
            return Decision::None;
        }
        let appid = key
            .split_once(':')
            .and_then(|(value, _)| value.parse::<u32>().ok());
        if appid.is_some_and(|appid| self.launched_appids.contains(&appid)) {
            entry.claimed = true;
            return Decision::None;
        }
        if !installed {
            entry.claimed = true;
            return Decision::None;
        }
        if already_running {
            entry.claimed = true;
            return Decision::AlreadyRunning;
        }
        match readiness {
            Readiness::Ready => {
                entry.claimed = true;
                self.launched_appids.extend(appid);
                Decision::LaunchReady
            }
            Readiness::Waiting => {
                let first = !entry.waiting_reported;
                entry.waiting_reported = true;
                Decision::Waiting(first)
            }
            Readiness::Unavailable => {
                entry.unavailable = entry.unavailable.saturating_add(1);
                if entry.unavailable >= self.failure_limit {
                    entry.claimed = true;
                    self.launched_appids.extend(appid);
                    Decision::LaunchFallback
                } else {
                    Decision::Unavailable(entry.unavailable == 1)
                }
            }
        }
    }

    pub fn retire_except<'a>(&mut self, active: impl IntoIterator<Item = &'a str>) {
        let active: HashSet<&str> = active.into_iter().collect();
        self.instances
            .retain(|key, _| active.contains(key.as_str()));
        let active_appids: HashSet<u32> = self
            .instances
            .keys()
            .filter_map(|key| key.split_once(':')?.0.parse().ok())
            .collect();
        self.launched_appids
            .retain(|appid| active_appids.contains(appid));
    }
}

pub fn shortcut_gameid_from_vdf(data: &[u8], appid: u32) -> Option<u64> {
    let marker = b"\x02appid\x00";
    let needle = format!("fling run {appid}");
    let starts: Vec<_> = data
        .windows(marker.len())
        .enumerate()
        .filter_map(|(index, bytes)| (bytes == marker).then_some(index))
        .collect();
    for (position, start) in starts.iter().copied().enumerate() {
        let value = start + marker.len();
        if value + 4 > data.len() {
            continue;
        }
        let end = starts.get(position + 1).copied().unwrap_or(data.len());
        if data[value + 4..end]
            .windows(needle.len())
            .any(|bytes| bytes == needle.as_bytes())
        {
            let id = u32::from_le_bytes(data[value..value + 4].try_into().ok()?);
            return Some(((id as u64) << 32) | 0x0200_0000);
        }
    }
    None
}

pub fn session_environment(data: &[u8]) -> HashMap<String, String> {
    const ALLOWED: [&str; 5] = [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
    ];
    data.split(|byte| *byte == 0)
        .filter_map(|entry| {
            let separator = entry.iter().position(|byte| *byte == b'=')?;
            let (key, value) = (&entry[..separator], &entry[separator + 1..]);
            let key = std::str::from_utf8(key).ok()?;
            ALLOWED
                .contains(&key)
                .then(|| (key.to_owned(), String::from_utf8_lossy(value).into_owned()))
        })
        .collect()
}

pub(crate) fn steam_session_environment(config: &Config) -> HashMap<String, String> {
    let Ok(output) = Command::new("pgrep").args(["-x", "steam"]).output() else {
        return HashMap::new();
    };
    let Some(pid) = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::to_owned)
    else {
        return HashMap::new();
    };
    fs::read(config.proc_root.join(pid).join("environ"))
        .map(|data| session_environment(&data))
        .unwrap_or_default()
}

fn shortcut_gameid(config: &Config, appid: u32) -> Option<u64> {
    let users = fs::read_dir(config.steam_root.join("userdata")).ok()?;
    let mut paths: Vec<_> = users
        .flatten()
        .map(|entry| entry.path().join("config/shortcuts.vdf"))
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    paths.into_iter().find_map(|path| {
        fs::read(path)
            .ok()
            .and_then(|data| shortcut_gameid_from_vdf(&data, appid))
    })
}

fn trainer_running(appid: u32) -> bool {
    Command::new("pgrep")
        .args(["-f", &format!("Trainers/{appid} - .*Trainer\\.exe")])
        .status()
        .is_ok_and(|status| status.success())
}
fn active(appid: u32) -> bool {
    Command::new("busctl")
        .args(["--user", "list", "--no-legend"])
        .output()
        .is_ok_and(|o| {
            o.status.success()
                && parse_service_instances(&String::from_utf8_lossy(&o.stdout))
                    .iter()
                    .any(|service| service.appid == appid)
        })
}
pub fn retry(appid: u32) -> i32 {
    retry_with_environment(appid, &HashMap::new())
}

fn retry_with_environment(appid: u32, session: &HashMap<String, String>) -> i32 {
    let max = env::var("FLING_WATCH_MAX_ATTEMPTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v: &u32| *v > 0)
        .unwrap_or(3);
    let delay = env::var("FLING_WATCH_RETRY_DELAY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5.0);
    let min = env::var("FLING_WATCH_MIN_RUNTIME")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);
    let runner = env::var_os("FLING_WATCH_RUNNER");
    let mut last = 1;
    for attempt in 1..=max {
        let start = Instant::now();
        let status = if let Some(r) = &runner {
            Command::new(r)
                .arg(appid.to_string())
                .envs(session)
                .status()
        } else {
            env::current_exe()
                .map_err(std::io::Error::other)
                .and_then(|exe| {
                    Command::new(exe)
                        .args(["run", &appid.to_string()])
                        .envs(session)
                        .status()
                })
        };
        last = status.ok().and_then(|s| s.code()).unwrap_or(1);
        let elapsed = start.elapsed().as_secs();
        if last == 0 && elapsed < min && active(appid) {
            println!("trainer {appid} exited too soon ({elapsed}s < {min}s) while game is active");
            last = 75
        }
        println!("trainer {appid} exited (status {last}, attempt {attempt}/{max})");
        if last == 0 {
            return 0;
        }
        if attempt == max {
            return last;
        }
        if !active(appid) {
            println!("trainer {appid} will not retry — game launcher service ended");
            return last;
        }
        println!("trainer {appid} failed — retrying in {delay}s");
        thread::sleep(Duration::from_secs_f64(delay));
    }
    last
}
pub fn watch(config: &Config) -> Result<(), Error> {
    println!("fling watch: polling for game launcher services...");
    let failure_limit = env::var("FLING_WATCH_DETECTION_FAILURE_LIMIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value: &u32| *value > 0)
        .unwrap_or(6);
    let mut state = WatchState::new(failure_limit);
    loop {
        let out = Command::new("busctl")
            .args(["--user", "list", "--no-legend"])
            .output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let services = parse_service_instances(&text);
        for service in &services {
            let id = service.appid;
            let readiness = match process::game_ready(config, id) {
                0 => Readiness::Ready,
                1 => Readiness::Waiting,
                _ => Readiness::Unavailable,
            };
            match state.observe(
                &service.key,
                crate::steam::find_trainer(config, id).is_some(),
                trainer_running(id),
                readiness,
            ) {
                Decision::Waiting(true) => {
                    println!("game {id} launcher detected — waiting for game process")
                }
                Decision::Unavailable(true) => {
                    println!("game {id} readiness detection unavailable — retrying")
                }
                Decision::AlreadyRunning
                | Decision::None
                | Decision::Waiting(false)
                | Decision::Unavailable(false) => {}
                decision @ (Decision::LaunchReady | Decision::LaunchFallback) => {
                    if decision == Decision::LaunchFallback {
                        println!(
                            "game {id} readiness unavailable after {failure_limit} checks — launching with fallback"
                        );
                    } else {
                        println!("game {id} process ready — auto-launching trainer");
                    }
                    if let Some(gameid) = shortcut_gameid(config, id) {
                        println!("launching via Steam shortcut (gameid {gameid})");
                        let session = steam_session_environment(config);
                        let _ = Command::new("steam")
                            .arg(format!("steam://rungameid/{gameid}"))
                            .envs(&session)
                            .spawn();
                    } else {
                        println!("no Steam shortcut found — direct injection");
                        let session = steam_session_environment(config);
                        std::thread::spawn(move || {
                            retry_with_environment(id, &session);
                        });
                    }
                }
            }
        }
        state.retire_except(services.iter().map(|service| service.key.as_str()));
        thread::sleep(Duration::from_secs_f64(
            env::var("FLING_WATCH_POLL_INTERVAL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5.0),
        ));
    }
}
