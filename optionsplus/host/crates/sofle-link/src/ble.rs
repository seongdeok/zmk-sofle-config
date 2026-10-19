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
use btleplug::platform::{Manager, Peripheral};
use futures::StreamExt;
use sofle_proto::{self as proto, Frame};
use tokio::sync::mpsc as amp;
use uuid::Uuid;

use crate::{Error, Result, Transport};

pub struct BleTransport {
    tx: amp::UnboundedSender<Frame>,
    rx: mpsc::Receiver<std::result::Result<Vec<u8>, String>>,
    name: String,
}

fn uuid(s: &str) -> Uuid {
    Uuid::parse_str(s).expect("valid uuid")
}

async fn find(manager: &Manager) -> std::result::Result<Peripheral, String> {
    let service = uuid(proto::BT_SERVICE_UUID);
    for adapter in manager.adapters().await.map_err(|e| e.to_string())? {
        // Already-connected devices are listed by the OS without scanning on
        // BlueZ; a short filtered scan also lets CoreBluetooth surface them.
        let _ = adapter.start_scan(ScanFilter { services: vec![service] }).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let _ = adapter.stop_scan().await;
        for p in adapter.peripherals().await.map_err(|e| e.to_string())? {
            let Ok(Some(props)) = p.properties().await else { continue };
            let advertises = props.services.contains(&service);
            let named = props.local_name.as_deref().is_some_and(|n| n.contains("Sofle"));
            if (advertises || named) && p.is_connected().await.unwrap_or(false) {
                return Ok(p);
            }
        }
    }
    Err("no connected keyboard exposing the Options+ GATT service".into())
}

async fn run(
    ready: mpsc::Sender<std::result::Result<String, String>>,
    mut out: amp::UnboundedReceiver<Frame>,
    inbox: mpsc::Sender<std::result::Result<Vec<u8>, String>>,
) {
    let setup = async {
        let manager = Manager::new().await.map_err(|e| e.to_string())?;
        let p = find(&manager).await?;
        p.discover_services().await.map_err(|e| e.to_string())?;
        let chrc = p
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == uuid(proto::BT_CHRC_UUID))
            .ok_or("Options+ characteristic missing (old firmware or stale GATT cache)")?;
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
    pub fn open() -> Result<Self> {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (out_tx, out_rx) = amp::unbounded_channel();
        let (in_tx, in_rx) = mpsc::channel();
        thread::Builder::new()
            .name("sofle-ble".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build();
                match rt {
                    Ok(rt) => rt.block_on(run(ready_tx, out_rx, in_tx)),
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
