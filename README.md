# fling — FLiNG trainer manager for Steam + Proton on Linux

Install and **auto-launch** [FLiNG](https://flingtrainer.com) game trainers on
Linux (Steam Deck, ROG Ally, Bazzite, or any desktop) with a single command.

FLiNG trainers are Windows `.exe`s that must run **inside the running game's
Proton container** to hook it. `fling` handles all of that: it finds the game
in your Steam library, downloads the matching trainer, and injects it into the
game's own sandbox — automatically, every time the game starts.

```console
$ fling auto "hollow knight"
=== fling auto: Hollow Knight (appid 367520) ===
--- step 1/2: download trainer ---
>>> Installed: ~/Trainers/367520 - Hollow Knight/Trainer.exe
--- step 2/2: ensure trainer-injection env (global) ---
>>> ACTIVE ✓ — games will auto-attach their trainer.
```

Then just launch the game from Steam. The trainer window pops up a few seconds
later. That's it.

## How it works

A Proton game runs inside an isolated **pressure-vessel container** (its own
process namespace + private wineserver). A trainer launched outside it lands in
a *different* container and can't see the game's memory. The fix:

1. **`STEAM_COMPAT_LAUNCHER_SERVICE=proton`** is exported globally via
   `~/.config/environment.d/`. This makes every Proton game open a *launcher
   service* — a door into its container, announced on D-Bus as
   `com.steampowered.App<appid>`.
2. **`fling-watch`** (a systemd user service) watches D-Bus. When a game with a
   downloaded trainer starts, it pushes the trainer through that door via
   `steam-runtime-launch-client`, so the trainer lands in the **same
   container** as the game and can hook it.

No per-game Steam launch options, no per-game config editing (which is racy on
gamescope/Gaming Mode). One global env var covers every current and future game.

## Requirements

- Steam with Proton games
- [`protontricks`](https://github.com/Matoking/protontricks) (native or Flatpak)
- `curl`, `jq`, `python3`, `file`, `busctl`, `systemctl` (all standard)
- Optional: `xdotool` + `xprop` (for Gaming Mode window focus tagging)

## Install

```bash
git clone https://github.com/<you>/fling.git
cd fling
./install.sh
```

Then activate the env once (the running Steam predates it):

- **reboot**, or
- `fling restart-steam` (bounces the gamescope Steam session)

## Usage

```
fling auto <name|appid>    # download trainer + enable injection (the one you want)
fling list                 # list installed Steam games
fling get <name|appid>     # just download the trainer
fling setup [name|appid]   # just enable the global injection env
fling run <name|appid>     # manually inject the trainer into a running game
fling installed            # games that have a trainer
fling restart-steam        # activate the env by bouncing the Steam session
fling watch                # the daemon (run by fling-watch.service)
```

Trainers are stored in `~/Trainers/"<appid> - <name>"/Trainer.exe`.

## Notes & caveats

- **Single-player only.** Never use trainers with online/multiplayer games.
- **Proton games only** — native Linux games don't run Windows trainer exes.
- Trainer slug guessing handles most titles (incl. roman-numeral variants); if a
  game isn't found, check the name on flingtrainer.com.
- Some trainers ship as `.zip` (auto-extracted) or `.rar` (extract manually
  into the game's `~/Trainers/...` folder).
- Gaming Mode: `fling` tags the trainer window with the game's Steam AppId so it
  becomes switchable via the Steam button.

## Uninstall

```bash
./uninstall.sh            # keeps downloaded trainers
./uninstall.sh --purge    # also deletes ~/Trainers
```

## License

MIT — see [LICENSE](LICENSE). Not affiliated with FLiNG, Valve, or Anthropic.
Use trainers responsibly and only in single-player games you own.
