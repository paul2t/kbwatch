use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

pub fn load_key_value_file(path: &Path) -> HashMap<String, String> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let mut values: HashMap<String, String> = HashMap::new();
    for line in content.lines() {
        if let Some((key, value)) = line.split_once(' ') {
            if key.len() > 0 && value.len() > 0 {
                values.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
    }
    values
}

pub fn load_aliases(app_dir: &Path) -> HashMap<String, String> {
    let mut alias_path = app_dir.to_path_buf();
    alias_path.push("alias.txt");
    load_key_value_file(&alias_path)
}

fn get_alias_raw<'a>(name: &'a str, aliases: &'a HashMap<String, String>) -> &'a str {
    if let Some(alias) = aliases.get(name) {
        &alias[..]
    } else {
        name
    }
}

pub fn get_alias<'a>(name: &'a str, aliases: &'a HashMap<String, String>) -> &'a str {
    if let Some(alias) = aliases.get(name) {
        &alias[..]
    } else {
        let name = name.trim_start_matches(r"\\?\");
        if !name.starts_with(r"HID#VID_") {
            return get_alias_raw("INTERNAL", aliases);
        } else {
            let name = &name[4..];
            let name: &str = match name.rsplit_once('#') {
                Some((name, _remainder)) => name,
                None => name,
            };
            let mut split = name.split('&').into_iter();
            let vid = split.next().unwrap_or_default();
            let pid = split.next().unwrap_or_default();
            if !vid.is_empty() && !pid.is_empty() {
                let name = &name[..vid.len() + 1 + pid.len()];
                return get_alias_raw(name, aliases);
            } else {
                return get_alias_raw(name, aliases);
            }
        }
    }
}

pub fn get_app_dir() -> PathBuf {
    use directories::BaseDirs;
    let mut app_dir = PathBuf::new();
    if let Some(base_dirs) = BaseDirs::new() {
        app_dir.push(base_dirs.cache_dir());
    }
    app_dir.push("kbwatch");
    app_dir
}
