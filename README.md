![Rust](https://img.shields.io/badge/rust-1.98-orange)
![License](https://img.shields.io/badge/license-GPL--3.0-blue)


<p align="center">
  <img src="assets/logo.svg" width="120" alt="logo"><br>
</p>
<h1 align="center">PushMC</h1>

A fast, single-binary CLI for setting up and running Minecraft servers. One command: download the core, accept the EULA, write `server.properties`, pick Java, start the server.

## Why PushMC?
Setting up a Minecraft server by hand is a chore: find the right download page, grab the jar, create `eula.txt`, edit `server.properties`, figure out which Java you need, write a start script. Do it a few times and it gets old fast.
Existing tools that automate this are mostly heavy, Java-based and slow to start. PushMC is one small Rust binary that does it all in a single command.

### Features

1. **6 cores out of the box** — Vanilla, Paper, Purpur, Fabric, Forge, NeoForge. The latest build for your Minecraft version is resolved automatically.
2. **Forge / NeoForge handled properly** — the installer is downloaded and run for you, and the server is started through the generated `run.sh` / `run.bat`.
3. **`server.properties` from flags** — seed, gamemode, difficulty, MOTD, view distance and more, written before the first launch.
4. **Bring your own world** — `--ready-world <path>` copies an existing world folder into the new server.
5. **Java detection** — finds Java installs (`JAVA_HOME`, `PATH`, `/usr/lib/jvm`), lets you pick one, or auto-selects the newest one up to Java 21.
6. **Live console** — server output is streamed to your terminal and everything you type is passed to the server.
7. **Daemon mode** — `--daemon` runs the server in the background, and survives closing the terminal. Manage it with `status`, `cmd`, `errors` and `stop`.
8. **Quick restart** — once a server is installed, `pushmc run <name>` starts it again, no version or core flags needed.
9. **Logs** — all server output (stdout + stderr) is saved to `servers/<name>/server.log` in every mode.
10. **Escape hatch** — `--custom-core-url` lets you use any jar or installer URL for cores that aren't supported directly.

## Installation
### Option 1: prebuilt binary (no Rust needed)
Grab the latest binary for your OS from the [Releases page](https://github.com/jacobballnarch/PushMC/releases) and run it directly.

### Option 2: build from source
```
git clone https://github.com/jacobballnarch/PushMC
cd PushMC
cargo build --release
```
The executable will be in `target/release/`.

Requirements to build:
- **Rust 1.98+**
- Either way, you'll also need: **Java** 

## Usage
Minimal example (Paper 1.21.4, EULA accepted):
```
./pushmc start --name survival --version 1.21.4 --core paper --eula
```
The server is created in `./servers/<name>/`, relative to the folder you run PushMC from. Always run it from the same folder to find your servers again.
For ready servers strongly recommend use run instead of start.

### Managing servers
```
pushmc run <name>             # start an installed server (live console)
pushmc run <name> --daemon    # start it in the background
pushmc status <name>          # pid, uptime, error count
pushmc cmd <name> say hello   # send a command to the server console
pushmc errors <name>          # lines with ERROR / Exception
pushmc stop <name>            # graceful stop
```
`status`, `cmd`, `errors` and `stop` work for both foreground and daemon servers (from another terminal). In foreground mode, type `stop` in the console for a clean shutdown. Ctrl+C kills the server without saving.

Logs: `servers/<name>/server.log` (server output), `servers/<name>/daemon.log` (PushMC's own output in daemon mode).

More examples:
```
# Fabric, 4 GB of RAM, auto-pick Java
./pushmc start --name fab --version 1.21.4 --core fabric --eula --xms 2G --xmx 4G --java-auto

# Creative, peaceful server with a fixed seed and an existing world
./pushmc start --name build --version 1.21.4 --core purpur --eula \
  --gamemode creative --difficulty peaceful --seed 12345 --ready-world ~/saves/MyWorld

# Cracked server (turn off online-mode)
./pushmc start --name lan --version 1.21.4 --core paper --eula --online-mode false

# Core without built-in support: your own URL
./pushmc start --name custom --version 1.20.1 --core paper --eula \
  --custom-core-url https://example.com/some-server.jar
```

### Flags
| Flag | Description | Default |
|---|---|---|
| `--name` | Server folder name | required |
| `--version` | Minecraft version | required |
| `--core` | `vanilla`, `paper`, `purpur`, `fabric`, `forge`, `neoforge` | required |
| `--eula` | Accept the [Minecraft EULA](https://aka.ms/MinecraftEULA). The server won't start without it | off |
| `--experimental` | Use experimental builds (Paper) | off |
| `--custom-core-url` | Download the jar/installer from this URL instead | — |
| `--xms` / `--xmx` | Initial / max RAM | `1G` / `2G` |
| `--custom-java-path` | Path to a Java executable | — |
| `--java-auto` | Auto-select the Java for your version | off |
| `--ready-world` | Copy this world folder into the server | — |
| `--daemon` | Run the server in the background | off |
| `--cmd` | Console command to run once the server is ready. Repeat the flag for several | — |

`server.properties` flags: `--online-mode`, `--pvp`, `--allow-flight` (default `true`, disable with `--allow-flight false`), `--max-players` (20), `--gamemode` (survival), `--difficulty` (hard), `--motd`, `--white-list`, `--view-distance` (10), `--simulation-distance` (10), `--enable-command-block`, `--spawn-protection` (16), `--seed`, `--world-type` (default), `--bonus-chest`.

These flags are only applied when `server.properties` doesn't exist yet. If the file is already there, PushMC keeps it untouched. To re-apply flags, delete the file.

### Startup commands
`--cmd` runs a console command as soon as the server is ready. Repeat the flag to run several, in order:
```
./pushmc start --name survival --version 1.21.4 --core paper --eula \
  --cmd "op Steve" --cmd "whitelist add Alex" --cmd "gamerule keepInventory true"
```
Put each command in quotes. The commands are saved with the server and run on **every** start, including `pushmc run <name>`, so use ones that are safe to repeat (`op`, `whitelist add`, `gamerule`, ...).

## Known limitations
- Windows code paths (`run.bat` / `cmd`, and daemon mode) are written from the documentation but **not tested on a real Windows machine yet**. Feedback is very welcome.
- The control channel for `status` / `cmd` / `stop` is a local TCP port on `127.0.0.1`. Any process on the same machine can send commands to your server. Don't use it on shared machines.
- While a daemon is running, the PushMC binary can be locked on Windows. Run `pushmc stop <name>` before rebuilding.
- `server.log` is not rotated.

## Roadmap
v1 (CLI) is done.

v2 (daemon) is done: 
- [x] daemon mode for proper working background task 
- [x] quick start of installed servers (`pushmc run <name>`)

for v2.5 planned:

- [x] Extra management commands for running servers
- [x] MC version → required Java version table

for v3 planned:

- [ ] SFTP/SSH deploy to a remote server
- [ ] Modrinth and CurseForge integration: quick install of modpacks, mods or plugins
- [ ] GUI + proper localization

## License
This project is licensed under [GPL-3.0](LICENSE).\
You must accept the [Minecraft EULA](https://aka.ms/MinecraftEULA) to run a server. PushMC only writes the acceptance file when you pass `--eula` explicitly.

### Third-party services
- [Mojang piston-meta](https://piston-meta.mojang.com) — Vanilla
- [PaperMC Fill API](https://fill.papermc.io) — Paper
- [Purpur API](https://api.purpurmc.org) — Purpur
- [Fabric Meta](https://meta.fabricmc.net) — Fabric
- [Forge Maven](https://maven.minecraftforge.net) / [NeoForged Maven](https://maven.neoforged.net) — Forge / NeoForge
