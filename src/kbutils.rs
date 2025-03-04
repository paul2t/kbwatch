use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

type KeyValues = HashMap<String, Vec<String>>;
type Aliases = KeyValues;

pub fn load_key_value_file(path: &Path) -> KeyValues {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let mut values: KeyValues = HashMap::new();
    for line in content.lines() {
        if let Some((key, value)) = line.split_once(' ') {
            if !key.is_empty() && !value.is_empty() {
                let key = key.trim().to_string();
                let value = value.trim().to_string();
                if !values.contains_key(&key) {
                    values.insert(key.clone(), Vec::new());
                }
                values.get_mut(&key).unwrap().push(value);
            }
        }
    }
    values
}

pub fn load_aliases(app_dir: &Path) -> Aliases {
    let mut alias_path = app_dir.to_path_buf();
    alias_path.push("alias.txt");
    load_key_value_file(&alias_path)
}

fn get_alias_opt<'a>(name: &'a str, aliases: &'a Aliases) -> Option<&'a str> {
    if let Some(alias) = aliases.get(name) {
        if let Some(alias) = alias.first() {
            Some(&alias[..])
        } else {
            None
        }
    } else {
        None
    }
}

fn get_alias_raw<'a>(name: &'a str, aliases: &'a Aliases) -> &'a str {
    if let Some(alias) = get_alias_opt(name, aliases) {
        alias
    } else {
        name
    }
}

pub fn get_alias<'a>(name: &'a str, aliases: &'a Aliases) -> &'a str {
    if let Some(alias) = get_alias_opt(name, aliases) {
        alias
    } else {
        let name = name.trim_start_matches(r"\\?\");
        if !name.starts_with(r"HID#VID_") {
            get_alias_raw("INTERNAL", aliases)
        } else {
            let name = &name[4..];
            let name: &str = match name.rsplit_once('#') {
                Some((name, _remainder)) => name,
                None => name,
            };
            let mut split = name.split('&');
            let vid = split.next().unwrap_or_default();
            let pid = split.next().unwrap_or_default();
            if !vid.is_empty() && !pid.is_empty() {
                let name = &name[..vid.len() + 1 + pid.len()];
                get_alias_raw(name, aliases)
            } else {
                get_alias_raw(name, aliases)
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
