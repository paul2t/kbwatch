#![allow(dead_code)]

use std::collections::HashMap;
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

use multiinput::DeviceType;
use multiinput::RawInputManager;

use crossbeam_channel::{bounded, select, tick, Receiver};

use hostname;
use reqwest;
use serde_json::json;

mod kbutils;
use crate::kbutils::*;

fn main() {
    let app_dir = get_app_dir();
    if !app_dir.exists() {
        std::fs::create_dir(&app_dir).expect(&format!(
            "Unable to create directory {}",
            app_dir.to_string_lossy()
        ));
    }
    let config = load_config(&app_dir);

    init_logging(&app_dir);

    let computer_name = hostname::get()
        .expect("Unable to retrieve hostname")
        .to_string_lossy()
        .to_string();

    if let Err(e) = send_message(&config, &format!("starting on {computer_name}")) {
        error!("{:#?}", e);
    }

    // Load aliases
    let aliases = load_aliases(&app_dir);
    for (name, alias) in &aliases {
        info!("{} => {}", name, alias);
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
    let mut device_list: Vec<String> = Vec::new();

    // App events
    let ctrlc_events = ctrl_channel().expect("Unable to catch CTRL+C");
    let ticks_min = tick(Duration::from_secs(60));
    let ticks_sec = tick(Duration::from_secs(1));

    watch_keyboard_changes(&config, &mut device_list, &aliases, &computer_name);

    loop {
        select! {
            recv(ctrlc_events) -> _ => {
                if let Err(e) = send_message(&config, &format!("stopping on {computer_name}")) {
                    error!("{:#?}", e);
                }
                std::process::exit(1);
            }
            recv(ticks_sec) -> _ => {
                watch_keyboard_changes(&config, &mut device_list, &aliases, &computer_name);
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
    device_list: &mut Vec<String>,
    aliases: &HashMap<String, String>,
    computer_name: &str,
) {
    let mut manager = RawInputManager::new().unwrap();
    manager.register_devices(DeviceType::Keyboards);
    let devices = manager.get_device_list();

    let mut new_devices: Vec<String> = Vec::new();
    let mut removed_devices: Vec<usize> = Vec::new();

    for (i, device) in device_list.iter().enumerate() {
        let mut found = false;
        for keyboard in &devices.keyboards {
            if *device == get_keyboard_name(keyboard) {
                found = true;
                break;
            }
        }

        if !found {
            removed_devices.push(i);
        }
    }

    for keyboard in &devices.keyboards {
        let mut found = false;
        for device in device_list.iter() {
            if *device == get_keyboard_name(keyboard) {
                found = true;
                break;
            }
        }
        if !found {
            new_devices.push(get_keyboard_name(keyboard).to_string());
        }
    }

    let mut new_aliases: Vec<String> = Vec::new();
    let mut removed_aliases: Vec<String> = Vec::new();

    if removed_devices.len() > 0 {
        for device_index in removed_devices.iter().rev() {
            if *device_index >= device_list.len() {
                continue;
            }
            let alias = get_alias(&device_list[*device_index], aliases);
            if !removed_aliases.iter().any(|x| *x == alias) {
                if alias != "INTERNAL" {
                    if let Err(e) = send_message(
                        config,
                        &format!("keyboard {alias} unplugged from {computer_name}"),
                    ) {
                        error!("{:#?}", e);
                    }
                }
                removed_aliases.push(alias.to_string());
            }
            device_list.remove(*device_index);
        }
    }

    if new_devices.len() > 0 {
        for device in &new_devices {
            let alias = get_alias(&*device, aliases);
            if !new_aliases.iter().any(|x| *x == alias) {
                if alias != "INTERNAL" {
                    if let Err(e) = send_message(
                        config,
                        &format!("keyboard {alias} plugged in {computer_name}"),
                    ) {
                        error!("{:#?}", e);
                    }
                }
                new_aliases.push(alias.to_string());
            }
        }
        device_list.append(&mut new_devices);
    }
}

#[derive(Default)]
struct KBConfig {
    telegram_bot_token: String,
    telegram_chat_id: String,
}

fn load_config(app_dir: &Path) -> KBConfig {
    let mut path = app_dir.to_path_buf();
    path.push("config.txt");
    let configs = load_key_value_file(&path);

    let mut telegram_bot_token = configs
        .get("TELEGRAM_BOT_TOKEN")
        .map(|x| x.to_string())
        .unwrap_or_default();
    let mut telegram_chat_id = configs
        .get("TELEGRAM_CHAT_ID")
        .map(|x| x.to_string())
        .unwrap_or_default();

    while telegram_bot_token.is_empty() {
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

    let config_str =
        format!("TELEGRAM_BOT_TOKEN {telegram_bot_token}\nTELEGRAM_CHAT_ID {telegram_chat_id}\n");
    let mut file = File::create(&path).expect(&format!(
        "Unable to write config file : {}",
        path.to_string_lossy()
    ));
    file.write(config_str.as_bytes()).expect(&format!(
        "Error while saving config to file {}",
        path.to_string_lossy()
    ));
    eprintln!("Config saved in {}", path.to_string_lossy());

    KBConfig {
        telegram_bot_token,
        telegram_chat_id,
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
    if message.len() <= 0 {
        bail!("Empty message to send");
    }
    let json = json!(message);
    info!("{message}");

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
