use sqlx::{Database, Execute, SqlStr};

/// Captures a query before `Execute::sql` consumes it, so tracing can borrow
/// its SQL without losing arguments, statement metadata, or cache preferences.
pub(crate) struct Query<DB: Database> {
    sql: SqlStr,
    statement: Option<DB::Statement>,
    arguments: Result<Option<DB::Arguments>, sqlx::error::BoxDynError>,
    persistent: bool,
}

impl<DB: Database> Query<DB> {
    pub(crate) fn new<'q>(mut query: impl Execute<'q, DB>) -> Self {
        let statement = query.statement().cloned();
        let arguments = query.take_arguments();
        let persistent = query.persistent();
        Self {
            sql: query.sql(),
            statement,
            arguments,
            persistent,
        }
    }

    pub(crate) fn sql(&self) -> &str {
        self.sql.as_str()
    }
}

impl<DB: Database> Execute<'_, DB> for Query<DB> {
    fn sql(self) -> SqlStr {
        self.sql
    }

    fn statement(&self) -> Option<&DB::Statement> {
        self.statement.as_ref()
    }

    fn take_arguments(&mut self) -> Result<Option<DB::Arguments>, sqlx::error::BoxDynError> {
        std::mem::replace(&mut self.arguments, Ok(None))
    }

    fn persistent(&self) -> bool {
        self.persistent
    }
}
