//! The session a table data editor keeps while its tab is open (`SPEC-0190` behaviours 2 and 9):
//! one thread with its own runtime and connection, so pages, submits and a manual transaction all
//! run on the same connection.

use std::sync::mpsc;

use idecraft_database::{
    Applied, ConnectionProfile, EditSet, OnError, Outcome, Session, SystemStore, TableQuery,
    introspect, resolve_credentials,
};

/// Column names and rows of a result.
pub type Rows = (Vec<String>, Vec<Vec<Option<String>>>);

type Job = Box<dyn FnOnce(&tokio::runtime::Runtime, Result<&Session, &str>) + Send>;

/// A page of the table as loaded.
#[derive(Debug, Clone)]
pub struct Page {
    /// Column names.
    pub columns: Vec<String>,
    /// Rows; `None` is SQL NULL.
    pub rows: Vec<Vec<Option<String>>>,
    /// Rows matching the filter.
    pub total: u64,
    /// The page query that ran.
    pub sql: String,
    /// How long both queries took.
    pub time: std::time::Duration,
}

/// The worker of one data editor tab. Dropping it closes the connection; the database rolls an
/// open transaction back then.
pub struct TableSession {
    jobs: mpsc::Sender<Job>,
}

impl TableSession {
    /// Starts the worker and connects with `profile`'s stored credentials.
    pub fn open(profile: ConnectionProfile) -> Self {
        let (jobs, inbox) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name(format!("table-session-{}", profile.name))
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(_) => return,
                };
                let session = resolve_credentials(&profile, &SystemStore)
                    .map_err(|e| format!("{}: {e}", profile.name))
                    .and_then(|c| {
                        rt.block_on(Session::connect(&profile, &c))
                            .map_err(|e| format!("{}: {e}", profile.name))
                    });
                for job in inbox {
                    job(&rt, session.as_ref().map_err(String::as_str));
                }
            })
            .ok();
        Self { jobs }
    }

    /// Runs `work` on the worker; the receiver gets its result.
    pub fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce(&tokio::runtime::Runtime, &Session) -> Result<T, String> + Send + 'static,
    ) -> mpsc::Receiver<Result<T, String>> {
        let (tx, rx) = mpsc::channel();
        let job: Job = Box::new(move |rt, session| {
            let _ = tx.send(session.map_err(str::to_owned).and_then(|s| work(rt, s)));
        });
        if self.jobs.send(job).is_err() {
            // The worker is gone; the receiver reports a closed channel.
        }
        rx
    }
}

fn rows(rt: &tokio::runtime::Runtime, session: &Session, sql: &str) -> Result<Rows, String> {
    let run = rt.block_on(session.run_script(sql, OnError::Stop));
    match run.outcomes.into_iter().next().map(|o| o.outcome) {
        Some(Outcome::ResultSet { columns, rows }) => Ok((columns, rows)),
        Some(Outcome::Error(e)) => Err(e.to_string()),
        _ => Err("the query returned no rows".into()),
    }
}

/// Runs one statement that returns no rows.
pub fn execute(rt: &tokio::runtime::Runtime, session: &Session, sql: &str) -> Result<(), String> {
    let run = rt.block_on(session.run_script(sql, OnError::Stop));
    match run.outcomes.into_iter().next().map(|o| o.outcome) {
        Some(Outcome::Error(e)) => Err(e.to_string()),
        _ => Ok(()),
    }
}

/// Reads table `name` of the session's schema (behaviour 2: fresh column names).
pub fn read_table(
    rt: &tokio::runtime::Runtime,
    session: &Session,
    name: &str,
) -> Result<idecraft_database::Table, String> {
    rt.block_on(introspect(session))
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|t| t.name == name)
        .ok_or_else(|| format!("`{name}` was not found"))
}

/// Loads page `page` of `query` and the count (behaviour 4).
pub fn load_page(
    rt: &tokio::runtime::Runtime,
    session: &Session,
    query: &TableQuery,
    page: usize,
    size: usize,
) -> Result<Page, String> {
    let d = session.dialect();
    let started = std::time::Instant::now();
    let sql = query.page_sql(d, page, size).map_err(|e| e.to_string())?;
    let count = query.count_sql(d).map_err(|e| e.to_string())?;
    let (columns, page_rows) = rows(rt, session, &sql)?;
    let (_, counted) = rows(rt, session, &count)?;
    let total = counted
        .first()
        .and_then(|r| r.first().cloned().flatten())
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(page_rows.len() as u64);
    Ok(Page {
        columns,
        rows: page_rows,
        total,
        sql,
        time: started.elapsed(),
    })
}

/// All rows of `query` (for exports, SPEC-0021 v2 behaviour 11).
pub fn load_all(
    rt: &tokio::runtime::Runtime,
    session: &Session,
    query: &TableQuery,
) -> Result<Rows, String> {
    let sql = query
        .all_sql(session.dialect())
        .map_err(|e| e.to_string())?;
    rows(rt, session, &sql)
}

/// Applies `set`: committed at once (`savepoint` `None`, Auto), or inside the open transaction
/// behind that savepoint (Manual), starting the transaction when `begin` (behaviour 9).
pub fn apply(
    rt: &tokio::runtime::Runtime,
    session: &Session,
    set: &EditSet,
    savepoint: Option<(&str, bool)>,
    isolation: idecraft_database::Isolation,
) -> Result<Applied, String> {
    Ok(match savepoint {
        None => rt.block_on(set.apply(session)),
        Some((name, begin)) => {
            if begin {
                // SPEC-0190 v3 behaviour 13: the transaction opens at the chosen isolation.
                for sql in idecraft_database::begin_statements(session.dialect(), isolation)? {
                    execute(rt, session, &sql)?;
                }
            }
            rt.block_on(set.apply_in_transaction(session, name))
        }
    })
}
