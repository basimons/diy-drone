use drone_common::{crsf, domain::RcChannels};

fn main() -> anyhow::Result<()> {
    println!("default channels: {:?}", RcChannels::default());
    println!("CRSF RC frame type: {:#04x}", crsf::FRAME_TYPE_RC_CHANNELS_PACKED);

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
