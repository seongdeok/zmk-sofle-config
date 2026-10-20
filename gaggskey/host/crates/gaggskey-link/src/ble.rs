//! BLE transport: custom GATT service, one characteristic (write + notify).
//!
//! The keyboard is normally already connected to the OS as a HID keyboard, so
//! it does not advertise. We look it up among the devices the OS already knows
//! and reuse that connection.
//!
//! btleplug is async; this wraps it in a private current-thread tokio runtime
//! running on its own thread so the [`Transport`] API stays blocking.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter, WriteType};
#[cfg(target_os = "macos")]
use btleplug::api::RetrievePeripheralsOptions;
use btleplug::platform::{Manager, Peripheral};
use futures::StreamExt;
use gaggskey_proto::{self as proto, Frame};
use tokio::sync::mpsc as amp;
use uuid::Uuid;

use crate::{Device, Error, Result, Transport, Via};

pub struct BleTransport {
    tx: amp::UnboundedSender<Frame>,
    rx: mpsc::Receiver<std::result::Result<Vec<u8>, String>>,
    name: String,
}

fn uuid(s: &str) -> Uuid {
    Uuid::parse_str(s).expect("valid uuid")
}

/// Name hints for system-connected keyboards whose cached services do not
/// list the GaggsKey UUID yet.
const NAME_HINTS: [&str; 4] = ["Gaggs", "Sofle", "Kyria", "Corne"];

/// Every OS-connected peripheral that looks like a GaggsKey keyboard.
async fn candidates(manager: &Manager) -> std::result::Result<Vec<(Peripheral, String)>, String> {
    let service = uuid(proto::BT_SERVICE_UUID);
    let mut out: Vec<(Peripheral, String)> = Vec::new();
    for adapter in manager.adapters().await.map_err(|e| e.to_string())? {
        // CoreBluetooth reports system-connected peripherals as disconnected
        // until this app calls connect(), which reuses the OS link.
        #[cfg(target_os = "macos")]
        if let Ok(peripherals) = adapter
            .retrieve_peripherals(RetrievePeripheralsOptions {
                services: Some(vec![service]),
                ..Default::default()
            })
            .await
        {
            for p in peripherals {
                if p.is_connected().await.unwrap_or(false) || p.connect().await.is_ok() {
                    let name = p.properties().await.ok().flatten().and_then(|pr| pr.local_name).unwrap_or_default();
                    out.push((p, name));
                }
            }
        }

        // A short filtered scan also lets CoreBluetooth surface connected devices.
        let _ = adapter.start_scan(ScanFilter { services: vec![service] }).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let _ = adapter.stop_scan().await;
        for p in adapter.peripherals().await.map_err(|e| e.to_string())? {
            if out.iter().any(|(q, _)| q.id() == p.id()) {
                continue;
            }
            let Ok(Some(props)) = p.properties().await else { continue };
            let name = props.local_name.clone().unwrap_or_default();
            let advertises = props.services.contains(&service);
            let named = NAME_HINTS.iter().any(|h| name.contains(h));
            if (advertises || named) && p.is_connected().await.unwrap_or(false) {
                out.push((p, name));
            }
        }
    }
    Ok(out)
}

async fn find(manager: &Manager, want: Option<&str>) -> std::result::Result<Peripheral, String> {
    candidates(manager)
        .await?
        .into_iter()
        .find(|(p, _)| want.is_none_or(|w| p_addr(p) == w))
        .map(|(p, _)| p)
        .ok_or_else(|| "no connected keyboard exposing the GaggsKey GATT service".into())
}

async fn run(
    want: Option<String>,
    ready: mpsc::Sender<std::result::Result<String, String>>,
    mut out: amp::UnboundedReceiver<Frame>,
    inbox: mpsc::Sender<std::result::Result<Vec<u8>, String>>,
) {
    let setup = async {
        let manager = Manager::new().await.map_err(|e| e.to_string())?;
        let p = find(&manager, want.as_deref()).await?;
        p.discover_services().await.map_err(|e| e.to_string())?;
        let chrc = p
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == uuid(proto::BT_CHRC_UUID))
            .ok_or("GaggsKey characteristic missing (old firmware or stale GATT cache)")?;
        let notes = p.notifications().await.map_err(|e| e.to_string())?;
        p.subscribe(&chrc).await.map_err(|e| e.to_string())?;
        let name = p.properties().await.ok().flatten().and_then(|pr| pr.local_name).unwrap_or_default();
        let desc = format!("ble {name} {}", p_addr(&p));
        Ok::<_, String>((p, chrc, notes, desc))
    };
    let (p, chrc, mut notes, desc) = match setup.await {
        Ok(v) => v,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(desc));

    loop {
        tokio::select! {
            f = out.recv() => {
                let Some(f) = f else { break };
                // Write-with-response serialises writes (BlueZ rejects overlap
                // with InProgress otherwise).
                if let Err(e) = p.write(&chrc, &f, WriteType::WithResponse).await {
                    let _ = inbox.send(Err(e.to_string()));
                }
            }
            n = notes.next() => match n {
                Some(n) if n.uuid == chrc.uuid => { let _ = inbox.send(Ok(n.value)); }
                Some(_) => {}
                None => { let _ = inbox.send(Err("BLE notification stream closed".into())); break; }
            }
        }
    }
    let _ = p.unsubscribe(&chrc).await;
}

fn p_addr(p: &Peripheral) -> String {
    p.id().to_string()
}

impl BleTransport {
    /// OS-connected keyboards (names as advertised). Blocks ~0.3 s for a scan.
    pub fn list() -> Vec<Device> {
        let job = thread::spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
            rt.block_on(async {
                let manager = Manager::new().await.ok()?;
                let found = candidates(&manager).await.ok()?;
                Some(
                    found
                        .into_iter()
                        .map(|(p, name)| Device { name, via: Via::Ble, id: p_addr(&p) })
                        .collect::<Vec<_>>(),
                )
            })
        });
        job.join().ok().flatten().unwrap_or_default()
    }

    /// Open the peripheral with platform id `id`, or the first candidate.
    pub fn open(id: Option<&str>) -> Result<Self> {
        let want = id.map(str::to_string);
        let (ready_tx, ready_rx) = mpsc::channel();
        let (out_tx, out_rx) = amp::unbounded_channel();
        let (in_tx, in_rx) = mpsc::channel();
        thread::Builder::new()
            .name("gaggskey-ble".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build();
                match rt {
                    Ok(rt) => rt.block_on(run(want, ready_tx, out_rx, in_tx)),
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.to_string()));
                    }
                }
            })
            .map_err(|e| Error::Io(e.to_string()))?;
        let name = ready_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| Error::Io("BLE setup timed out".into()))?
            .map_err(Error::Io)?;
        Ok(Self { tx: out_tx, rx: in_rx, name })
    }
}

impl Transport for BleTransport {
    fn send(&mut self, frame: &Frame) -> Result<()> {
        self.tx.send(*frame).map_err(|_| Error::Io("BLE worker gone".into()))
    }

    fn recv(&mut self, timeout: Option<Duration>) -> Result<Option<Vec<u8>>> {
        let r = match timeout {
            Some(t) => match self.rx.recv_timeout(t) {
                Ok(r) => r,
                Err(mpsc::RecvTimeoutError::Timeout) => return Ok(None),
                Err(_) => return Err(Error::Io("BLE worker gone".into())),
            },
            None => self.rx.recv().map_err(|_| Error::Io("BLE worker gone".into()))?,
        };
        r.map(Some).map_err(Error::Io)
    }

    fn describe(&self) -> String {
        self.name.clone()
    }
}
