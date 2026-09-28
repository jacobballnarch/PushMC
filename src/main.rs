use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;
use clap::Parser;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

#[derive(clap::Args)]
struct Args {
    #[arg(long)]
    name:String,
    #[arg(long)]
    version:String,
    #[arg(long)]
    eula:bool,
    #[arg(long, value_enum)]
    core: Core,
    #[arg(long)]
    custom_core_url: Option<String>,
    #[arg(long)]
    experimental:bool,
    #[arg(long, default_value = "1G")]
    xms: String,
    #[arg(long, default_value = "2G")]
    xmx: String,
    #[arg(long)]
    custom_java_path: Option<String>,
    #[arg(long)]
    java_auto: bool,
    #[arg(long, action = clap::ArgAction::Set, default_value_t = true)]
    online_mode: bool,
    #[arg(long, action = clap::ArgAction::Set, default_value_t = true)]
    pvp: bool,
    #[arg(long, default_value_t = 20)]
    max_players: u32,
    #[arg(long, default_value = "survival")]
    gamemode: String,
    #[arg(long, default_value = "hard")]
    difficulty: String,
    #[arg(long, default_value = "A Minecraft Server")]
    motd: String,
    #[arg(long)]
    white_list: bool,
    #[arg(long, default_value_t = 10)]
    view_distance: u8,
    #[arg(long, default_value_t = 10)]
    simulation_distance: u8,
    #[arg(long)]
    enable_command_block: bool,
    #[arg(long, default_value_t = 16)]
    spawn_protection: u32,
    #[arg(long, action = clap::ArgAction::Set, default_value_t = true)]
    allow_flight: bool,
    #[arg(long)]
    seed: Option<String>,
    #[arg(long, default_value = "default")]
    world_type: String,
    #[arg(long)]
    bonus_chest: bool,
    #[arg(long)]
    ready_world: Option<String>,
    #[arg(long)]
    daemon: bool,
}

#[derive(clap::Subcommand)]
enum Cmd {
    Start(Args),
    Status { name: String },
    Stop { name: String },
    Errors { name: String },
    Cmd { name: String, line: Vec<String> },
    /// Start an already installed server (no version/core flags needed)
    Run {
        name: String,
        #[arg(long)]
        daemon: bool,
    },
    #[command(hide = true)]
    Serve { name: String },
}

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

async fn write_properties(dir: &Path, args: &Args) {
    let path = dir.join("server.properties");
    if path.exists() {
        println!("server.properties already exists, keeping it (property flags ignored)");
        return;
    }
    let mut c = String::new();
    c.push_str(&format!("online-mode={}\n", args.online_mode));
    c.push_str(&format!("pvp={}\n", args.pvp));
    c.push_str(&format!("max-players={}\n", args.max_players));
    c.push_str(&format!("gamemode={}\n", args.gamemode));
    c.push_str(&format!("difficulty={}\n", args.difficulty));
    c.push_str(&format!("motd={}\n", args.motd));
    c.push_str(&format!("white-list={}\n", args.white_list));
    c.push_str(&format!("view-distance={}\n", args.view_distance));
    c.push_str(&format!("simulation-distance={}\n", args.simulation_distance));
    c.push_str(&format!("enable-command-block={}\n", args.enable_command_block));
    c.push_str(&format!("spawn-protection={}\n", args.spawn_protection));
    c.push_str(&format!("allow-flight={}\n", args.allow_flight));
    if let Some(s) = &args.seed { c.push_str(&format!("level-seed={s}\n")); }
    c.push_str(&format!("level-type={}\n", args.world_type));
    c.push_str(&format!("generate-bonus-chest={}\n", args.bonus_chest));
    tokio::fs::write(dir.join("server.properties"), c).await.unwrap();
}

async fn copy_dir_recursively(src: &Path, dst: &Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dst).await?;
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let file_type = entry.file_type().await?;
        let dest_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            Box::pin(copy_dir_recursively(&entry.path(), &dest_path)).await?;
        } else {
            tokio::fs::copy(entry.path(), &dest_path).await?;
        }
    }
    Ok(())
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum Core {
    Vanilla,
    Paper,
    Purpur,
    Fabric,
    Forge,
    Neoforge,
}

fn client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder().connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .build().expect("Could not build client for jar downloading")
    })
}

fn server_dir(name: &str) -> PathBuf {
    Path::new("./servers").join(name)
}

async fn parse_versions(response: reqwest::Response) {
    let response_json: serde_json::Value = response.json().await.unwrap();
    let versions_obj = response_json["versions"].as_object().unwrap();
    let mut output:Vec<String> = Vec::new();
    for (_major, list) in versions_obj {
        for v in list.as_array().unwrap() {
            output.push(v.as_str().unwrap().to_string());
        }
    }
    println!("Versions: {:?}", output);
}

fn find_build_url(builds: &serde_json::Value, experimental: bool) -> Option<String> {
    let wanted_channel = if experimental { "EXPERIMENTAL" } else { "STABLE" };
    let builds_arr = builds.as_array()?;
    for build in builds_arr {
        if build["channel"].as_str() == Some(wanted_channel) {
            let download_url = build["downloads"]["server:default"]["url"].as_str()?;
            return Some(download_url.to_string());
        }
    }
    None
}

async fn get_download_url(core: &Core, mc_version: &str, exp: bool) -> Option<String> {
    match core {
        Core::Vanilla => get_vanilla_url(mc_version).await,
        Core::Paper => {
            let url = format!("https://fill.papermc.io/v3/projects/paper/versions/{mc_version}/builds");
            let resp = client().get(url).send().await.ok()?;
            let builds: serde_json::Value = resp.json().await.ok()?;
            find_build_url(&builds, exp)
        }
        Core::Purpur => {Some(format!("https://api.purpurmc.org/v2/purpur/{mc_version}/latest/download"))}
        Core::Fabric => get_fabric_url(mc_version).await,
        Core::Forge => get_forge_url(mc_version).await,
        Core::Neoforge => get_neoforge_url(mc_version).await,
    }
}

async fn get_vanilla_url(mc_version: &str) -> Option<String> {
    let resp = client()
        .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
        .send().await.ok()?;
    let manifest: serde_json::Value = resp.json().await.ok()?;

    let versions = manifest["versions"].as_array()?;
    let version_url = versions.iter()
        .find(|v| v["id"].as_str() == Some(mc_version))?["url"]
        .as_str()?;

    let resp = client().get(version_url).send().await.ok()?;
    let version_meta: serde_json::Value = resp.json().await.ok()?;
    let download_url = version_meta["downloads"]["server"]["url"].as_str()?;

    Some(download_url.to_string())
}

async fn get_fabric_url(mc_version: &str) -> Option<String> {
    let resp = client()
        .get(format!("https://meta.fabricmc.net/v2/versions/loader/{mc_version}"))
        .send().await.ok()?;
    let loaders: serde_json::Value = resp.json().await.ok()?;
    let loader_version = loaders.as_array()?.first()?["loader"]["version"].as_str()?;

    let resp = client()
        .get("https://meta.fabricmc.net/v2/versions/installer")
        .send().await.ok()?;
    let installers: serde_json::Value = resp.json().await.ok()?;
    let installer_version = installers.as_array()?.first()?["version"].as_str()?;

    Some(format!(
        "https://meta.fabricmc.net/v2/versions/loader/{mc_version}/{loader_version}/{installer_version}/server/jar"
    ))
}

async fn get_neoforge_url(mc_version: &str) -> Option<String> {
    let resp = client()
        .get("https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml")
        .send().await.ok()?;
    let xml = resp.text().await.ok()?;
    let versions = parse_maven_versions(&xml);

    let prefix = format!("{}.", mc_version.strip_prefix("1.")?);
    let version = versions.iter().rev().find(|v| v.starts_with(&prefix))?;

    Some(format!(
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar"
    ))
}

async fn get_forge_url(mc_version: &str) -> Option<String> {
    let resp = client()
        .get("https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml")
        .send().await.ok()?;
    let xml = resp.text().await.ok()?;
    let versions = parse_maven_versions(&xml);

    let prefix = format!("{mc_version}-");
    let version = versions.iter().rev().find(|v| v.starts_with(&prefix))?;

    Some(format!(
        "https://maven.minecraftforge.net/net/minecraftforge/forge/{version}/forge-{version}-installer.jar"
    ))
}

async fn download_version(core: &Core, mc_version: &str, dir: &Path, exp: bool, custom_url: &Option<String>) -> Option<String> {
    let url = if let Some(custom) = custom_url {
        custom.clone()
    } else {
        let Some(u) = get_download_url(core, mc_version, exp).await else {
            println!("Couldn't find a stable build for version {mc_version}");
            println!("Available versions to download:");
            match core {
                Core::Paper => {
                    let resp = client().get("https://fill.papermc.io/v3/projects/paper").send().await.ok().unwrap();
                    parse_versions(resp).await;
                }
                Core::Purpur => {
                    let resp = client().get("https://api.purpurmc.org/v2/purpur").send().await.ok().unwrap();
                    let respstatus = resp.error_for_status().expect("Purpur api error");
                    parse_purpur_versions(respstatus).await;
                }
                _ => println!("(version listing not supported for this core yet)"),
            }
            return None;
        };
        u
    };

    tokio::fs::create_dir_all(dir).await.unwrap();

    let response = client().get(&url).send().await.unwrap().error_for_status().expect("Download failed, check your internet connection");

    let file_name = response.headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split("filename=").nth(1))
        .map(|s| s.trim_matches('"').to_string())
        .unwrap_or_else(|| url.split('/').last().unwrap().to_string());

    let file_path = dir.join(&file_name);

    let mut stream = response.bytes_stream();
    let mut file = File::create(&file_path).await.unwrap();
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.unwrap();
        file.write_all(&chunk).await.unwrap();
    }

    Some(file_name)
}

async fn parse_purpur_versions(response: reqwest::Response) {
    let response_json: serde_json::Value = response.json().await.unwrap();
    let versions = response_json["versions"].as_array().unwrap();
    let output: Vec<&str> = versions.iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    println!("Versions: {:?}", output);
}

fn parse_maven_versions(xml: &str) -> Vec<String> {
    let mut versions = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<version>") {
        rest = &rest[start + 9..];
        if let Some(end) = rest.find("</version>") {
            versions.push(rest[..end].trim().to_string());
            rest = &rest[end + 10..];
        } else { break; }
    }
    versions
}

async fn write_eula(server_dir: &Path, accepted: bool) {
    let content = format!(
        "#By changing the setting below to TRUE you are indicating your agreement to our EULA (https://aka.ms/MinecraftEULA).\neula={}\n",
        accepted
    );
    tokio::fs::write(server_dir.join("eula.txt"), content).await.unwrap();
}

fn find_java_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        let p = Path::new(&java_home).join("bin").join(if cfg!(windows) { "java.exe" } else { "java" });
        if p.exists() { candidates.push(p); }
    }

    if let Ok(p) = which::which("java") {
        if !candidates.contains(&p) { candidates.push(p); }
    }

    if cfg!(target_os = "linux") {
        if let Ok(entries) = std::fs::read_dir("/usr/lib/jvm") {
            for entry in entries.flatten() {
                let p = entry.path().join("bin").join("java");
                if p.exists() && !candidates.contains(&p) { candidates.push(p); }
            }
        }
    }

    candidates
}

fn get_java_major_version(java_path: &Path) -> Option<u32> {
    let output = std::process::Command::new(java_path).arg("-version").output().ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let start = stderr.find('"')? + 1;
    let rest = &stderr[start..];
    let version_str = &rest[..rest.find('"')?];

    let mut parts = version_str.split('.');
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

async fn start(args: Args) {
    if try_ask(&args.name, "status").await.is_some() {
        println!("Server '{}' is already running", args.name);
        return;
    }
    let java_path: PathBuf = if let Some(custom) = &args.custom_java_path {
        PathBuf::from(custom)
    } else {
        let candidates = find_java_candidates();
        let versioned: Vec<(PathBuf, Option<u32>)> = candidates.into_iter()
            .map(|p| { let v = get_java_major_version(&p); (p, v) })
            .collect();

        if args.java_auto {
            versioned.iter()
                .filter_map(|(p, v)| v.map(|v| (p, v)))
                .filter(|(_, v)| *v <= 21)
                .max_by_key(|(_, v)| *v)
                .map(|(p, _)| p.clone())
                .expect("no suitable Java (<=21) found on this machine")
        } else {
            println!("Found Java versions:");
            for (i, (p, v)) in versioned.iter().enumerate() {
                println!("{}) {} — {}", i + 1, p.display(), v.map(|v| v.to_string()).unwrap_or("unknown".into()));
            }
            println!("Choose a number:");

            if versioned.is_empty() {
                eprintln!("No Java found. Use --custom-java-path");
                return;
            }

            let idx = loop {
                println!("Choose a number (1-{}):", versioned.len());
                let mut input = String::new();
                std::io::stdin().read_line(&mut input).unwrap();
                match input.trim().parse::<usize>() {
                    Ok(n) if (1..=versioned.len()).contains(&n) => break n,
                    _ => println!("Invalid choice, try again"),
                }
            };

            versioned[idx - 1].0.clone()
        }
    };
    let dir = server_dir(&args.name);
    let jar_name = download_version(&args.core, &args.version, &dir, args.experimental, &args.custom_core_url)
        .await
        .expect("failed to download server");
    let accepted = args.eula;
    if accepted {
        write_eula(&dir, args.eula).await;
    } else {
        println!("Server won't start without EULA. Accept it with --eula");
        return;
    }
    write_properties(&dir, &args).await;
    if let Some(world_path) = &args.ready_world {
        let dest = dir.join("world");
        if let Err(e) = copy_dir_recursively(Path::new(world_path), &dest).await {
            println!("Couldn't copy the world from '{world_path}': {e}");
            println!("Continue with a fresh default world? (y/n)");
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).unwrap();
            if input.trim().eq_ignore_ascii_case("n") {
                println!("Aborted by user.");
                return;
            }
        }
    }
    if matches!(args.core, Core::Forge | Core::Neoforge) {
        println!("Running installer {}...", jar_name);
        let install_status = tokio::process::Command::new(&java_path)
            .arg("-jar").arg(&jar_name).arg("--installServer")
            .current_dir(&dir)
            .status()
            .await
            .expect("failed to launch installer");

        if !install_status.success() {
            eprintln!("Installation failed, exit code: {:?}", install_status.code());
            return;
        }
    }

    let xms_flag = format!("-Xms{}", args.xms);
    let xmx_flag = format!("-Xmx{}", args.xmx);

    let script_name = if cfg!(target_os = "windows") { "run.bat" } else { "run.sh" };
    let java_path_str = java_path.to_str().expect("java path contains invalid characters");

    let (program, launch_args): (&str, Vec<&str>) = if matches!(args.core, Core::Forge | Core::Neoforge) {
        if cfg!(target_os = "windows") {
            ("cmd", vec!["/C", script_name, "nogui"])
        } else {
            ("bash", vec![script_name, "nogui"])
        }
    } else {
        (java_path_str, vec![&xms_flag, &xmx_flag, "-jar", &jar_name, "nogui"])
    };

    let spec = serde_json::json!({ "program": program, "args": launch_args });
    std::fs::write(dir.join("launch.json"), spec.to_string()).unwrap();

    launch(args.name, args.daemon).await;
}

async fn launch(name: String, daemon: bool) {
    if try_ask(&name, "status").await.is_some() {
        println!("Server '{name}' is already running");
        return;
    }
    if daemon {
        spawn_daemon(&name);
    } else {
        serve(name, true).await;
    }
}

fn spawn_daemon(name: &str) {
    let dir = server_dir(name);
    let log = std::fs::OpenOptions::new()
        .create(true).append(true)
        .open(dir.join("daemon.log")).unwrap();
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args(["serve", name])
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log);

    #[cfg(unix)]
    { use std::os::unix::process::CommandExt; cmd.process_group(0); }
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; cmd.creation_flags(0x00000008 | 0x00000200); }

    cmd.spawn().expect("failed to start daemon");
    println!("Daemon started. Try: pushmc status {name}");
}

struct State {
    errors: Mutex<Vec<String>>,
    started: std::time::Instant,
    tx: tokio::sync::mpsc::Sender<String>,
    pid: u32,
    log: Mutex<std::fs::File>,
}

fn log_line(st: &State, line: &str) {
    use std::io::Write;
    if let Ok(mut f) = st.log.lock() {
        let _ = writeln!(f, "{line}");
    }
}

async fn serve(name: String, foreground: bool) {
    let dir = server_dir(&name);
    let spec: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("launch.json")).unwrap()).unwrap();
    let args: Vec<&str> = spec["args"].as_array().unwrap().iter().filter_map(|a| a.as_str()).collect();

    let log_file = std::fs::OpenOptions::new()
        .create(true).append(true)
        .open(dir.join("server.log")).expect("cannot open server.log");

    let mut child = tokio::process::Command::new(spec["program"].as_str().unwrap())
        .args(args).current_dir(&dir)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().expect("failed to launch server");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);
    let state = Arc::new(State {
        errors: Mutex::new(Vec::new()),
        started: std::time::Instant::now(),
        tx,
        pid: child.id().unwrap_or(0),
        log: Mutex::new(log_file),
    });

    tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            if stdin.write_all(format!("{line}\n").as_bytes()).await.is_err() { break; }
        }
    });

    let mut readers = Vec::new();
    for stream in [
        Box::pin(stdout) as std::pin::Pin<Box<dyn tokio::io::AsyncRead + Send>>,
        Box::pin(stderr),
    ] {
        let st = state.clone();
        readers.push(tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                if foreground { println!("[server] {l}"); }
                log_line(&st, &l);
                if l.contains("/ERROR]") || l.contains("Exception") {
                    st.errors.lock().unwrap().push(l);
                }
            }
        }));
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let port_file = dir.join("pushmc.port");
    std::fs::write(&port_file, port.to_string()).unwrap();

    let st = state.clone();
    tokio::spawn(async move {
        while let Ok((s, _)) = listener.accept().await {
            tokio::spawn(handle(s, st.clone()));
        }
    });

    if foreground {
        let tx = state.tx.clone();
        tokio::spawn(async move {
            let mut input = BufReader::new(tokio::io::stdin()).lines();
            while let Ok(Some(line)) = input.next_line().await {
                if tx.send(line).await.is_err() { break; }
            }
        });
    }

    let status = child.wait().await.ok();
    let _ = std::fs::remove_file(&port_file);
    for h in readers { let _ = h.await; }
    if foreground {
        println!("Java exited with code: {:?}", status.and_then(|s| s.code()));
        std::process::exit(0); // stdin-таск блокирует завершение рантайма
    }
}

async fn handle(stream: TcpStream, st: Arc<State>) {
    let (r, mut w) = stream.into_split();
    let mut req = String::new();
    BufReader::new(r).read_line(&mut req).await.ok();
    let req = req.trim();

    let reply = match req {
        "status" => format!("running, pid {}, uptime {}s, errors: {}",
                            st.pid, st.started.elapsed().as_secs(), st.errors.lock().unwrap().len()),
        "errors" => {
            let e = st.errors.lock().unwrap();
            if e.is_empty() { "no errors".into() } else { e.join("\n") }
        }
        "stop" => { st.tx.send("stop".into()).await.ok(); "stopping...".into() }
        _ if req.starts_with("cmd ") => { st.tx.send(req[4..].to_string()).await.ok(); "sent".into() }
        _ => "unknown command".into(),
    };
    w.write_all(reply.as_bytes()).await.ok();
}

async fn try_ask(name: &str, req: &str) -> Option<String> {
    let port_file = server_dir(name).join("pushmc.port");
    let port = std::fs::read_to_string(&port_file).ok()?;
    let mut s = match TcpStream::connect(format!("127.0.0.1:{}", port.trim())).await {
        Ok(s) => s,
        Err(_) => {
            let _ = std::fs::remove_file(&port_file);
            return None;
        }
    };
    s.write_all(format!("{req}\n").as_bytes()).await.ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).await.ok()?;
    Some(out)
}

async fn ask(name: &str, req: &str) -> String {
    try_ask(name, req).await
        .unwrap_or_else(|| format!("Server '{name}' is not running"))
}
#[tokio::main]
async fn main() {
    match Cli::parse().cmd {
        Cmd::Start(args) => start(args).await,
        Cmd::Serve { name } => serve(name, false).await,
        Cmd::Run { name, daemon } => {
            if !server_dir(&name).join("launch.json").exists() {
                println!("Server '{name}' is not installed. Use: pushmc start --name {name} ...");
                return;
            }
            launch(name, daemon).await;
        }
        Cmd::Status { name } => println!("{}", ask(&name, "status").await),
        Cmd::Stop { name } => println!("{}", ask(&name, "stop").await),
        Cmd::Errors { name } => println!("{}", ask(&name, "errors").await),
        Cmd::Cmd { name, line } => println!("{}", ask(&name, &format!("cmd {}", line.join(" "))).await),
    }
}