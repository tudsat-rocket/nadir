use std::sync::{Arc, Mutex};

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
            println!("{:#?}", message);

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
            let query = format!(
                "CREATE TABLE messages_{} (received_at INTEGER, system_id INTEGER, component_id INTEGER, {})",
                message.name(),
                columns.join(", ")
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
}

macros::generate_message_writers!();
macros::generate_message_readers!();
