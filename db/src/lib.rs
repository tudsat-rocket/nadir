use std::sync::{Arc, Mutex};

use mavspec::rust::dialects::common::messages::CanFrame;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Failed to execute query: {0}")]
    Query(#[from] rusqlite::Error),
}

macro_rules! define_message_tables {
    ($conn:expr, $dialect:expr) => {
        for message in $dialect.messages() {
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
                        MavType::UInt8 => "INTEGER",
                        MavType::UInt16 => "INTEGER",
                        MavType::UInt32 => "INTEGER",
                        MavType::UInt64 => "INTEGER",
                        MavType::Int8 => "INTEGER",
                        MavType::Int16 => "INTEGER",
                        MavType::Int32 => "INTEGER",
                        MavType::Int64 => "INTEGER",
                        MavType::Char => "INTEGER",
                        MavType::UInt8MavlinkVersion => "INTEGER",
                        MavType::Float => "FLOAT",
                        MavType::Double => "REAL",
                        MavType::Array(_type, _len) => "BLOB",
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
        }
    };
}
impl Db {
    pub fn init() -> Self {
        let conn = rusqlite::Connection::open_in_memory().unwrap();

        let protocol = mavspec::definitions::protocol();
        define_message_tables!(conn, protocol.get_dialect_by_name("common").unwrap());

        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    pub fn conn<'a>(&'a self) -> std::sync::MutexGuard<'a, rusqlite::Connection> {
        self.conn.lock().unwrap()
    }

    // TODO: replace with macro-generated get-all methods

    // get can frames to display for system
    pub fn can_frames_for_system(
        &self,
        system_and_component_ids: (u8, u8),
    ) -> Result<Vec<CanFrame>, DbError> {
        let system_id = system_and_component_ids.0;
        let component_id = system_and_component_ids.0;

        let conn = self.conn();
        let mut stmt = conn.prepare(&"SELECT received_at, bus, id, len, data FROM messages_can_frame WHERE system_id=?1 AND component_id=?2")?;
        let rows = stmt.query_map(rusqlite::params![&system_id, &component_id,], |row| {
            Ok(CanFrame {
                target_system: 0xff,
                target_component: 0xff,
                bus: row.get(1)?,
                id: row.get(2)?,
                len: row.get(3)?,
                data: row.get(4)?,
            })
        })?;

        Ok(rows.filter_map(|frame| frame.ok()).collect())
    }
}

macros::generate_message_writers!();
macros::generate_message_readers!();
