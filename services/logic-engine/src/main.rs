use tokio::net::UnixStream;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use common_ipc::{LogicRequest, LogicResponse};
use std::process::Command;

async fn get_real_mac_address() -> String {
    let output = Command::new("cat")
        .arg("/sys/class/net/eth0/address")       //Lighton: this is for qemu arm target
        //.arg("/sys/class/net/enp5s0/address")   //         this is for Ubuntu PC
        .output()
        .unwrap_or_else(|_| {
            println!("eth0 not found, using mock MAC");
            Command::new("echo").arg("00:11:22:33:44:55").output().unwrap()
        });

    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Logic Engine: Starting...");

    let mac = get_real_mac_address().await;
    println!("Logic Engine: Detected MAC: {}", mac);

    let mut stream = UnixStream::connect("/tmp/openbmc_logic.sock").await?;

    // MAC 값을 실제로 포함해서 전송
    let req = LogicRequest::SetProperty {
        path: "/xyz/openbmc_project/network/eth0".into(),
        property: "MACAddress".into(),
        value: mac,
    };

    let encoded = bincode::serialize(&req)?;
    stream.write_all(&encoded).await?;

    // Response 수신
    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).await?;
    let resp = bincode::deserialize::<LogicResponse>(&buf[..n])?;
    match resp {
        LogicResponse::PropertyValue { value } => {
            println!("Logic Engine: Proxy confirmed MAC = {}", value);
        }
        LogicResponse::Error { message } => {
            eprintln!("Logic Engine: Proxy error = {}", message);
        }
    }

    Ok(())
}
