//! Tauri commands: thin wrappers that lock the database and call into the modules.
//! Arguments arrive from the frontend in camelCase (e.g. `planId`).

use crate::error::{AppError, Result};
use crate::import::{self, CommitRequest, ImportSource, ImportSummary, Preview};
use crate::money::Cents;
use crate::overview::{self, Overview, WeekSpend};
use crate::plan::{self, Adjustment, Category, IncomeSource, IncomeSourceInput, Plan};
use crate::savings::{self, AccountBalance, SavingsAccount, Snapshot};
use crate::settings::{self, Settings};
use crate::transactions::{
    self, Filter, NewTransaction, Page, Tag, TransactionUpdate, TransactionView,
};
use chrono::NaiveDate;
use rusqlite::Connection;
use std::sync::{Mutex, MutexGuard};
use tauri::State;

pub struct AppState {
    pub db: Mutex<Connection>,
}

fn db<'a>(state: &'a State<AppState>) -> Result<MutexGuard<'a, Connection>> {
    state
        .db
        .lock()
        .map_err(|_| AppError::invalid("the database is unavailable; restart the app"))
}

fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

// ---------- overview & plans ----------

#[tauri::command]
pub fn get_overview(state: State<AppState>, year: i32) -> Result<Overview> {
    overview::overview(&*db(&state)?, year, today())
}

#[tauri::command]
pub fn get_today() -> NaiveDate {
    today()
}

#[tauri::command]
pub fn list_plans(state: State<AppState>) -> Result<Vec<Plan>> {
    plan::list_plans(&*db(&state)?)
}

#[tauri::command]
pub fn create_plan(state: State<AppState>, year: i32, copy_from: Option<i32>) -> Result<Plan> {
    plan::create_plan(&*db(&state)?, year, copy_from)
}

#[tauri::command]
pub fn set_plan_archived(state: State<AppState>, plan_id: i64, archived: bool) -> Result<Plan> {
    plan::set_archived(&*db(&state)?, plan_id, archived)
}

#[tauri::command]
pub fn list_adjustments(state: State<AppState>, plan_id: i64) -> Result<Vec<Adjustment>> {
    plan::adjustments(&*db(&state)?, plan_id)
}

#[tauri::command]
pub fn add_category(
    state: State<AppState>,
    plan_id: i64,
    name: String,
    limit_cents: Cents,
    lumpy: bool,
) -> Result<Category> {
    plan::add_category(&*db(&state)?, plan_id, &name, limit_cents, lumpy, today())
}

#[tauri::command]
pub fn update_category(
    state: State<AppState>,
    id: i64,
    name: String,
    lumpy: bool,
) -> Result<Category> {
    plan::update_category(&*db(&state)?, id, &name, lumpy)
}

#[tauri::command]
pub fn set_category_limit(
    state: State<AppState>,
    id: i64,
    limit_cents: Cents,
    reason: String,
) -> Result<Category> {
    plan::set_category_limit(&*db(&state)?, id, limit_cents, &reason, today())
}

#[tauri::command]
pub fn reallocate(
    state: State<AppState>,
    from_id: i64,
    to_id: i64,
    amount_cents: Cents,
    reason: String,
) -> Result<()> {
    plan::reallocate(
        &*db(&state)?,
        from_id,
        to_id,
        amount_cents,
        &reason,
        today(),
    )
}

#[tauri::command]
pub fn remove_category(
    state: State<AppState>,
    id: i64,
    merge_into: Option<i64>,
    move_limit: bool,
    reason: String,
) -> Result<i64> {
    plan::remove_category(&*db(&state)?, id, merge_into, move_limit, &reason, today())
}

#[tauri::command]
pub fn category_trend(state: State<AppState>, category_id: i64) -> Result<Vec<WeekSpend>> {
    overview::category_trend(&*db(&state)?, category_id, today())
}

#[tauri::command]
pub fn add_income_source(
    state: State<AppState>,
    plan_id: i64,
    input: IncomeSourceInput,
    projected_cents: Cents,
) -> Result<IncomeSource> {
    plan::add_income_source(&*db(&state)?, plan_id, &input, projected_cents, today())
}

#[tauri::command]
pub fn update_income_source(
    state: State<AppState>,
    id: i64,
    input: IncomeSourceInput,
) -> Result<IncomeSource> {
    plan::update_income_source(&*db(&state)?, id, &input)
}

#[tauri::command]
pub fn set_income_projection(
    state: State<AppState>,
    id: i64,
    projected_cents: Cents,
    confirm: bool,
    reason: String,
) -> Result<IncomeSource> {
    plan::set_income_projection(
        &*db(&state)?,
        id,
        projected_cents,
        confirm,
        &reason,
        today(),
    )
}

#[tauri::command]
pub fn remove_income_source(
    state: State<AppState>,
    id: i64,
    move_to: Option<i64>,
    reason: String,
) -> Result<()> {
    plan::remove_income_source(&*db(&state)?, id, move_to, &reason, today())
}

#[tauri::command]
pub fn set_savings_plan(
    state: State<AppState>,
    plan_id: i64,
    holdback_cents: Cents,
    draw_cents: Cents,
    reason: String,
) -> Result<Plan> {
    plan::set_savings_plan(
        &*db(&state)?,
        plan_id,
        holdback_cents,
        draw_cents,
        &reason,
        today(),
    )
}

// ---------- transactions ----------

#[tauri::command]
pub fn list_transactions(state: State<AppState>, filter: Filter) -> Result<Page> {
    transactions::list(&*db(&state)?, &filter)
}

#[tauri::command]
pub fn create_transaction(
    state: State<AppState>,
    transaction: NewTransaction,
) -> Result<TransactionView> {
    transactions::create(&*db(&state)?, &transaction)
}

#[tauri::command]
pub fn update_transaction(
    state: State<AppState>,
    id: i64,
    update: TransactionUpdate,
) -> Result<TransactionView> {
    transactions::update(&*db(&state)?, id, &update)
}

#[tauri::command]
pub fn set_tag_bulk(state: State<AppState>, ids: Vec<i64>, tag: Tag, learn: bool) -> Result<usize> {
    transactions::set_tag_bulk(&*db(&state)?, &ids, &tag, learn)
}

#[tauri::command]
pub fn delete_transactions(state: State<AppState>, ids: Vec<i64>) -> Result<usize> {
    transactions::delete(&*db(&state)?, &ids)
}

// ---------- savings ----------

#[tauri::command]
pub fn list_savings(state: State<AppState>) -> Result<Vec<AccountBalance>> {
    let conn = db(&state)?;
    savings::list_accounts(&conn)?
        .iter()
        .map(|a| savings::balance(&conn, a, today()))
        .collect()
}

#[tauri::command]
pub fn add_savings_account(
    state: State<AppState>,
    name: String,
    starting_balance_cents: Cents,
    starting_date: NaiveDate,
) -> Result<SavingsAccount> {
    savings::add_account(&*db(&state)?, &name, starting_balance_cents, starting_date)
}

#[tauri::command]
pub fn update_savings_account(
    state: State<AppState>,
    id: i64,
    name: String,
    starting_balance_cents: Cents,
    starting_date: NaiveDate,
    archived: bool,
) -> Result<SavingsAccount> {
    savings::update_account(
        &*db(&state)?,
        id,
        &name,
        starting_balance_cents,
        starting_date,
        archived,
    )
}

#[tauri::command]
pub fn add_snapshot(
    state: State<AppState>,
    account_id: i64,
    date: NaiveDate,
    balance_cents: Cents,
    note: String,
) -> Result<Snapshot> {
    savings::add_snapshot(&*db(&state)?, account_id, date, balance_cents, &note)
}

#[tauri::command]
pub fn list_snapshots(state: State<AppState>, account_id: i64) -> Result<Vec<Snapshot>> {
    savings::snapshots(&*db(&state)?, account_id)
}

#[tauri::command]
pub fn delete_snapshot(state: State<AppState>, id: i64) -> Result<()> {
    savings::delete_snapshot(&*db(&state)?, id)
}

// ---------- import ----------

#[tauri::command]
pub fn list_import_sources(state: State<AppState>) -> Result<Vec<ImportSource>> {
    import::list_sources(&*db(&state)?)
}

#[tauri::command]
pub fn add_import_source(
    state: State<AppState>,
    name: String,
    kind: String,
) -> Result<ImportSource> {
    import::add_source(&*db(&state)?, &name, &kind)
}

#[tauri::command]
pub fn update_import_source(
    state: State<AppState>,
    id: i64,
    name: String,
    kind: String,
) -> Result<ImportSource> {
    import::update_source(&*db(&state)?, id, &name, &kind)
}

#[tauri::command]
pub fn import_preview(
    state: State<AppState>,
    source_id: i64,
    text: String,
    purchases_positive: Option<bool>,
) -> Result<Preview> {
    import::preview(&*db(&state)?, source_id, &text, purchases_positive, today())
}

#[tauri::command]
pub fn import_commit(state: State<AppState>, request: CommitRequest) -> Result<ImportSummary> {
    import::commit(&*db(&state)?, &request, today())
}

#[tauri::command]
pub fn undo_import(state: State<AppState>, batch_id: i64) -> Result<()> {
    import::undo(&*db(&state)?, batch_id)
}

#[tauri::command]
pub fn list_import_batches(state: State<AppState>) -> Result<Vec<import::Batch>> {
    import::list_batches(&*db(&state)?, 50)
}

// ---------- settings ----------

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<Settings> {
    settings::load(&*db(&state)?)
}

#[tauri::command]
pub fn save_settings(state: State<AppState>, settings: Settings) -> Result<Settings> {
    settings::save(&*db(&state)?, &settings)
}
