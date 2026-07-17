use crate::{config::Config, error::Error, game_profiles, process};
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
    Delaying { first: bool, remaining: Duration },
    AlreadyRunning,
    LaunchReady,
    LaunchFallback,
}

#[derive(Default)]
struct InstanceState {
    claimed: bool,
    waiting_reported: bool,
    delay_reported: bool,
    ready_at: Option<Duration>,
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
        self.observe_at(
            key,
            installed,
            already_running,
            readiness,
            Duration::ZERO,
            Duration::ZERO,
        )
    }

    pub fn observe_at(
        &mut self,
        key: &str,
        installed: bool,
        already_running: bool,
        readiness: Readiness,
        now: Duration,
        launch_delay: Duration,
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
                entry.unavailable = 0;
                if !launch_delay.is_zero() {
                    let ready_at = *entry.ready_at.get_or_insert(now);
                    let elapsed = now.saturating_sub(ready_at);
                    if elapsed < launch_delay {
                        let first = !entry.delay_reported;
                        entry.delay_reported = true;
                        return Decision::Delaying {
                            first,
                            remaining: launch_delay.saturating_sub(elapsed),
                        };
                    }
                }
                entry.claimed = true;
                self.launched_appids.extend(appid);
                Decision::LaunchReady
            }
            Readiness::Waiting => {
                entry.ready_at = None;
                let first = !entry.waiting_reported;
                entry.waiting_reported = true;
                Decision::Waiting(first)
            }
            Readiness::Unavailable => {
                entry.ready_at = None;
                entry.unavailable = entry.unavailable.saturating_add(1);
                if launch_delay.is_zero() && entry.unavailable >= self.failure_limit {
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
        let shortcut = &data[value + 4..end];
        if shortcut
            .windows(needle.len())
            .enumerate()
            .any(|(index, bytes)| {
                bytes == needle.as_bytes()
                    && shortcut
                        .get(index + needle.len())
                        .is_none_or(|next| !next.is_ascii_digit())
            })
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
    steam_session_environment_for_pids(config, &output.stdout)
}

pub fn steam_session_environment_for_pids(config: &Config, pids: &[u8]) -> HashMap<String, String> {
    String::from_utf8_lossy(pids)
        .lines()
        .filter_map(|pid| fs::read(config.proc_root.join(pid).join("environ")).ok())
        .map(|data| session_environment(&data))
        .find(|environment| !environment.is_empty())
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

pub fn retry_delay_from(value: Option<&str>) -> f64 {
    const MAX_DELAY_SECONDS: f64 = 86_400.0;
    value
        .and_then(|value| value.parse().ok())
        .filter(|value: &f64| value.is_finite() && (0.0..=MAX_DELAY_SECONDS).contains(value))
        .unwrap_or(5.0)
}

pub fn poll_interval_from(value: Option<&str>) -> f64 {
    const MAX_INTERVAL_SECONDS: f64 = 86_400.0;
    value
        .and_then(|value| value.parse().ok())
        .filter(|value: &f64| {
            value.is_finite() && (f64::MIN_POSITIVE..=MAX_INTERVAL_SECONDS).contains(value)
        })
        .unwrap_or(5.0)
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
    let delay_value = env::var("FLING_WATCH_RETRY_DELAY").ok();
    let delay = retry_delay_from(delay_value.as_deref());
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
    let watch_started = Instant::now();
    loop {
        let interval = env::var("FLING_WATCH_POLL_INTERVAL").ok();
        let mut sleep_for = Duration::from_secs_f64(poll_interval_from(interval.as_deref()));
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
            let observed_at = watch_started.elapsed();
            let launch_delay = game_profiles::for_appid(id)
                .map(|profile| Duration::from_secs(profile.trainer_launch_delay_seconds))
                .unwrap_or_default();
            match state.observe_at(
                &service.key,
                crate::steam::find_trainer(config, id).is_some(),
                trainer_running(id),
                readiness,
                observed_at,
                launch_delay,
            ) {
                Decision::Waiting(true) => {
                    println!("game {id} launcher detected — waiting for game process")
                }
                Decision::Unavailable(true) => {
                    println!("game {id} readiness detection unavailable — retrying")
                }
                Decision::Delaying { first, remaining } => {
                    if first {
                        println!(
                            "game {id} has special settings — waiting {}s after readiness before trainer launch",
                            launch_delay.as_secs()
                        );
                    }
                    sleep_for = sleep_for.min(remaining);
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
        thread::sleep(sleep_for);
    }
}
