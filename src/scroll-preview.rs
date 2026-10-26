//! Explicit manual scroll preview. Never used as a TV inventory fallback.
use crate::{i18n, model::Inventory};

const PREFIX: &str = "com.lounge.scroll-preview.";

pub fn is_placeholder(id: &str) -> bool {
    id.starts_with(PREFIX)
}

pub fn append(inventory: &mut Inventory, count: usize) {
    if count == 0 {
        return;
    }
    let templates = inventory.apps.clone();
    if templates.is_empty() {
        eprintln!("Lounge scroll preview: no real apps to use as artwork templates");
        return;
    }
    let mut added = 0;
    for index in 0..count {
        let id = format!("{PREFIX}{index:03}");
        if inventory.apps.iter().any(|app| app.id == id) {
            continue;
        }
        let mut app = templates[index % templates.len()].clone();
        app.id = id;
        app.title = format!(
            "{} {:03} · {}",
            i18n::t("scrollTestApp"),
            index + 1,
            app.title
        );
        inventory.apps.push(app);
        added += 1;
    }
    eprintln!(
        "Lounge scroll preview: {added} placeholder apps appended to {} real apps",
        templates.len()
    );
}
