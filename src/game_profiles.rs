#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GameProfile {
    pub trainer_launch_delay_seconds: u64,
    pub trainer_instructions: &'static [&'static str],
}

pub const ELDEN_RING_APPID: u32 = 1_245_620;

const ELDEN_RING_INSTRUCTIONS: &[&str] = &[
    "Use Windowed mode before activating the trainer.",
    "Launch without Easy Anti-Cheat (EAC) and stay offline.",
];

const ELDEN_RING: GameProfile = GameProfile {
    trainer_launch_delay_seconds: 90,
    trainer_instructions: ELDEN_RING_INSTRUCTIONS,
};

pub fn for_appid(appid: u32) -> Option<GameProfile> {
    match appid {
        ELDEN_RING_APPID => Some(ELDEN_RING),
        _ => None,
    }
}
