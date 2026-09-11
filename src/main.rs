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

async fn get_download_url(mc_version: &str) -> Option<String> {
    let url = format!("https://fill.papermc.io/v3/projects/paper/versions/{mc_version}/builds");
    let resp = client().get(url).send().await.ok()?;
    let builds: serde_json::Value = resp.json().await.ok()?;

    let builds_arr = builds.as_array()?;
    for build in builds_arr {
        if build["channel"].as_str() == Some("STABLE") {
            let download_url = build["downloads"]["server:default"]["url"].as_str()?;
            return Some(download_url.to_string());
        }
    }
    None
}

async fn download_version(mc_version: &str, dir: &Path) -> Option<String> {
    let Some(url) = get_download_url(mc_version).await else {
        println!("Не нашёл стабильный билд для версии {mc_version}");
        println!("Доступные версии для скачивания:");
        let resp = client().get("https://fill.papermc.io/v3/projects/paper").send().await.ok().unwrap();
        parse_versions(resp).await;
        return None;
    };

    tokio::fs::create_dir_all(dir).await.unwrap();
    let file_name = url.split('/').last().unwrap().to_string();
    let file_path = dir.join(&file_name);

    let response = client().get(url).send().await.unwrap();
    let mut stream = response.bytes_stream();
    let mut file = File::create(&file_path).await.unwrap();
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.unwrap();
        file.write_all(&chunk).await.unwrap();
    }

    Some(file_name)
}

async fn write_eula(server_dir: &Path, accepted: bool) {
    let content = format!(
        "#By changing the setting below to TRUE you are indicating your agreement to our EULA (https://aka.ms/MinecraftEULA).\neula={}\n",
        accepted
    );
    tokio::fs::write(server_dir.join("eula.txt"), content).await.unwrap();
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let dir = server_dir(&args.name);
    let jar_name = download_version(&args.version, &dir).await.expect("не удалось скачать сервер");
    /*println!("{:?}", which::which("java"));
    println!("{:?}", std::env::var("JAVA_HOME").ok());*/
    let accepted = args.eula;
    if accepted {
        write_eula(&dir, args.eula).await;
    } else {
        println!("Без eula сервер не запустится. Принять eula можно через --eula");
        return;
    }
    let mut child = tokio::process::Command::new("java")
        .arg("-Xms1G").arg("-Xmx2G").arg("-jar").arg(jar_name).arg("nogui")
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("не смог запустить java");

    let mut stdin = child.stdin.take().expect("stdin не подключен");
    let stdout = child.stdout.take().expect("stdout не подключен");

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

    let status = child.wait().await.expect("ошибка при ожидании процесса");
    println!("Java завершилась с кодом: {:?}", status.code());
}
