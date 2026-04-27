use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use mavinspect::protocol::MavType;
use maviola::protocol::MessageSpec;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Failed to execute query: {0}")]
    Query(#[from] rusqlite::Error),
}

pub trait MessageExt: MessageSpec {
    fn table(&self) -> &str;
    fn rows(&self) -> &[&str];

    fn insert(
        &self,
        conn: &rusqlite::Connection,
        system_id: u8,
        component_id: u8,
    ) -> Result<(), rusqlite::Error>;

    fn from_row(row: &rusqlite::Row<'_>) -> Result<Self, rusqlite::Error>
    where
        Self: Sized;
}

macro_rules! define_message_tables {
    ($conn:expr, $dialect:expr) => {
        for message in $dialect.messages() {
            use maviola::prelude::Dialect;

            // Skip messages from common in other dialects derived from it
            if $dialect.name() != "common" && mavspec::rust::dialects::Common::message_info(message.id()).is_ok() {
                continue;
            }

            let columns: Vec<String> = message
                .fields()
                .iter()
                .map(|f| {
                    use mavinspect::protocol::MavType;

                    let colname = match f.name() {
                        "index" => "index_",
                        "type" => "type_",
                        n => n,
                    };

                    let coltype = match f.r#type() {
                        MavType::Array(_type, _len) => "BLOB",
                        // Since SQLite doesn't properly handle NaN, we store even floats
                        // as integers.
                        _ => "INTEGER",
                    };

                    format!("{colname} {coltype}")
                })
                .collect::<Vec<_>>();

            let query = if message.name() == "COMMAND_INT" || message.name() == "COMMAND_LONG" {
                format!(
                    "CREATE TABLE messages_{} (received_at INTEGER, system_id INTEGER, component_id INTEGER, acked_at INTEGER, result INTEGER, {})",
                    message.name(),
                    columns.join(", ")
                )
            } else {
                format!(
                    "CREATE TABLE messages_{} (received_at INTEGER, system_id INTEGER, component_id INTEGER, {})",
                    message.name(),
                    columns.join(", ")
                )
            };
            $conn.execute(&query, rusqlite::params![]).unwrap();

            let query = format!(
                "CREATE INDEX index_{}_system ON messages_{} (system_id, component_id)",
                message.name(),
                message.name(),
            );
            $conn.execute(&query, rusqlite::params![]).unwrap();

            let query = format!(
                "CREATE INDEX index_{}_received_at ON messages_{} (received_at)",
                message.name(),
                message.name(),
            );
            $conn.execute(&query, rusqlite::params![]).unwrap();

            let query = format!(
                "CREATE INDEX index_{} ON messages_{} (system_id, component_id, received_at)",
                message.name(),
                message.name(),
            );
            $conn.execute(&query, rusqlite::params![]).unwrap();
        }
    };
}

impl Db {
    pub fn init() -> Self {
        let conn = rusqlite::Connection::open_in_memory().unwrap();

        let protocol = mavspec::definitions::protocol();
        define_message_tables!(conn, protocol.get_dialect_by_name("common").unwrap());
        define_message_tables!(conn, protocol.get_dialect_by_name("ardupilotmega").unwrap());
        define_message_tables!(
            conn,
            rapid_dialect::definitions::protocol()
                .get_dialect_by_canonical_name("rapid")
                .unwrap()
        );

        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    pub fn conn(&self) -> std::sync::MutexGuard<'_, rusqlite::Connection> {
        self.conn.lock().unwrap()
    }

    pub fn write_message<M: MessageExt>(
        &self,
        system_id: u8,
        component_id: u8,
        msg: &M,
    ) -> Result<(), DbError> {
        let conn = self.conn();
        conn.busy_timeout(std::time::Duration::from_millis(10))?;
        msg.insert(&conn, system_id, component_id)?;
        Ok(())
    }

    pub fn last_message<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
    ) -> Result<M, DbError> {
        puffin::profile_function!();

        let conn = self.conn();
        conn.busy_timeout(std::time::Duration::from_millis(10))?;

        let query = format!(
            "SELECT {} FROM {}
                WHERE system_id=:system_id AND component_id=:component_id
                ORDER BY received_at DESC
                LIMIT 1",
            M::default().rows().join(","),
            M::default().table(),
        );

        let mut stmt = conn.prepare(&query)?;
        let msg = stmt.query_one(
            &[(":system_id", &system_id), (":component_id", &component_id)],
            |row| M::from_row(row),
        )?;

        Ok(msg)
    }

    pub fn all_messages<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
    ) -> Result<Vec<(DateTime<Utc>, M)>, DbError> {
        puffin::profile_function!();

        let conn = self.conn();
        conn.busy_timeout(std::time::Duration::from_millis(10))?;

        let query = format!(
            "SELECT {}, received_at FROM {}
                WHERE system_id=:system_id AND component_id=:component_id
                ORDER BY received_at ASC",
            M::default().rows().join(","),
            M::default().table(),
        );

        let mut stmt = conn.prepare(&query)?;
        let rows = stmt
            .query_map(
                &[(":system_id", &system_id), (":component_id", &component_id)],
                |row| {
                    let m = M::from_row(row)?;
                    let t: DateTime<Utc> = row.get(M::default().rows().len())?;
                    Ok((t, m))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    pub fn count_message<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
    ) -> Result<usize, DbError> {
        let conn = self.conn();
        conn.busy_timeout(std::time::Duration::from_millis(10))?;

        let mut stmt = conn.prepare(&format!(
            "SELECT COUNT(*) FROM {}
                WHERE system_id=:system_id AND component_id=:component_id",
            M::default().table()
        ))?;

        let count = stmt.query_one(
            &[(":system_id", &system_id), (":component_id", &component_id)],
            |row| row.get(0),
        )?;

        Ok(count)
    }

    pub fn count_message_by_name(
        &self,
        message_name: &str,
        system_id: u8,
        component_id: u8,
    ) -> Result<usize, DbError> {
        let conn = self.conn();
        conn.busy_timeout(std::time::Duration::from_millis(10))?;

        let table_name = format!("messages_{}", message_name.to_lowercase());
        let mut stmt = conn.prepare(&format!(
            "SELECT COUNT(*) FROM {table_name}
                WHERE system_id=:system_id AND component_id=:component_id",
        ))?;

        let count = stmt.query_one(
            &[(":system_id", &system_id), (":component_id", &component_id)],
            |row| row.get(0),
        )?;

        Ok(count)
    }

    pub fn timeseries_by_name(
        &self,
        msg_name: &str,
        field_name: &str,
        system_id: u8,
        component_id: u8,
        since: Option<chrono::DateTime<chrono::Utc>>,
        _until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Vec<(chrono::DateTime<chrono::Utc>, f64)>, DbError> {
        puffin::profile_function!(msg_name.to_owned() + "." + field_name);

        let conn = self.conn();
        let lower_case = msg_name.to_lowercase();

        let since = since.unwrap_or_default();

        let protocol = mavspec::definitions::protocol();
        let Some(field) = ["common", "ardupilotmega"]
            .iter()
            .filter_map(|dn| {
                let dialect = protocol.get_dialect_by_name(dn).unwrap();
                dialect
                    .get_message_by_name(&msg_name)
                    .and_then(|msg| msg.get_field_by_name(field_name))
            })
            .next()
            .or_else(|| {
                rapid_dialect::definitions::protocol()
                    .get_dialect_by_canonical_name("rapid")
                    .unwrap()
                    .get_message_by_name(&msg_name)
                    .and_then(|msg| msg.get_field_by_name(field_name))
            })
        else {
            unreachable!();
        };

        // Look Ma, SQL injection
        let query = format!(
            "SELECT received_at, {field_name} FROM messages_{lower_case}
                    WHERE system_id=?1 AND component_id=?2 AND received_at >= ?3
                    ORDER BY received_at ASC"
        );

        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(
            rusqlite::params![&system_id, &component_id, &since],
            |row| {
                let timestamp: chrono::DateTime<chrono::Utc> = row.get(0)?;
                let int: u64 = row.get(1)?;
                let value = match field.r#type() {
                    MavType::Float => f32::from_bits(int as u32) as f64,
                    MavType::Double => f64::from_bits(int),
                    _ => int as f64,
                };
                Ok((timestamp, value))
            },
        )?;

        let timeseries = rows.collect::<Result<Vec<(chrono::DateTime<chrono::Utc>, f64)>, _>>()?;

        Ok(timeseries)
    }
}

macros::implement_message_ext_for_dialect!(
    "common",
    mavspec::rust::dialects::Common,
    mavspec::rust::dialects::common
);
macros::implement_message_ext_for_dialect!(
    "ardupilotmega",
    mavspec::rust::dialects::Ardupilotmega,
    mavspec::rust::dialects::ardupilotmega
);
macros::implement_message_ext_for_dialect!("rapid", rapid_dialect::Rapid, rapid_dialect::rapid);
