//! `forge remove`: deletes exactly what the lock records.

use forge_packs::model::Level;
use forge_render::changes::{owned_hash, read_lock};
use forge_render::plan::lock_path;
use forge_render::precommit::{block_hash, has_block, only_forge, remove_block};
use forge_render::settings::remove_entries;

use super::io::{Error, confirm, delete_path, prune_dirs, show_diff, write_settings, write_text};
use super::{Flags, base_of};

pub fn remove(flags: &Flags) -> Result<u8, Error> {
    let level = if flags.user {
        Level::User
    } else {
        Level::Project
    };
    let base = base_of(level)?;
    let lock_file = lock_path(level);
    let lock = read_lock(&base, level, lock_file)
        .map_err(|e| Error::new(format!("forge: {e}")))?
        .ok_or_else(|| Error::new(format!("forge: no {lock_file}; nothing to remove")))?;
    let read = |p: &str| std::fs::read_to_string(base.join(p)).ok();

    let mut settings: Vec<(String, String, Option<String>)> = Vec::new();
    let mut files_seen: Vec<&str> = Vec::new();
    for e in &lock.settings {
        if files_seen.contains(&e.file.as_str()) {
            continue;
        }
        files_seen.push(&e.file);
        let Some(before) = read(&e.file) else {
            continue;
        };
        let mine: Vec<_> = lock
            .settings
            .iter()
            .filter(|x| x.file == e.file)
            .cloned()
            .collect();
        let after = remove_entries(&before, &mine)
            .map_err(|err| Error::new(format!("forge: {}: {err}; fix it and re-run", e.file)))?;
        settings.push((e.file.clone(), before, after));
    }
    let files: Vec<&String> = lock
        .files
        .keys()
        .filter(|f| base.join(f).exists())
        .collect();
    let mut blocks = Vec::new();
    let mut stuck = Vec::new();
    for b in &lock.blocks {
        let Some(before) = read(&b.file).filter(|t| has_block(t)) else {
            continue;
        };
        match remove_block(b.kind, &before) {
            None => stuck.push(b.file.clone()),
            Some(after) => {
                let after = (!(b.created && only_forge(&after))).then_some(after);
                blocks.push((b.clone(), before, after));
            }
        }
    }
    for f in &stuck {
        eprintln!(
            "forge: {f}: forge's pre-commit block cannot be removed safely (its begin/end markers were changed, or entries were added inside it); remove the lines between \"# forge: begin\" and \"# forge: end\" yourself"
        );
    }
    if flags.dry_run {
        for f in &files {
            println!("--- {f} (deleted)");
        }
        println!("--- {lock_file} (deleted)");
        for (file, before, after) in &settings {
            show_diff(Some(before), after.as_deref(), file);
        }
        for (b, before, after) in &blocks {
            show_diff(Some(before), after.as_deref(), &b.file);
        }
        return Ok(0);
    }
    for (file, before, after) in &settings {
        if file.ends_with("settings.local.json") || Some(before) == after.as_ref() {
            continue;
        }
        show_diff(Some(before), after.as_deref(), file);
        if !confirm(&format!("apply these changes to {file}?"), flags.yes)? {
            return Err(Error::new("forge: cancelled; nothing removed"));
        }
    }
    for f in &files {
        if read(f).is_some_and(|t| owned_hash(&t) != lock.files[*f]) {
            eprintln!("forge: {f} was modified since the last apply; removing it anyway");
        }
        delete_path(&base, f)?;
    }
    for (file, _, after) in &settings {
        write_settings(&base, file, after.as_deref())?;
        if let Some(dir) = base.join(file).parent() {
            prune_dirs(dir, &base);
        }
    }
    for (b, before, after) in &blocks {
        if block_hash(before).as_deref() != Some(b.hash.as_str()) {
            eprintln!(
                "forge: {}: forge's pre-commit block was modified since the last apply; removing it anyway",
                b.file
            );
        }
        match after {
            None => delete_path(&base, &b.file)?,
            Some(t) => write_text(&base.join(&b.file), t, false)?,
        }
    }
    delete_path(&base, lock_file)?;
    let n = lock.settings.len();
    let from: Vec<&str> = blocks.iter().map(|(b, _, _)| b.file.as_str()).collect();
    println!(
        "forge: removed {} file(s) and {n} settings entr{}{}",
        files.len(),
        if n == 1 { "y" } else { "ies" },
        if from.is_empty() {
            String::new()
        } else {
            format!(", and the pre-commit block from {}", from.join(", "))
        }
    );
    Ok(u8::from(!stuck.is_empty()))
}
