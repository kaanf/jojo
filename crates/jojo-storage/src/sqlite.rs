//! Compatibility facade over rusqlite and the system SQLite library.
use rusqlite::{
    OpenFlags,
    types::{ToSql, ToSqlOutput, ValueRef},
};
use std::{fmt, path::Path, time::Duration};
const SQLITE_MISUSE: i32 = 21;
const SQLITE_BUSY: i32 = 5;
const SQLITE_CONSTRAINT: i32 = 19;
const MINIMUM_SQLITE_VERSION: i32 = 3_037_000;
/// An error reported by SQLite (extended result code) or by this wrapper (`SQLITE_MISUSE`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqliteError {
    pub code: i32,
    pub message: String,
}

impl SqliteError {
    pub(crate) fn misuse(message: impl Into<String>) -> Self {
        Self {
            code: SQLITE_MISUSE,
            message: message.into(),
        }
    }

    pub fn is_busy(&self) -> bool {
        self.code & 0xff == SQLITE_BUSY
    }

    pub fn is_constraint(&self) -> bool {
        self.code & 0xff == SQLITE_CONSTRAINT
    }
}

impl fmt::Display for SqliteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sqlite error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for SqliteError {}

/// A borrowed statement parameter.
#[derive(Clone, Copy, Debug)]
pub enum Value<'a> {
    Null,
    Int(i64),
    Text(&'a str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenMode {
    /// Open an existing database read-write (required for WAL readers); never creates it.
    Existing,
    /// Open read-write, creating the file if it does not exist.
    Create,
}

impl From<rusqlite::Error> for SqliteError {
    fn from(error: rusqlite::Error) -> Self {
        match error {
            rusqlite::Error::SqliteFailure(code, message) => Self {
                code: code.extended_code,
                message: message.unwrap_or_else(|| code.to_string()),
            },
            other => Self::misuse(other.to_string()),
        }
    }
}
impl ToSql for Value<'_> {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(match self {
            Self::Null => ValueRef::Null,
            Self::Int(value) => ValueRef::Integer(*value),
            Self::Text(value) => ValueRef::Text(value.as_bytes()),
        }))
    }
}
pub fn library_version_number() -> i32 {
    rusqlite::version_number()
}
pub struct Connection {
    raw: rusqlite::Connection,
}
impl Connection {
    pub fn open(path: &Path, mode: OpenMode) -> Result<Self, SqliteError> {
        let version = library_version_number();
        if version < MINIMUM_SQLITE_VERSION {
            return Err(SqliteError::misuse(format!(
                "system SQLite {version} is older than required {MINIMUM_SQLITE_VERSION}"
            )));
        }
        let flags = OpenFlags::SQLITE_OPEN_FULL_MUTEX
            | OpenFlags::SQLITE_OPEN_READ_WRITE
            | match mode {
                OpenMode::Existing => OpenFlags::empty(),
                OpenMode::Create => OpenFlags::SQLITE_OPEN_CREATE,
            };
        let raw = rusqlite::Connection::open_with_flags(path, flags)?;
        // rusqlite defaults to five seconds; the original facade defaults to no wait.
        raw.busy_timeout(Duration::ZERO)?;
        Ok(Self { raw })
    }
    pub fn set_busy_timeout(&self, milliseconds: u32) -> Result<(), SqliteError> {
        i32::try_from(milliseconds)
            .map_err(|_| SqliteError::misuse("busy timeout out of range"))?;
        self.raw
            .busy_timeout(Duration::from_millis(u64::from(milliseconds)))
            .map_err(Into::into)
    }
    pub fn execute_batch(&self, sql: &str) -> Result<(), SqliteError> {
        check_sql(sql)?;
        use rusqlite::fallible_iterator::FallibleIterator;
        let mut batch = rusqlite::Batch::new(&self.raw, sql);
        while let Some(mut statement) = batch.next()? {
            let mut rows = statement.raw_query();
            while rows.next()?.is_some() {}
        }
        Ok(())
    }
    pub fn execute(&self, sql: &str, params: &[Value<'_>]) -> Result<u64, SqliteError> {
        self.run(sql, params, |_| Ok(()))?;
        Ok(self.raw.changes())
    }
    pub fn query<T>(
        &self,
        sql: &str,
        params: &[Value<'_>],
        map: impl FnMut(&Row<'_>) -> Result<T, SqliteError>,
    ) -> Result<Vec<T>, SqliteError> {
        self.run(sql, params, map)
    }
    pub fn query_optional<T>(
        &self,
        sql: &str,
        params: &[Value<'_>],
        map: impl FnMut(&Row<'_>) -> Result<T, SqliteError>,
    ) -> Result<Option<T>, SqliteError> {
        let mut rows = self.run(sql, params, map)?;
        if rows.len() > 1 {
            return Err(SqliteError::misuse("query returned more than one row"));
        }
        Ok(rows.pop())
    }
    pub fn query_one<T>(
        &self,
        sql: &str,
        params: &[Value<'_>],
        map: impl FnMut(&Row<'_>) -> Result<T, SqliteError>,
    ) -> Result<T, SqliteError> {
        self.query_optional(sql, params, map)?
            .ok_or_else(|| SqliteError::misuse("query returned no row"))
    }
    pub fn in_transaction(&self) -> bool {
        !self.raw.is_autocommit()
    }
    fn run<T>(
        &self,
        sql: &str,
        params: &[Value<'_>],
        mut map: impl FnMut(&Row<'_>) -> Result<T, SqliteError>,
    ) -> Result<Vec<T>, SqliteError> {
        check_sql(sql)?;
        let mut statement = self
            .raw
            .prepare(sql.trim_end_matches(|c: char| c.is_ascii_whitespace()))?;
        let prepared = statement
            .expanded_sql()
            .ok_or_else(|| SqliteError::misuse("SQL contains no statement"))?;
        if prepared.is_empty() {
            return Err(SqliteError::misuse("SQL contains no statement"));
        }
        // rusqlite rejects additional statements but accepts comments after a final
        // semicolon. Preserve the original stricter tail rule. Ask SQLite itself
        // which semicolon completes the statement (including triggers/quoted text).
        for (index, _) in sql.match_indices(';') {
            if self
                .raw
                .prepare(&sql[..=index])
                .ok()
                .and_then(|s| s.expanded_sql())
                .as_ref()
                == Some(&prepared)
            {
                if !sql[index + 1..].bytes().all(|b| b.is_ascii_whitespace()) {
                    return Err(SqliteError::misuse("trailing content after statement"));
                }
                break;
            }
        }
        if statement.parameter_count() != params.len() {
            return Err(SqliteError::misuse("incorrect parameter count"));
        }
        let mut cursor = statement.query(rusqlite::params_from_iter(params))?;
        let mut rows = Vec::new();
        while let Some(raw) = cursor.next()? {
            rows.push(map(&Row { raw })?);
        }
        Ok(rows)
    }
}
fn check_sql(sql: &str) -> Result<(), SqliteError> {
    if sql.contains('\0') {
        Err(SqliteError::misuse("SQL contains NUL"))
    } else {
        Ok(())
    }
}
pub struct Row<'s> {
    raw: &'s rusqlite::Row<'s>,
}
impl Row<'_> {
    pub fn int(&self, column: usize) -> Result<i64, SqliteError> {
        self.optional_int(column)?
            .ok_or_else(|| SqliteError::misuse(format!("column {column} is NULL")))
    }
    pub fn optional_int(&self, column: usize) -> Result<Option<i64>, SqliteError> {
        match self.raw.get_ref(column)? {
            ValueRef::Null => Ok(None),
            ValueRef::Integer(value) => Ok(Some(value)),
            _ => Err(SqliteError::misuse(format!(
                "column {column} is not an integer"
            ))),
        }
    }
    pub fn text(&self, column: usize) -> Result<String, SqliteError> {
        self.optional_text(column)?
            .ok_or_else(|| SqliteError::misuse(format!("column {column} is NULL")))
    }
    pub fn optional_text(&self, column: usize) -> Result<Option<String>, SqliteError> {
        match self.raw.get_ref(column)? {
            ValueRef::Null => Ok(None),
            ValueRef::Text(value) => String::from_utf8(value.to_vec())
                .map(Some)
                .map_err(|_| SqliteError::misuse(format!("column {column} is not UTF-8"))),
            _ => Err(SqliteError::misuse(format!("column {column} is not text"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Connection {
        Connection::open(Path::new(":memory:"), OpenMode::Create).unwrap()
    }

    #[test]
    fn rusqlite_adapter_preserves_statement_and_value_boundaries() {
        let db = memory();
        for sql in [
            "-- comment only",
            "/* comment */",
            ";",
            "SELECT 1; -- tail",
            "SELECT 1; /* tail */",
            "SELECT 1;;",
        ] {
            assert!(db.execute(sql, &[]).is_err(), "{sql}");
        }
        assert_eq!(
            db.query_one("SELECT ';', ?1; ", &[Value::Text("a\0b")], |r| r.text(1))
                .unwrap(),
            "a\0b"
        );
        assert!(
            db.query_one("SELECT CAST(x'ff' AS TEXT)", &[], |r| r.text(0))
                .is_err()
        );
        assert!(db.query_one("SELECT 1.5", &[], |r| r.int(0)).is_err());
        assert_eq!(
            db.query_one("SELECT 7;\u{c}", &[], |r| r.int(0)).unwrap(),
            7
        );
        // sqlite3_exec drained every row, including errors after the first row.
        assert!(db.execute_batch("SELECT CASE x WHEN 2 THEN abs(-9223372036854775808) ELSE x END FROM (SELECT 1 AS x UNION ALL SELECT 2)").is_err());
        assert!(db.set_busy_timeout(u32::MAX).is_err());
        assert_eq!(
            db.query_one("PRAGMA busy_timeout", &[], |r| r.int(0))
                .unwrap(),
            0
        );
        db.execute_batch("CREATE TABLE t (a INTEGER)").unwrap();
        db.execute(
            "CREATE TRIGGER tr AFTER INSERT ON t BEGIN SELECT 1; SELECT 2; END;",
            &[],
        )
        .unwrap();
    }

    #[test]
    fn round_trips_integers_text_and_null() {
        let db = memory();
        db.execute_batch("CREATE TABLE t (a INTEGER, b TEXT, c TEXT) STRICT")
            .unwrap();
        let changed = db
            .execute(
                "INSERT INTO t VALUES (?1, ?2, ?3)",
                &[Value::Int(i64::MIN), Value::Text("héllo ✓"), Value::Null],
            )
            .unwrap();
        assert_eq!(changed, 1);
        db.execute(
            "INSERT INTO t VALUES (?1, ?2, ?3)",
            &[Value::Int(7), Value::Text(""), Value::Text("x")],
        )
        .unwrap();
        let rows = db
            .query("SELECT a, b, c FROM t ORDER BY rowid", &[], |row| {
                Ok((row.int(0)?, row.text(1)?, row.optional_text(2)?))
            })
            .unwrap();
        assert_eq!(
            rows,
            vec![
                (i64::MIN, "héllo ✓".to_string(), None),
                (7, String::new(), Some("x".to_string())),
            ]
        );
    }

    #[test]
    fn misuse_is_reported_instead_of_being_ignored() {
        let db = memory();
        db.execute_batch("CREATE TABLE t (a INTEGER) STRICT")
            .unwrap();
        // Trailing statements must not be silently dropped.
        assert!(db.execute("SELECT 1; DELETE FROM t", &[]).is_err());
        // Parameter arity is checked rather than defaulting missing values to NULL.
        assert!(db.execute("INSERT INTO t VALUES (?1)", &[]).is_err());
        assert!(db.execute("", &[]).is_err());
        assert!(db.execute("SELECT 1\0", &[]).is_err());
        // Type confusion and missing columns are errors.
        assert!(db.query_one("SELECT 'x'", &[], |r| r.int(0)).is_err());
        assert!(db.query_one("SELECT 1", &[], |r| r.text(0)).is_err());
        assert!(db.query_one("SELECT 1", &[], |r| r.int(1)).is_err());
        assert!(db.query_one("SELECT NULL", &[], |r| r.int(0)).is_err());
        assert!(
            db.query_optional("SELECT 1 UNION SELECT 2", &[], |r| r.int(0))
                .is_err()
        );
    }

    #[test]
    fn constraint_errors_carry_extended_codes_and_transactions_are_visible() {
        let db = memory();
        db.execute_batch("CREATE TABLE t (a INTEGER PRIMARY KEY) STRICT; INSERT INTO t VALUES (1)")
            .unwrap();
        let error = db.execute("INSERT INTO t VALUES (1)", &[]).unwrap_err();
        assert!(error.is_constraint() && !error.is_busy());
        assert!(error.code > 0xff, "extended result code expected");
        assert!(!db.in_transaction());
        db.execute_batch("BEGIN IMMEDIATE").unwrap();
        assert!(db.in_transaction());
        db.execute_batch("ROLLBACK").unwrap();
        assert!(!db.in_transaction());
    }

    #[test]
    fn opening_a_missing_database_without_create_fails() {
        let missing = std::env::temp_dir().join("jojo-sqlite-missing-dir/none.sqlite3");
        assert!(Connection::open(&missing, OpenMode::Existing).is_err());
    }
}
