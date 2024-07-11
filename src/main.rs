#![allow(dead_code)]

use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use log::{error, info};
use serde_json::Value;
use simple_log::LogConfigBuilder;

use chrono::Local;

use anyhow::bail;
use anyhow::Result;

use crossbeam_channel::{bounded, select, tick, Receiver};

use serde_json::json;

mod kbutils;
use crate::kbutils::*;

mod kbdevice;
use crate::kbdevice::*;

fn main() {
    let app_dir = get_app_dir();
    if !app_dir.exists() {
        std::fs::create_dir(&app_dir)
            .unwrap_or_else(|_| panic!("Unable to create directory {}", app_dir.to_string_lossy()));
    }
    let config = load_config(&app_dir);

    init_logging(&app_dir);

    info!("Ignore {} devices:", config.ignored_devices.len());
    for device in &config.ignored_devices {
        info!("Ignore device: {device}");
    }

    let computer_name = hostname::get()
        .expect("Unable to retrieve hostname")
        .to_string_lossy()
        .to_string();

    if let Err(e) = send_message(&config, &format!("starting on {computer_name}")) {
        error!("{:#?}", e);
    }

    // Log still running in separate log file
    let mut run_log = app_dir.to_path_buf();
    run_log.push("running.log");
    let mut running_file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(run_log)
        .expect("Unable to save the running logs");

    // App data
    let mut device_list: Vec<DeviceInfos> = Vec::new();

    // App events
    let ctrlc_events = ctrl_channel().expect("Unable to catch CTRL+C");
    let ticks_min = tick(Duration::from_secs(60));
    let ticks_sec = tick(Duration::from_secs(1));

    watch_keyboard_changes(&config, &mut device_list, &computer_name);

    loop {
        select! {
            recv(ctrlc_events) -> _ => {
                if let Err(e) = send_message(&config, &format!("stopping on {computer_name}")) {
                    error!("{:#?}", e);
                }
                std::process::exit(1);
            }
            recv(ticks_sec) -> _ => {
                watch_keyboard_changes(&config, &mut device_list, &computer_name);
            }
            recv(ticks_min) -> _ => {
                let time = Local::now();
                _ = writeln!(&mut running_file, "{} : still running", time.format("%Y-%m-%d %H:%M:%S"));
                _ = running_file.flush();
            }
        }
    }
}

fn watch_keyboard_changes(
    config: &KBConfig,
    device_list: &mut Vec<DeviceInfos>,
    computer_name: &str,
) {
    let usb_devices = match rusb::devices() {
        Ok(devices) => {
            let mut list = Vec::with_capacity(devices.len());
            for d in devices.iter() {
                list.push(d);
            }
            list
        }
        Err(e) => {
            error!("{e}");
            Vec::new()
        }
    };

    let mut new_devices: Vec<DeviceInfos> = Vec::new();
    let mut devices: Vec<Device> = Vec::with_capacity(usb_devices.len());
    let mut removed_devices: Vec<usize> = Vec::new();

    // Find new devices
    for device in &usb_devices {
        let desc = match device.device_descriptor() {
            Ok(desc) => desc,
            Err(_) => continue, // Ignore devices that don't have a descriptor.
        };
        let handle = match device.open() {
            Ok(h) => h,
            Err(_) => continue, // Ignore devices that we cannot open.
        };

        let dev = Device::new(device, &desc);
        devices.push(dev);

        if !device_list.iter().map(|x| x.device).any(|x| x == dev) {
            let infos = DeviceInfos::new(dev, &handle, &desc);
            if !config
                .ignored_devices
                .contains(&infos.get_name().to_uppercase())
                && !config.ignored_devices.contains(
                    &format!("{:04x}:{:04x}", dev.vendor_id, dev.product_id).to_uppercase(),
                )
            {
                new_devices.push(infos);
            }
        }
    }

    // Find removed devices
    for (i, device) in device_list.iter().enumerate() {
        if !devices.contains(&device.device) {
            removed_devices.push(i);
        }
    }

    if !removed_devices.is_empty() {
        for device_index in removed_devices.iter().rev().copied() {
            if device_index >= device_list.len() {
                error!(
                    "Invalid index {} when device_list.len() == {}",
                    device_index,
                    device_list.len()
                );
                continue;
            }
            let device = &device_list[device_index];
            info!(
                "Removed device: Bus {:03} | Address {:03} | ID {:04x}:{:04x} | {} | {} | {}",
                device.device.bus,
                device.device.address,
                device.device.vendor_id,
                device.device.product_id,
                device.manufacturer,
                device.product,
                device.serial
            );

            let name = device.get_name();
            if let Err(e) = send_message(config, &format!("{name} unplugged from {computer_name}"))
            {
                error!("{:#?}", e);
            }
            device_list.remove(device_index);
        }
    }

    if !new_devices.is_empty() {
        for device in &new_devices {
            info!(
                "New device: Bus {:03} | Address {:03} | ID {:04x}:{:04x} | {} | {} | {}",
                device.device.bus,
                device.device.address,
                device.device.vendor_id,
                device.device.product_id,
                device.manufacturer,
                device.product,
                device.serial
            );
            let name = device.get_name();
            if let Err(e) = send_message(config, &format!("{name} plugged in {computer_name}")) {
                error!("{:#?}", e);
            }
        }
        device_list.append(&mut new_devices);
    }
}

#[derive(Default)]
struct KBConfig {
    telegram_bot_token: String,
    telegram_chat_id: String,
    ignored_devices: Vec<String>,
}

fn load_config(app_dir: &Path) -> KBConfig {
    let mut path = app_dir.to_path_buf();
    path.push("config.txt");
    let configs = load_key_value_file(&path);

    let mut telegram_bot_token = configs
        .get("TELEGRAM_BOT_TOKEN")
        .map(|x| x.first().map(|it| it.to_string()).unwrap_or_default())
        .unwrap_or_default();
    let mut telegram_chat_id = configs
        .get("TELEGRAM_CHAT_ID")
        .map(|x| x.first().map(|it| it.to_string()).unwrap_or_default())
        .unwrap_or_default();
    let ignored_devices = configs
        .get("IGNORE")
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|it| it.to_uppercase())
        .collect();

    let mut config_changed = false;

    while telegram_bot_token.is_empty() {
        config_changed = true;
        println!("No bot token found. What token do you want to use ? https://telegram.me/BotFather to create a new one.");
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap_or_default();
        let line = line.trim();
        // eprintln!("{line}");
        if line.is_empty() {
            continue;
        }
        let url = format!("https://api.telegram.org/bot{}/getUpdates", line);
        // eprintln!("url = {url}");
        let resp = reqwest::blocking::get(url);
        if let Err(e) = resp {
            error!("{e}");
            continue;
        }
        let resp = resp.unwrap();
        if resp.status() != reqwest::StatusCode::OK {
            error!("Invalid token");
            continue;
        }
        telegram_bot_token = line.to_string();
    }

    while telegram_chat_id.is_empty() {
        config_changed = true;
        println!("No chat id found. Send a message to the bot to initialize the chat id.");

        let mut first = true;
        loop {
            if !first {
                std::thread::sleep(Duration::from_secs(1));
            }
            first = false;

            let resp = reqwest::blocking::get(format!(
                "https://api.telegram.org/bot{telegram_bot_token}/getUpdates"
            ));
            if let Err(e) = resp {
                error!("{e}");
                continue;
            }
            let resp = resp.unwrap();
            let status = resp.status();
            let text = resp.text();
            if let Err(e) = text {
                error!("{}", e);
                continue;
            }
            let text = text.unwrap();
            if status != reqwest::StatusCode::OK {
                error!("Code {} : {}", status.as_str(), text);
                continue;
            }
            // eprintln!("text {}", text);
            let json: Value = serde_json::from_str(&text).unwrap();
            if !json["ok"].as_bool().unwrap_or(false) {
                error!("{}", &text);
                continue;
            }
            // eprintln!("json {:?}", json["result"][0]["message"]["chat"]["id"]);
            let jresult = json["result"].as_array();
            if jresult.is_none() {
                error!("No result in the response : {}", text);
                continue;
            }
            let jresult = jresult.unwrap();
            if jresult.is_empty() {
                continue;
            } // No message received yet.
            for jmsg in jresult {
                let jmsg = &jmsg["message"];
                if jmsg.is_null() {
                    continue;
                }
                let chat_id = jmsg["chat"]["id"].as_i64();
                if chat_id.is_none() {
                    continue;
                }
                let chat_id = chat_id.unwrap();
                let first_name = jmsg["from"]["first_name"].as_str().unwrap_or_default();
                let last_name = jmsg["from"]["last_name"].as_str().unwrap_or_default();
                telegram_chat_id = format!("{chat_id}");
                eprintln!("Received message from {chat_id} : {first_name} {last_name}");
                break;
            }

            if !telegram_chat_id.is_empty() {
                break;
            }
        }
    }

    if config_changed {
        let config_str = format!(
            "TELEGRAM_BOT_TOKEN {telegram_bot_token}\nTELEGRAM_CHAT_ID {telegram_chat_id}\n"
        );
        let mut file = File::create(&path)
            .unwrap_or_else(|_| panic!("Unable to write config file : {}", path.to_string_lossy()));
        _ = file.write(config_str.as_bytes()).unwrap_or_else(|_| {
            panic!(
                "Error while saving config to file {}",
                path.to_string_lossy()
            )
        });
        eprintln!("Config saved in {}", path.to_string_lossy());
    }

    KBConfig {
        telegram_bot_token,
        telegram_chat_id,
        ignored_devices,
    }
}

fn init_logging(app_path: &Path) {
    // Init logging
    let mut path = app_path.to_path_buf();
    path.push("kbwatch.log");
    println!("Logging in {}", path.display());
    let config = LogConfigBuilder::builder()
        .level("info")
        .path(path.to_string_lossy())
        .build();
    simple_log::new(config).expect("Unable to initialize the logging");
}

fn ctrl_channel() -> Result<Receiver<()>, ctrlc::Error> {
    let (sender, receiver) = bounded(100);
    ctrlc::set_handler(move || {
        let _ = sender.send(());
    })?;

    Ok(receiver)
}

/*
curl -X POST \
     -H 'Content-Type: application/json' \
     -d '{"chat_id": "$KBWATCH_TELEGRAM_CHAT_ID", "text": "This is a test from curl"}' \
     "https://api.telegram.org/bot$KBWATCH_TELEGRAM_BOT_TOKEN/sendMessage"
      */
fn send_message_ex(config: &KBConfig, message: &str, silent: bool) -> Result<()> {
    if message.is_empty() {
        bail!("Empty message to send");
    }
    let json = json!(message);
    info!("send message: {message}");

    let data_str = format!(
        "{{\"chat_id\": \"{}\", \"text\": {json}, \"disable_notification\": {silent}}}",
        &config.telegram_chat_id
    );
    // println!("{}", &data_str);
    let url = format!(
        "https://api.telegram.org/bot{}/sendMessage",
        &config.telegram_bot_token
    );

    let client = reqwest::blocking::Client::new();
    let _res = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(data_str)
        .send()?;

    // println!("_res={}", res.text().unwrap_or_default());

    Ok(())
}

fn send_message(config: &KBConfig, message: &str) -> Result<()> {
    send_message_ex(config, message, false)
}
fn send_message_silent(config: &KBConfig, message: &str) -> Result<()> {
    send_message_ex(config, message, true)
}
