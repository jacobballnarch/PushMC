use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;
use clap::Parser;
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Parser)]
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
        reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap()
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
                    parse_purpur_versions(resp).await;
                }
                _ => println!("(version listing not supported for this core yet)"),
            }
            return None;
        };
        u
    };

    tokio::fs::create_dir_all(dir).await.unwrap();

    let response = client().get(&url).send().await.unwrap();

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

#[tokio::main]
async fn main() {
    let args = Args::parse();
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
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).unwrap();
            let idx: usize = input.trim().parse().expect("not a number");
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

    let mut child = tokio::process::Command::new(program)
        .args(&launch_args)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to launch server");

    let mut stdin = child.stdin.take().expect("stdin not connected");
    let stdout = child.stdout.take().expect("stdout not connected");

    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            println!("[server] {line}");
        }
    });

    tokio::spawn(async move {
        let mut input = BufReader::new(tokio::io::stdin()).lines();
        while let Ok(Some(line)) = input.next_line().await {
            if stdin.write_all(line.as_bytes()).await.is_err() { break; }
            if stdin.write_all(b"\n").await.is_err() { break; }
            if line == "stop" { break; }
        }
    });

    let status = child.wait().await.expect("error while waiting for process");
    println!("Java exited with code: {:?}", status.code());
}
