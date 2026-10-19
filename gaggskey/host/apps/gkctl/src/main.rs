use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use gaggskey_link::Client;
use gaggskey_proto::{Command, Output, State};

/// GaggsKey client: show keyboard state and send commands.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Print JSON (one object per line for `watch`)
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Keyboard identity and capabilities
    Hello,
    /// Current state, one shot
    Status,
    /// Stream state changes
    Watch,
    /// List layer ids and names
    Layers,
    /// Dump keymap legends (all layers, or one)
    Keymap { layer: Option<u8> },
    /// Switch to layer (layer_to)
    Layer { id: u8 },
    /// Toggle a layer
    Toggle { id: u8 },
    /// Momentarily-style on/off
    On { id: u8 },
    Off { id: u8 },
    /// BLE profile: number, next or prev
    Bt { profile: String },
    /// Clear the bond of the current BLE profile
    BtClear,
    /// Select output
    Out { target: OutArg },
    /// Reboot the keyboard
    Reset,
    /// Reboot into the UF2 bootloader
    Bootloader,
}

#[derive(Clone, ValueEnum)]
enum OutArg {
    Usb,
    Ble,
    Toggle,
}

fn line(s: &State, names: &[String]) -> String {
    let pct = |v: Option<u8>| v.map_or("--".into(), |v| format!("{v}%"));
    let name = names.get(s.layer as usize).filter(|n| !n.is_empty());
    format!(
        "layer {}{}  bat L {} R {}{}  out {}  bt {}{}  wpm {}  {}{}",
        s.layer,
        name.map(|n| format!(" {n}")).unwrap_or_default(),
        pct(s.battery.left),
        pct(s.battery.right),
        if s.periph_connected { "" } else { " (R offline)" },
        match s.output { Output::Usb => "USB", Output::Ble => "BLE" },
        s.ble_profile,
        if s.ble_profile_connected { "*" } else { "" },
        s.wpm,
        if s.caps_lock { "CAPS " } else { "" },
        serde_json::to_value(s.activity).unwrap().as_str().unwrap_or("?"),
    )
}

fn emit<T: serde::Serialize>(json: bool, v: &T, text: impl FnOnce() -> String) {
    if json {
        println!("{}", serde_json::to_string(v).unwrap());
    } else {
        println!("{}", text());
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut c = Client::open_any()?;
    let json = cli.json;

    let cmd = match cli.cmd {
        Cmd::Hello => {
            let h = c.hello()?;
            let via = c.describe();
            emit(json, &h, || {
                format!(
                    "{}  proto v{}  layers {}  bt profiles {}  caps {}  ({via})",
                    h.name, h.proto, h.layers, h.ble_profiles, h.caps.join(",")
                )
            });
            return Ok(());
        }
        Cmd::Status => {
            let s = c.state()?;
            emit(json, &s, || line(&s, &[]));
            return Ok(());
        }
        Cmd::Layers => {
            let h = c.hello()?;
            let names = c.layer_names(h.layers);
            emit(json, &names, || {
                names.iter().enumerate().map(|(i, n)| format!("{i}: {n}")).collect::<Vec<_>>().join("\n")
            });
            return Ok(());
        }
        Cmd::Keymap { layer } => {
            let h = c.hello()?;
            let names = c.layer_names(h.layers);
            let t = std::time::Instant::now();
            let km = c.keymap(&names)?.ok_or_else(|| anyhow::anyhow!("firmware has no keymap introspection"))?;
            if json {
                println!("{}", serde_json::to_string(&km)?);
                return Ok(());
            }
            eprintln!("{} keys, {} layers in {:?}", km.keys.len(), km.layers.len(), t.elapsed());
            for (i, row) in km.layers.iter().enumerate() {
                if layer.is_some_and(|l| l as usize != i) {
                    continue;
                }
                println!("== {i} {}", names.get(i).map(String::as_str).unwrap_or(""));
                for (pos, k) in row.iter().enumerate() {
                    let hold = k.hold.as_deref().map(|h| format!(" / {h}")).unwrap_or_default();
                    println!("  {pos:2} {:<10}{:<12} {}", k.tap, hold, k.raw);
                }
            }
            return Ok(());
        }
        Cmd::Watch => {
            let h = c.hello()?;
            let names = c.layer_names(h.layers);
            let mut s = c.state()?;
            loop {
                emit(json, &s, || line(&s, &names));
                match c.next_push(None)? {
                    Some(n) => s = n,
                    None => continue,
                }
            }
        }
        Cmd::Layer { id } => Command::LayerTo(id),
        Cmd::Toggle { id } => Command::LayerToggle(id),
        Cmd::On { id } => Command::LayerOn(id),
        Cmd::Off { id } => Command::LayerOff(id),
        Cmd::Bt { profile } => match profile.as_str() {
            "next" => Command::BtNext,
            "prev" => Command::BtPrev,
            n => Command::BtSelect(n.parse()?),
        },
        Cmd::BtClear => Command::BtClear,
        Cmd::Out { target } => match target {
            OutArg::Usb => Command::OutSelect(Output::Usb),
            OutArg::Ble => Command::OutSelect(Output::Ble),
            OutArg::Toggle => Command::OutToggle,
        },
        Cmd::Reset => Command::Reset,
        Cmd::Bootloader => Command::Bootloader,
    };
    c.command(cmd)?;
    Ok(())
}
