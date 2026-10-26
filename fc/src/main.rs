use drone_common::domain::{ArmState, RcChannels};

fn main() -> anyhow::Result<()> {
    println!("boot state: {:?}", ArmState::default());
    println!("channels: {:?}", RcChannels::default());

    #[cfg(target_os = "linux")]
    {
        let cores = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
        println!("running on Linux with {cores} cores");
    }

    #[cfg(not(target_os = "linux"))]
    println!("not on Linux: hardware access is disabled in this build");

    return Ok(());
}
