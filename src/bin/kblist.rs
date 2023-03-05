use multiinput::DeviceType;
use multiinput::RawInputManager;

#[path = "../kbutils.rs"]
mod kbutils;
use kbutils::*;

fn main() {
    let app_dir = get_app_dir();
    let aliases = load_aliases(&app_dir);
    let mut manager = RawInputManager::new().unwrap();
    manager.register_devices(DeviceType::Keyboards);
    let devices = manager.get_device_list();
    for device in &devices.keyboards {
        let name = get_keyboard_name(device);
        println!("{} {}", get_alias(name, &aliases), name);
    }
}
