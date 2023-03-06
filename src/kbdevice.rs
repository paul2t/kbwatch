#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct Device {
    pub bus: u8,
    pub address: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub class_code: u8,
}

#[derive(Default, Clone)]
pub struct DeviceInfos {
    pub device: Device,
    pub manufacturer: String,
    pub product: String,
    pub serial: String,
}

impl Device {
    pub fn new<T: rusb::UsbContext>(
        device: &rusb::Device<T>,
        desc: &rusb::DeviceDescriptor,
    ) -> Device {
        Device {
            bus: device.bus_number(),
            address: device.address(),
            vendor_id: desc.vendor_id(),
            product_id: desc.product_id(),
            class_code: desc.class_code(),
        }
    }
}

impl DeviceInfos {
    pub fn new<T: rusb::UsbContext>(
        device: Device,
        handle: &rusb::DeviceHandle<T>,
        desc: &rusb::DeviceDescriptor,
    ) -> DeviceInfos {
        DeviceInfos {
            device,
            manufacturer: handle
                .read_manufacturer_string_ascii(&desc)
                .unwrap_or_default(),
            product: handle.read_product_string_ascii(&desc).unwrap_or_default(),
            serial: handle
                .read_serial_number_string_ascii(&desc)
                .unwrap_or_default(),
        }
    }

    // Used but for some reason rustc thinks it isn't
    #[allow(dead_code)]
    pub fn get_name(&self) -> String {
        if !self.product.is_empty() {
            return self.product.clone();
        } else if !self.manufacturer.is_empty() {
            return self.manufacturer.clone();
        } else {
            return format!(
                "{:04X}:{:04X}",
                self.device.vendor_id, self.device.product_id
            );
        }
    }
}
