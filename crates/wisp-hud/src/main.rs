// SPDX-License-Identifier: MIT
mod client;

fn main() -> std::io::Result<()> {
    let path = client::socket_path();
    let mut stream = client::connect(&path)?;
    eprintln!("wisp-hud: connected to {}", path.display());

    while let Some(item) = stream.next_snapshot() {
        match item {
            Ok(snap) => println!(
                "seq={} lines={} kills={} ts={}",
                snap.seq, snap.lines_ingested, snap.session_kills, snap.ts
            ),
            Err(e) => {
                eprintln!("wisp-hud: {e}");
                std::process::exit(1);
            }
        }
    }
    eprintln!("wisp-hud: daemon closed the connection");
    Ok(())
}
