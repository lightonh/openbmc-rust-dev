use zbus::Connection;
use tokio::net::UnixListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::sync::Arc;
use tokio::sync::RwLock;
use common_ipc::{LogicRequest, LogicResponse};

struct NetworkProxy {
    mac_cache: Arc<RwLock<String>>,
}

// trait 방식 제거 → struct에 직접 적용
#[zbus::interface(name = "xyz.openbmc_project.Network.MACAddress")]
impl NetworkProxy {
    #[zbus(property)]
    async fn mac_address(&self) -> String {
        self.mac_cache.read().await.clone()
    }
}

#[tokio::main]
async fn main() -> zbus::Result<()> {
    let mac_cache = Arc::new(RwLock::new("00:11:22:33:44:55".to_string()));

    let proxy = NetworkProxy {
        mac_cache: mac_cache.clone(),
    };

    // 시스템 버스 연결 후 이름 등록
    let conn = Connection::system().await?;
    conn.request_name("xyz.openbmc_project.Network").await?;
    conn.object_server()
        .at("/xyz/openbmc_project/network/eth0", proxy)
        .await?;

    println!("Stable Proxy: MACAddress interface exported");

    // IPC 소켓 설정
    let socket_path = "/tmp/openbmc_logic.sock";
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path).expect("Failed to bind socket");
    println!("Stable Proxy: Listening for Logic Engine on {}", socket_path);

    loop {
        let (mut socket, _) = listener.accept().await.unwrap();
        let cache = mac_cache.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            if let Ok(n) = socket.read(&mut buf).await {
                match bincode::deserialize::<LogicRequest>(&buf[..n]) {
                    Ok(LogicRequest::SetProperty { value, .. }) => {
                        // 캐시 실제 업데이트
                        {
                            let mut w = cache.write().await;
                            *w = value.clone();
                        }
                        println!("Proxy: MAC cache updated to {}", value);

                        // Response 전송
                        let resp = LogicResponse::PropertyValue { value };
                        let encoded = bincode::serialize(&resp).unwrap();
                        let _ = socket.write_all(&encoded).await;
                    }
                    Ok(LogicRequest::GetProperty { .. }) => {
                        let value = cache.read().await.clone();
                        let resp = LogicResponse::PropertyValue { value };
                        let encoded = bincode::serialize(&resp).unwrap();
                        let _ = socket.write_all(&encoded).await;
                    }
                    Err(e) => {
                        let resp = LogicResponse::Error { message: e.to_string() };
                        let encoded = bincode::serialize(&resp).unwrap();
                        let _ = socket.write_all(&encoded).await;
                    }
                }
            }
        });
    }
}
