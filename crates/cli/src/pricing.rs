//! `peektrace pricing`: which price table cost estimates use, and the
//! opt-in `pricing update` that downloads a newer one from models.dev.

use std::path::{Path, PathBuf};
use std::time::Duration;

use peektrace_core::model_provider::providers_snapshot;
use peektrace_core::pricing::{
    self, LocalTable, MODELS_DEV_URL, Table, from_models_dev, local_table_path, parse_table,
    read_local_table, table_info, table_json, table_label,
};

use crate::args::PricingUpdateArgs;
use crate::ls::UserError;
use crate::upgrade::release::{self, FetchError, get_bytes_capped};

/// models.dev's `api.json` is a few MB; this bounds a bad response.
const MAX_DOWNLOAD: u64 = 64 << 20;
const TIMEOUT: Duration = Duration::from_secs(60);

fn var(k: &str) -> Option<String> {
    std::env::var_os(k).map(|v| v.to_string_lossy().into_owned())
}

/// The downloaded table's path: `$PEEKTRACE_DIR/prices.json`, else
/// `~/.peektrace/prices.json`.
pub fn local_path() -> PathBuf {
    let home = std::env::home_dir()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_default();
    local_table_path(var("PEEKTRACE_DIR").as_deref(), &home)
}

/// Price from the downloaded table when it is newer than the built-in one.
pub fn use_local() {
    pricing::use_local_table(&local_path());
}

/// What `pricing` prints.
pub fn show(path: &Path) -> Vec<String> {
    let info = table_info();
    let local = match read_local_table(path) {
        LocalTable::Missing => "none, `peektrace pricing update` downloads one".to_owned(),
        LocalTable::Invalid(e) => format!("rejected, not used: {e}"),
        LocalTable::NotNewer(t) => {
            format!("fetched {}, not newer than built-in, not used", t.fetched)
        }
        LocalTable::Newer(t) => format!("fetched {}", t.fetched),
    };
    let built_in =
        pricing::built_in_table().map_or_else(|e| format!("unreadable: {e}"), |t| t.fetched);
    vec![
        format!("Prices: {}", table_label(info)),
        format!(
            "Models: {} from the table, {} hand-written overrides",
            info.models, info.overrides
        ),
        format!("Built-in table: models.dev {built_in}"),
        format!("Downloaded table: {}: {local}", path.display()),
    ]
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let agent = release::agent(Some(TIMEOUT));
    get_bytes_capped(&agent, url, &[], MAX_DOWNLOAD).map_err(|e| match e {
        FetchError::Status(n) => format!("responded {n}"),
        FetchError::Other(m) => m,
    })
}

fn today() -> String {
    jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date()
        .to_string()
}

/// Write through a temp file beside `path`, so a reader never sees half.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// True when the file at `path` already holds `table`'s models.
fn same_models(path: &Path, table: &Table) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| parse_table(&t).ok())
        .is_some_and(|old| old.models == table.models)
}

/// The model maker snapshot for `path`, built from `api` with the file's
/// current lines kept for makers models.dev dropped.
fn build_providers(
    path: &Path,
    api: &str,
    print: &mut dyn FnMut(&str),
) -> Result<String, UserError> {
    let previous = std::fs::read_to_string(path).unwrap_or_default();
    let snap = providers_snapshot(api, &previous)
        .map_err(|e| UserError::Invalid(format!("Not a usable models.dev table: {e}")))?;
    if !snap.missing.is_empty() {
        print(&format!(
            "Warning: models.dev lists no {}; kept their previous lines.",
            snap.missing.join(", ")
        ));
    }
    Ok(snap.text)
}

/// Write the model maker snapshot to `path` unless it already holds it.
fn write_providers(path: &Path, text: &str, print: &mut dyn FnMut(&str)) -> Result<(), UserError> {
    if std::fs::read_to_string(path).is_ok_and(|old| old == text) {
        print(&format!(
            "Providers unchanged: {} left as is.",
            path.display()
        ));
        return Ok(());
    }
    write_atomic(path, text)
        .map_err(|e| UserError::Invalid(format!("Cannot write {}: {e}", path.display())))?;
    print(&format!("Wrote model makers to {}.", path.display()));
    Ok(())
}

/// Run `pricing update`; each line goes to `print`.
pub fn update(
    args: &PricingUpdateArgs,
    read_only: bool,
    print: &mut dyn FnMut(&str),
) -> Result<(), UserError> {
    let path = args.output.as_ref().map_or_else(local_path, PathBuf::from);
    if read_only {
        return Err(UserError::Invalid(format!(
            "Refusing to write {}: --read-only is set (no write performed).",
            path.display()
        )));
    }
    let api = if let Some(file) = &args.from {
        std::fs::read(file).map_err(|e| UserError::Invalid(format!("Cannot read {file}: {e}")))?
    } else {
        print(&format!("Fetching {MODELS_DEV_URL} ..."));
        download(MODELS_DEV_URL)
            .map_err(|e| UserError::Invalid(format!("Download failed ({MODELS_DEV_URL}): {e}")))?
    };
    let api = String::from_utf8_lossy(&api);
    let providers = match &args.providers {
        Some(out) => Some((
            Path::new(out),
            build_providers(Path::new(out), &api, print)?,
        )),
        None => None,
    };
    let table = from_models_dev(&api, &today())
        .map_err(|e| UserError::Invalid(format!("Not a usable models.dev table: {e}")))?;
    if let Some((out, text)) = &providers {
        write_providers(out, text, print)?;
    }
    if args.output.is_some() && same_models(&path, &table) {
        print(&format!("Prices unchanged: {} left as is.", path.display()));
        return Ok(());
    }
    write_atomic(&path, &table_json(&table))
        .map_err(|e| UserError::Invalid(format!("Cannot write {}: {e}", path.display())))?;
    print(&format!(
        "Wrote {} models (models.dev {}) to {}.",
        table.models.len(),
        table.fetched,
        path.display()
    ));
    if args.output.is_none() {
        print(match read_local_table(&path) {
            LocalTable::Newer(_) => {
                "It is used from the next start; running peektrace processes keep their prices."
            }
            _ => "It is not newer than the built-in table, so the built-in table stays in use.",
        });
    }
    Ok(())
}
