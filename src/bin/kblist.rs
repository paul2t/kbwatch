#[path = "../kbdevice.rs"]
mod kbdevice;
use crate::kbdevice::*;

fn main() {
    for usb_device in rusb::devices().unwrap().iter() {
        let desc = match usb_device.device_descriptor() {
            Ok(desc) => desc,
            Err(_) => continue, // Ignore devices that don't have a descriptor.
        };
        let handle = match usb_device.open() {
            Ok(h) => h,
            Err(_) => continue, // Ignore devices that we cannot open.
        };

        let dev = Device::new(&usb_device, &desc);
        let device = DeviceInfos::new(dev, &handle, &desc);

        println!(
            "Bus {:03} | Address {:03} | ID {:04x}:{:04x} | {} | {} | {}",
            device.device.bus,
            device.device.address,
            device.device.vendor_id,
            device.device.product_id,
            device.manufacturer,
            device.product,
            device.serial
        );
    }
}
