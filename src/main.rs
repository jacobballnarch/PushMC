use std::path::Path;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;


fn client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap()
    })
}

async fn parse_versions(response: reqwest::Response) {
    let response_json: serde_json::Value = response.json().await.unwrap();
    let versions_obj = response_json["versions"].as_object().unwrap();
    let mut output:Vec<String> = Vec::new();
    for (major, list) in versions_obj {
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

async fn download_version(mc_version: &str) {
    let Some(url) = get_download_url(mc_version).await else {
        println!("Не нашёл стабильный билд для версии {mc_version}");
        return;
    };

    let dir = Path::new("./downloads");
    tokio::fs::create_dir_all(dir).await.unwrap();
    let file_name = url.split('/').last().unwrap();
    let file_path = dir.join(file_name);

    let response = client().get(url).send().await.unwrap();
    let mut stream = response.bytes_stream();
    let mut file = File::create(&file_path).await.unwrap();
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.unwrap();
        file.write_all(&chunk).await.unwrap();
    }
}

#[tokio::main]
async fn main() {
    /*let resp = client().get("https://fill.papermc.io/v3/projects/paper")
    .send().await.ok().unwrap();
    parse_versions(resp).await;
    download_version("1.12.2").await;*/
    println!("{:?}", which::which("java"));
    println!("{:?}", std::env::var("JAVA_HOME").ok());
    let server_dir = Path::new("./downloads/");
    let mut child = tokio::process::Command::new("java")
        .arg("-Xms1G")
        .arg("-Xmx2G")
        .arg("-jar")
        .arg("paper-1.12.2-1620.jar")
        .arg("nogui")
        .current_dir(server_dir)
        .spawn()
        .expect("не смог запустить java");

    let status = child.wait().await.expect("ошибка при ожидании процесса");
    println!("Java завершилась с кодом: {:?}", status.code());
}
