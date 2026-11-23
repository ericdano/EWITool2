//! MIDI port listing and connections (port of the port handling in
//! `MidiHandler.java` / `PortsItemEventHandler.java`), using midir, which
//! talks to CoreMIDI on macOS and WinMM on Windows.
//!
//! Incoming messages arrive on a midir thread and are handed to the UI
//! through a channel; the UI is woken up so it can process them.
//!
//! Ports are identified by a label: the port's name, with " #2", " #3", …
//! added when several ports share a name (macOS names every unnamed external
//! device in Audio MIDI Setup "New external device").

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use eframe::egui;
use ewitool_core::sysex;
use midir::{
    Ignore, MidiInput, MidiInputConnection, MidiInputPort, MidiOutput, MidiOutputConnection,
    MidiOutputPort,
};

const CLIENT: &str = "EWITool2";

/// Give duplicate names a " #n" suffix so every label is unique.
fn unique_labels(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    names
        .into_iter()
        .map(|name| {
            let n = seen.entry(name.clone()).or_default();
            *n += 1;
            if *n == 1 {
                name
            } else {
                format!("{name} #{n}")
            }
        })
        .collect()
}

fn input_ports(input: &MidiInput) -> Vec<(String, MidiInputPort)> {
    let ports = input.ports();
    let names = ports
        .iter()
        .map(|p| input.port_name(p).unwrap_or_else(|_| "(unnamed)".into()));
    unique_labels(names).into_iter().zip(ports).collect()
}

fn output_ports(output: &MidiOutput) -> Vec<(String, MidiOutputPort)> {
    let ports = output.ports();
    let names = ports
        .iter()
        .map(|p| output.port_name(p).unwrap_or_else(|_| "(unnamed)".into()));
    unique_labels(names).into_iter().zip(ports).collect()
}

/// Labels of the MIDI input and output ports currently available.
pub fn list_ports() -> Result<(Vec<String>, Vec<String>), String> {
    let input = MidiInput::new(CLIENT).map_err(|e| e.to_string())?;
    let output = MidiOutput::new(CLIENT).map_err(|e| e.to_string())?;
    let ins = input_ports(&input).into_iter().map(|(l, _)| l).collect();
    let outs = output_ports(&output).into_iter().map(|(l, _)| l).collect();
    Ok((ins, outs))
}

/// Open MIDI connections. Either side may be missing.
#[derive(Default)]
pub struct Midi {
    input: Option<(String, MidiInputConnection<()>)>,
    output: Option<(String, MidiOutputConnection)>,
    received: Option<Receiver<Vec<u8>>>,
}

impl Midi {
    /// Connect to the labelled ports. Returns the connections that worked
    /// and a description of any that didn't.
    pub fn connect(
        in_label: Option<&str>,
        out_label: Option<&str>,
        ctx: &egui::Context,
    ) -> (Self, Vec<String>) {
        let mut midi = Self::default();
        let mut errors = Vec::new();

        if let Some(label) = in_label {
            let ctx = ctx.clone();
            let (tx, rx) = mpsc::channel();
            let on_message = move |msg: &[u8]| {
                // Only SysEx matters to this app; skip note/controller traffic.
                if msg.first() == Some(&0xF0) {
                    let _ = tx.send(msg.to_vec());
                    ctx.request_repaint();
                }
            };
            match open_input(label, on_message) {
                Ok(conn) => {
                    midi.input = Some((label.to_string(), conn));
                    midi.received = Some(rx);
                }
                Err(e) => errors.push(format!("MIDI In \"{label}\": {e}")),
            }
        }
        if let Some(label) = out_label {
            match open_output(label) {
                Ok(conn) => midi.output = Some((label.to_string(), conn)),
                Err(e) => errors.push(format!("MIDI Out \"{label}\": {e}")),
            }
        }
        (midi, errors)
    }

    pub fn in_name(&self) -> Option<&str> {
        self.input.as_ref().map(|(n, _)| n.as_str())
    }

    pub fn out_name(&self) -> Option<&str> {
        self.output.as_ref().map(|(n, _)| n.as_str())
    }

    pub fn is_connected(&self) -> bool {
        self.input.is_some() && self.output.is_some()
    }

    pub fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        let (_, conn) = self.output.as_mut().ok_or("no MIDI Out port is open")?;
        conn.send(bytes).map_err(|e| e.to_string())
    }

    /// Messages received since the last call.
    pub fn take_received(&self) -> Vec<Vec<u8>> {
        self.received
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default()
    }
}

const NOT_FOUND: &str = "port not found (is the device plugged in?)";

fn open_input(
    label: &str,
    mut on_message: impl FnMut(&[u8]) + Send + 'static,
) -> Result<MidiInputConnection<()>, String> {
    let mut input = MidiInput::new(CLIENT).map_err(|e| e.to_string())?;
    // The app needs SysEx, which some backends filter by default.
    input.ignore(Ignore::None);
    let (_, port) = input_ports(&input)
        .into_iter()
        .find(|(l, _)| l == label)
        .ok_or(NOT_FOUND)?;
    input
        .connect(
            &port,
            "EWITool2 In",
            move |_time, msg, _| on_message(msg),
            (),
        )
        .map_err(|e| e.to_string())
}

fn open_output(label: &str) -> Result<MidiOutputConnection, String> {
    let output = MidiOutput::new(CLIENT).map_err(|e| e.to_string())?;
    let (_, port) = output_ports(&output)
        .into_iter()
        .find(|(l, _)| l == label)
        .ok_or(NOT_FOUND)?;
    output
        .connect(&port, "EWITool2 Out")
        .map_err(|e| e.to_string())
}

/// Command-line diagnostic (`ewitool2 --midi-check [text]`): list the ports
/// and listen on every input. With `text`, also send an identity request to
/// the outputs whose label contains it, and print any SysEx replies.
pub fn diagnose(filter: Option<&str>) -> Result<(), String> {
    let (ins, outs) = list_ports()?;
    println!("MIDI In ports:  {ins:?}");
    println!("MIDI Out ports: {outs:?}");
    let Some(filter) = filter else {
        println!("Add part of an Out port's name to send it an identity request.");
        return Ok(());
    };

    let (tx, rx) = mpsc::channel();
    let mut listeners = Vec::new();
    for label in &ins {
        let (tx, port) = (tx.clone(), label.clone());
        match open_input(label, move |msg| {
            let _ = tx.send((port.clone(), msg.to_vec()));
        }) {
            Ok(conn) => listeners.push(conn),
            Err(e) => println!("Could not open In \"{label}\": {e}"),
        }
    }
    for label in outs.iter().filter(|l| l.contains(filter)) {
        match open_output(label)
            .and_then(|mut c| c.send(&sysex::DEVICE_ID_REQUEST).map_err(|e| e.to_string()))
        {
            Ok(()) => println!("Sent identity request to \"{label}\""),
            Err(e) => println!("Could not send to \"{label}\": {e}"),
        }
    }

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut replies = 0;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok((port, msg)) = rx.recv_timeout(left) else {
            break;
        };
        if msg.first() != Some(&0xF0) {
            continue;
        }
        replies += 1;
        let hex: Vec<String> = msg.iter().map(|b| format!("{b:02x}")).collect();
        let id = sysex::parse_device_id(&msg);
        println!("Reply on \"{port}\": {} -> {id:?}", hex.join(" "));
    }
    if replies == 0 {
        println!("No SysEx replies within 3 seconds.");
    }
    drop(listeners);
    Ok(())
}

/// A plain blocking connection to the device on one In/Out port pair, for
/// command-line tools.
pub struct Link {
    _input: MidiInputConnection<()>,
    output: MidiOutputConnection,
    rx: Receiver<Vec<u8>>,
}

impl Link {
    pub fn open(port: &str) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        let input = open_input(port, move |msg| {
            let _ = tx.send(msg.to_vec());
        })
        .map_err(|e| format!("MIDI In \"{port}\": {e}"))?;
        let output = open_output(port).map_err(|e| format!("MIDI Out \"{port}\": {e}"))?;
        Ok(Self {
            _input: input,
            output,
            rx,
        })
    }

    pub fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.output.send(bytes).map_err(|e| e.to_string())
    }

    /// Run a fetch to completion, calling `progress` after each patch.
    pub fn fetch(
        &mut self,
        mut fetcher: ewitool_core::fetch::Fetcher,
        mut progress: impl FnMut(usize),
    ) -> Result<ewitool_core::Bank, String> {
        use ewitool_core::fetch::Step;
        loop {
            match fetcher.poll(Instant::now()) {
                Step::Send(bytes) => self.send(&bytes)?,
                Step::Wait => {
                    if let Ok(msg) = self.rx.recv_timeout(Duration::from_millis(10)) {
                        if fetcher.receive(&msg) {
                            progress(fetcher.received());
                        }
                    }
                }
                Step::Done => return Ok(fetcher.into_bank()),
                Step::Failed(why) => return Err(why),
            }
        }
    }
}

/// Command-line fetch (`ewitool2 --fetch <port> <file.syx>`): read all 100
/// patches from the EWI on the In and Out ports labelled `port` and save
/// them as a new `.syx` file. Only reads from the EWI.
pub fn fetch_to_file(port: &str, file: &std::path::Path) -> Result<(), String> {
    let started = Instant::now();
    let bank = Link::open(port)?.fetch(ewitool_core::fetch::Fetcher::new(), |n| {
        if n % 10 == 0 {
            println!("{n} of 100");
        }
    })?;
    ewitool_core::patch_set::save_syx(file, &bank.patches()).map_err(|e| e.to_string())?;
    println!(
        "Fetched {} patches in {:.1}s into {}",
        bank.len(),
        started.elapsed().as_secs_f32(),
        file.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::unique_labels;

    #[test]
    fn duplicate_port_names_get_numbered() {
        let names = [
            "New external device",
            "Korg M1",
            "New external device",
            "New external device",
        ];
        assert_eq!(
            unique_labels(names.map(String::from)),
            [
                "New external device",
                "Korg M1",
                "New external device #2",
                "New external device #3"
            ]
        );
    }
}
