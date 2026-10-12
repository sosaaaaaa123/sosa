//! "Save As Default": le impostazioni scelte dall'utente come punto di
//! partenza di ogni nuova istanza di Sosa Tune.
//!
//! Sono salvate in un piccolo file JSON (id del parametro -> valore) nella
//! cartella delle preferenze dell'utente:
//! - macOS: `~/Library/Application Support/Sosa Tune/default.json`
//! - Windows: `%APPDATA%\Sosa Tune\default.json`
//! - Linux: `~/.config/sosa-tune/default.json`
//!
//! Quando il plugin viene creato, questi valori diventano i "default" dei
//! parametri: una nuova istanza parte cosi', e il doppio clic su una
//! manopola torna a questo valore. I progetti salvati non cambiano: il DAW
//! ripristina i loro valori come sempre.

use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(test)]
thread_local! {
    /// Nelle prove: cartella temporanea al posto di quella vera dell'utente.
    pub static TEST_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Il file delle impostazioni predefinite (se si sa dove metterlo).
pub fn path() -> Option<PathBuf> {
    #[cfg(test)]
    {
        TEST_DIR.with(|d| d.borrow().as_ref().map(|dir| dir.join("default.json")))
    }
    #[cfg(not(test))]
    {
        let home = |var: &str| std::env::var_os(var).map(PathBuf::from);
        #[cfg(target_os = "macos")]
        let dir = home("HOME").map(|h| h.join("Library/Application Support/Sosa Tune"));
        #[cfg(target_os = "windows")]
        let dir = home("APPDATA").map(|a| a.join("Sosa Tune"));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let dir = home("XDG_CONFIG_HOME")
            .or_else(|| home("HOME").map(|h| h.join(".config")))
            .map(|c| c.join("sosa-tune"));
        dir.map(|d| d.join("default.json"))
    }
}

/// Valori salvati (vuoto se non c'e' niente o il file non si legge).
pub fn load() -> HashMap<String, f32> {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str::<HashMap<String, f32>>(&text).ok())
        .map(|map| map.into_iter().filter(|(_, v)| v.is_finite()).collect())
        .unwrap_or_default()
}

/// Salva i valori (id del parametro -> valore "vero", non normalizzato).
pub fn save(values: &HashMap<String, f32>) -> std::io::Result<()> {
    let path = path().ok_or_else(|| std::io::Error::other("cartella delle preferenze sconosciuta"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(values).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

/// Torna ai valori di fabbrica (cancella il file).
pub fn clear() -> std::io::Result<()> {
    match path() {
        Some(p) if p.exists() => std::fs::remove_file(p),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_loads_and_clears() {
        let dir = std::env::temp_dir().join(format!("sosatune-default-{}", std::process::id()));
        TEST_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
        let mut values = HashMap::new();
        values.insert("retune".to_owned(), 42.0);
        values.insert("key".to_owned(), 9.0);
        save(&values).unwrap();
        assert_eq!(load(), values);
        clear().unwrap();
        assert!(load().is_empty());
        TEST_DIR.with(|d| *d.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
