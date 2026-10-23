use drone_common::{domain::RcChannels, protocol};

fn main() -> anyhow::Result<()> {
    println!("drone-client says: {}", drone_common::hello());
    println!("default channels: {:?}", RcChannels::default());
    println!("CRSF RC frame type: {:#04x}", protocol::FRAME_TYPE_RC_CHANNELS_PACKED);

    let ports = serialport::available_ports()?;
    println!("serial ports found: {}", ports.len());
    for port in &ports {
        println!("  {}", port.port_name);
    }

    match gilrs::Gilrs::new() {
        Ok(gilrs) => println!("gamepads connected: {}", gilrs.gamepads().count()),
        Err(e) => println!("gamepad support unavailable: {e}"),
    }

    return Ok(());
}
