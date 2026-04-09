use zbus::Connection;
use tokio::net::UnixListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::sync::Arc;
use tokio::sync::RwLock;
use common_ipc::{LogicRequest, LogicResponse};
use std::collections::HashMap;
use zbus::zvariant::Value;

// ── Property 저장소 ──────────────────────────────────────────
#[derive(Clone)]
struct PropertyStore {
    properties: Arc<RwLock<HashMap<String, String>>>,
}

impl PropertyStore {
    fn new() -> Self {
        Self {
            properties: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn get(&self, key: &str) -> Option<String> {
        self.properties.read().await.get(key).cloned()
    }

    async fn set(&self, key: String, value: String) -> bool {
        let mut map = self.properties.write().await;
        let old = map.get(&key).cloned();
        map.insert(key, value.clone());
        old.as_deref() != Some(&value)  // 값이 바뀌었으면 true
    }
}

// ── D-Bus 인터페이스 ─────────────────────────────────────────
struct NetworkProxy {
    store: PropertyStore,
}

#[zbus::interface(name = "xyz.openbmc_project.Network.MACAddress")]
impl NetworkProxy {
    // Property 읽기
    #[zbus(property)]
    async fn mac_address(&self) -> String {
        self.store.get("MACAddress").await
            .unwrap_or_else(|| "00:00:00:00:00:00".to_string())
    }

    // Property 쓰기
    #[zbus(property)]
    async fn set_mac_address(&self, value: String) {
        self.store.set("MACAddress".to_string(), value).await;
    }

    // PropertiesChanged 시그널
    #[zbus(signal)]
    async fn properties_changed(
        ctx: &zbus::SignalContext<'_>,
        interface_name: &str,
        changed_properties: HashMap<String, Value<'_>>,
        invalidated_properties: Vec<String>,
    ) -> zbus::Result<()>;
}

// ── 객체 관리자 ──────────────────────────────────────────────
struct ObjectManager;

#[zbus::interface(name = "org.freedesktop.DBus.ObjectManager")]
impl ObjectManager {
    fn get_managed_objects(
        &self,
    ) -> HashMap<String, HashMap<String, HashMap<String, Value<'_>>>> {
        HashMap::new()
    }
}

// ── IPC 핸들러 ───────────────────────────────────────────────
async fn handle_ipc(
    conn: Connection,
    listener: UnixListener,
    stores: Arc<RwLock<HashMap<String, PropertyStore>>>,
) {
    loop {
        let (mut socket, _) = listener.accept().await.unwrap();
        let conn = conn.clone();
        let stores = stores.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            if let Ok(n) = socket.read(&mut buf).await {
                match bincode::deserialize::<LogicRequest>(&buf[..n]) {
                    Ok(LogicRequest::SetProperty { path, property, value }) => {
                        // 경로에 해당하는 store 가져오기 (없으면 생성)
                        let store = {
                            let mut map = stores.write().await;
                            map.entry(path.clone())
                                .or_insert_with(PropertyStore::new)
                                .clone()
                        };

                        // if the object is not D-Bus, do dynamically register it
                        if conn.object_server()
                            .interface::<_, NetworkProxy>(path.as_str())
                            .await
                            .is_err()
                        {
                            let new_proxy = NetworkProxy { store: store.clone() };
                            conn.object_server()
                                .at(path.as_str(), new_proxy)
                                .await
                                .unwrap();
                            println!("Proxy: New object registered: {}", path);
                        }
                        

                        let changed = store.set(property.clone(), value.clone()).await;

                        if changed {
                            println!("Proxy: [{}] {} = {}", path, property, value);

                            // PropertiesChanged 시그널 발송
                            if let Ok(iface_ref) = conn.object_server()
                                .interface::<_, NetworkProxy>(path.as_str())
                                .await
                            {
                                let mut changed_props = HashMap::new();
                                changed_props.insert(
                                    property.clone(),
                                    Value::from(value.clone()),
                                );

                                let _ = NetworkProxy::properties_changed(
                                    iface_ref.signal_context(),
                                    "xyz.openbmc_project.Network.MACAddress",
                                    changed_props,
                                    vec![],
                                ).await;

                                println!("Proxy: PropertiesChanged signal sent for {}", path);
                            }
                        }

                        let resp = LogicResponse::PropertyValue { value };
                        let encoded = bincode::serialize(&resp).unwrap();
                        let _ = socket.write_all(&encoded).await;
                    }

                    Ok(LogicRequest::GetProperty { path, property }) => {
                        let stores = stores.read().await;
                        let value = if let Some(store) = stores.get(&path) {
                            store.get(&property).await
                                .unwrap_or_else(|| "".to_string())
                        } else {
                            "".to_string()
                        };

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

// ── main ─────────────────────────────────────────────────────
#[tokio::main]
async fn main() -> zbus::Result<()> {
    println!("Stable Proxy: Starting...");

    let conn = Connection::system().await?;
    conn.request_name("xyz.openbmc_project.Network").await?;

    // ObjectManager 등록 (루트 경로)
    conn.object_server()
        .at("/xyz/openbmc_project/network", ObjectManager)
        .await?;

    // 초기 객체 등록 (eth0)
    let stores: Arc<RwLock<HashMap<String, PropertyStore>>> =
        Arc::new(RwLock::new(HashMap::new()));

    let eth0_store = PropertyStore::new();
    stores.write().await.insert(
        "/xyz/openbmc_project/network/eth0".to_string(),
        eth0_store.clone(),
    );

    conn.object_server()
        .at("/xyz/openbmc_project/network/eth0", NetworkProxy {
            store: eth0_store,
        })
        .await?;

    println!("Stable Proxy: Objects exported");

    // IPC 소켓
    let socket_path = "/tmp/openbmc_logic.sock";
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path)
        .expect("Failed to bind socket");
    println!("Stable Proxy: Listening on {}", socket_path);

    // systemd에 준비 완료 알림 (sd_notify)
    // notify_systemd();

    handle_ipc(conn, listener, stores).await;

    Ok(())
}
