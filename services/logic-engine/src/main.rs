use tokio::net::UnixStream;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use common_ipc::{LogicRequest, LogicResponse};
use std::process::Command;
use tokio::time::{sleep, Duration};

async fn get_mac_address(iface: &str) -> String {
    let path = format!("/sys/class/net/{}/address", iface);
    let output = Command::new("cat")
        .arg(&path)
        .output()
        .unwrap_or_else(|_| {
            println!("Interface {} not found, using mock MAC", iface);
            Command::new("echo").arg("00:11:22:33:44:55").output().unwrap()
        });
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

async fn send_request(req: LogicRequest) -> Result<LogicResponse, Box<dyn std::error::Error>> {
    let mut stream = UnixStream::connect("/tmp/openbmc_logic.sock").await?;
    let encoded = bincode::serialize(&req)?;
    stream.write_all(&encoded).await?;

    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).await?;
    let resp = bincode::deserialize::<LogicResponse>(&buf[..n])?;
    Ok(resp)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Logic Engine: Starting...");

    let interfaces = vec!["eth0", "enp5s0"];
    let poll_interval = Duration::from_secs(5);

    loop {
        for iface in &interfaces {
            let mac = get_mac_address(iface).await;
            if mac.is_empty() {
                continue;
            }

            let path = format!("/xyz/openbmc_project/network/{}", iface);
            let req = LogicRequest::SetProperty {
                path,
                property: "MACAddress".into(),
                value: mac.clone(),
            };

            match send_request(req).await {
                Ok(LogicResponse::PropertyValue { value }) => {
                    println!("Logic Engine: [{}] MAC confirmed = {}", iface, value);
                }
                Ok(LogicResponse::Error { message }) => {
                    eprintln!("Logic Engine: [{}] Error = {}", iface, message);
                }
                Err(e) => {
                    eprintln!("Logic Engine: [{}] Connection error = {}", iface, e);
                }
            }
        }

        sleep(poll_interval).await;
    }
}
