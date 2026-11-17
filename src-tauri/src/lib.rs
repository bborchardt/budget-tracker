//! Budget Tracker backend. See requirements.md for the concepts.

pub mod calendar;
pub mod commands;
pub mod db;
pub mod error;
pub mod import;
pub mod money;
pub mod overview;
pub mod pace;
pub mod plan;
pub mod savings;
pub mod settings;
pub mod transactions;

use std::sync::Mutex;
use tauri::Manager;

/// Data lives in `~/Library/Application Support/budget-tracker` on macOS.
const DATA_DIR_NAME: &str = "budget-tracker";
const DB_FILE: &str = "budget.db";
const DATA_DIR_ENV: &str = "BUDGET_TRACKER_DATA_DIR";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // BUDGET_TRACKER_DATA_DIR points development and testing at a throwaway database
            let dir = match std::env::var_os(DATA_DIR_ENV) {
                Some(d) if !d.is_empty() => std::path::PathBuf::from(d),
                _ => app.path().data_dir()?.join(DATA_DIR_NAME),
            };
            let conn = db::open(&dir.join(DB_FILE))?;
            app.manage(commands::AppState {
                db: Mutex::new(conn),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_overview,
            commands::get_today,
            commands::list_plans,
            commands::create_plan,
            commands::set_plan_archived,
            commands::list_adjustments,
            commands::add_category,
            commands::update_category,
            commands::set_category_limit,
            commands::reallocate,
            commands::remove_category,
            commands::category_trend,
            commands::add_income_source,
            commands::update_income_source,
            commands::set_income_projection,
            commands::remove_income_source,
            commands::set_savings_plan,
            commands::list_transactions,
            commands::create_transaction,
            commands::update_transaction,
            commands::set_tag_bulk,
            commands::delete_transactions,
            commands::list_savings,
            commands::add_savings_account,
            commands::update_savings_account,
            commands::add_snapshot,
            commands::list_snapshots,
            commands::delete_snapshot,
            commands::list_import_sources,
            commands::add_import_source,
            commands::update_import_source,
            commands::import_preview,
            commands::import_commit,
            commands::undo_import,
            commands::list_import_batches,
            commands::get_settings,
            commands::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Budget Tracker");
}
