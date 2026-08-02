use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use mavinspect::protocol::MavType;
use mavspec::rust::spec::MessageSpec;

const FREQ_WINDOW_SECS: i64 = 5;

#[derive(Default)]
struct RowStats {
    count: u64,
    last: Option<DateTime<Utc>>,
    recent: VecDeque<DateTime<Utc>>,
}

/// (`system_id`, `component_id`, `message_id`, `instance_value`)
type StatsKey = (u8, u8, u32, Option<i64>);
type Stats = Arc<Mutex<HashMap<StatsKey, RowStats>>>;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<rusqlite::Connection>>,
    instance_fields: Arc<HashMap<u32, String>>,
    stats: Stats,
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Failed to execute query: {0}")]
    Query(#[from] rusqlite::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageInstance {
    pub field: String,
    pub value: i64,
}

#[derive(Debug, Clone)]
pub struct MessageSummary {
    pub msg_id: u32,
    pub name: String,
    pub instance: Option<MessageInstance>,
    pub count: usize,
    pub freq_hz: f32,
    pub last: DateTime<Utc>,
}

fn datetime_from_micros(col: usize, micros: i64) -> Result<DateTime<Utc>, rusqlite::Error> {
    DateTime::from_timestamp_micros(micros)
        .ok_or(rusqlite::Error::IntegralValueOutOfRange(col, micros))
}

pub trait MessageExt: MessageSpec {
    fn table(&self) -> &str;
    fn rows(&self) -> &[&str];

    /// Numeric value of the message's `instance="true"` field, if any.
    fn instance_value(&self) -> Option<i64> {
        None
    }

    /// SQL column name of the message's `instance="true"` field, if any.
    fn instance_field() -> Option<&'static str> {
        None
    }

    fn insert(
        &self,
        conn: &rusqlite::Connection,
        system_id: u8,
        component_id: u8,
        received_at: DateTime<Utc>,
    ) -> Result<(), rusqlite::Error>;

    fn from_row(row: &rusqlite::Row<'_>) -> Result<Self, rusqlite::Error>
    where
        Self: Sized;
}

macro_rules! define_message_tables {
    ($conn:expr, $dialect:expr) => {
        for message in $dialect.messages() {
            use mavspec::rust::spec::Dialect;

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

        conn.set_prepared_statement_cache_capacity(256);

        let protocol = mavspec::definitions::protocol();
        define_message_tables!(conn, protocol.get_dialect_by_name("common").unwrap());
        define_message_tables!(conn, protocol.get_dialect_by_name("ardupilotmega").unwrap());
        define_message_tables!(
            conn,
            rapid_dialect::definitions::protocol()
                .get_dialect_by_canonical_name("rapid")
                .unwrap()
        );

        let instance_fields: HashMap<u32, String> = [
            protocol.get_dialect_by_name("common").unwrap(),
            protocol.get_dialect_by_name("ardupilotmega").unwrap(),
            rapid_dialect::definitions::protocol()
                .get_dialect_by_canonical_name("rapid")
                .unwrap(),
        ]
        .into_iter()
        .flat_map(collect_instance_fields)
        .collect();

        Self {
            conn: Arc::new(Mutex::new(conn)),
            instance_fields: Arc::new(instance_fields),
            stats: Arc::new(Mutex::new(HashMap::new())),
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
        self.write_message_at(system_id, component_id, msg, Utc::now())
    }

    #[allow(clippy::unwrap_in_result)]
    pub fn write_message_at<M: MessageExt>(
        &self,
        system_id: u8,
        component_id: u8,
        msg: &M,
        received_at: DateTime<Utc>,
    ) -> Result<(), DbError> {
        let conn = self.conn();
        conn.busy_timeout(std::time::Duration::from_millis(10))?;
        msg.insert(&conn, system_id, component_id, received_at)?;
        drop(conn);

        let now = received_at;
        let cutoff = now - chrono::TimeDelta::seconds(FREQ_WINDOW_SECS);
        let key = (system_id, component_id, msg.id(), msg.instance_value());
        let mut stats = self.stats.lock().unwrap();
        let entry = stats.entry(key).or_default();
        entry.count += 1;
        entry.last = Some(now);
        entry.recent.push_back(now);
        while entry.recent.front().is_some_and(|t| *t < cutoff) {
            entry.recent.pop_front();
        }
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

        let mut stmt = conn.prepare_cached(&query)?;
        let msg = stmt.query_one(
            &[(":system_id", &system_id), (":component_id", &component_id)],
            |row| M::from_row(row),
        )?;

        Ok(msg)
    }

    pub fn last_message_filtered<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
        instance: Option<(&str, i64)>,
    ) -> Result<M, DbError> {
        puffin::profile_function!();

        let conn = self.conn();
        conn.busy_timeout(std::time::Duration::from_millis(10))?;

        let extra_where = match instance {
            Some((col, _)) => format!(" AND {col} = :instance_value"),
            None => String::new(),
        };

        let query = format!(
            "SELECT {} FROM {}
                WHERE system_id=:system_id AND component_id=:component_id{}
                ORDER BY received_at DESC
                LIMIT 1",
            M::default().rows().join(","),
            M::default().table(),
            extra_where,
        );

        let mut stmt = conn.prepare_cached(&query)?;
        let msg = match instance {
            Some((_, value)) => stmt.query_one(
                rusqlite::named_params! {
                    ":system_id": system_id,
                    ":component_id": component_id,
                    ":instance_value": value,
                },
                |row| M::from_row(row),
            )?,
            None => stmt.query_one(
                rusqlite::named_params! {
                    ":system_id": system_id,
                    ":component_id": component_id,
                },
                |row| M::from_row(row),
            )?,
        };

        Ok(msg)
    }

    pub fn all_messages<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
    ) -> Result<Vec<(DateTime<Utc>, M)>, DbError> {
        self.messages_since(system_id, component_id, None, None)
    }

    /// The oldest `limit` messages newer than `since`, or all of them if no limit is given.
    pub fn messages_since<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
        since: Option<DateTime<Utc>>,
        limit: Option<usize>,
    ) -> Result<Vec<(DateTime<Utc>, M)>, DbError> {
        puffin::profile_function!();

        let conn = self.conn();
        conn.busy_timeout(std::time::Duration::from_millis(10))?;

        let query = format!(
            "SELECT {}, received_at FROM {}
                WHERE system_id=:system_id AND component_id=:component_id
                    AND received_at > :since
                ORDER BY received_at ASC
                LIMIT :limit",
            M::default().rows().join(","),
            M::default().table(),
        );

        let since = since.map_or(i64::MIN, |t| t.timestamp_micros());
        // SQLite reads a negative LIMIT as no upper bound.
        let limit = limit.map_or(-1, |n| i64::try_from(n).unwrap_or(i64::MAX));
        let mut stmt = conn.prepare_cached(&query)?;
        let table = M::default().table().to_owned();
        let rows = stmt
            .query_map(
                rusqlite::named_params! {
                    ":system_id": system_id,
                    ":component_id": component_id,
                    ":since": since,
                    ":limit": limit,
                },
                |row| {
                    let m = M::from_row(row)?;
                    let col = M::default().rows().len();
                    let t = datetime_from_micros(col, row.get(col)?)?;
                    Ok((t, m))
                },
            )?
            // A row may have been written by a dialect whose enum extensions (e.g. MAV_CMD)
            // the requested type M doesn't know about; skip just that row rather than
            // failing the whole query.
            .filter_map(|r| match r {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::warn!("skipping unparseable {table} row: {e}");
                    None
                }
            })
            .collect::<Vec<_>>();

        Ok(rows)
    }

    /// Number of stored messages of this type, taken from the in-memory write stats instead of a
    /// `COUNT(*)` scan whose cost grows with session length. Exact, since every write updates the
    /// stats.
    pub fn count_message_cached<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
    ) -> usize {
        let msg_id = M::default().id();
        let stats = self.stats.lock().unwrap();
        stats
            .iter()
            .filter(|((sys, comp, id, _instance), _)| {
                *sys == system_id && *comp == component_id && *id == msg_id
            })
            .map(|(_, rs)| usize::try_from(rs.count).unwrap_or(usize::MAX))
            .sum()
    }

    pub fn count_message<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
    ) -> Result<usize, DbError> {
        let conn = self.conn();
        conn.busy_timeout(std::time::Duration::from_millis(10))?;

        let mut stmt = conn.prepare_cached(&format!(
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
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT COUNT(*) FROM {table_name}
                WHERE system_id=:system_id AND component_id=:component_id",
        ))?;

        let count = stmt.query_one(
            &[(":system_id", &system_id), (":component_id", &component_id)],
            |row| row.get(0),
        )?;

        Ok(count)
    }

    /// One row per `(message_type, instance_value)` pair received for the
    /// given system/component, sorted by message ID and instance value.
    #[allow(clippy::unwrap_in_result)]
    pub fn message_summary(
        &self,
        system_id: u8,
        component_id: u8,
    ) -> Result<Vec<MessageSummary>, DbError> {
        puffin::profile_function!();

        let cutoff = Utc::now() - chrono::TimeDelta::seconds(FREQ_WINDOW_SECS);
        let protocol = mavspec::definitions::protocol();
        let rapid = rapid_dialect::definitions::protocol();

        let mut result = Vec::new();
        let mut stats = self.stats.lock().unwrap();
        for ((sys, comp, msg_id, instance_value), rs) in stats.iter_mut() {
            if *sys != system_id || *comp != component_id {
                continue;
            }
            // Pop stale window entries so freq_hz decays correctly when
            // the stream goes silent between writes.
            while rs.recent.front().is_some_and(|t| *t < cutoff) {
                rs.recent.pop_front();
            }
            let Some(last) = rs.last else { continue };

            let msg_name = lookup_msg_name(*msg_id, protocol, rapid);
            let instance = match (instance_value, self.instance_fields.get(msg_id)) {
                (Some(value), Some(field)) => Some(MessageInstance {
                    field: field.clone(),
                    value: *value,
                }),
                _ => None,
            };
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                reason = "count and recent.len() are bounded by realistic message rates; precision loss is irrelevant for display"
            )]
            result.push(MessageSummary {
                msg_id: *msg_id,
                name: msg_name,
                instance,
                count: rs.count as usize,
                freq_hz: rs.recent.len() as f32 / FREQ_WINDOW_SECS as f32,
                last,
            });
        }

        drop(stats);

        result.sort_by(|a, b| {
            a.msg_id.cmp(&b.msg_id).then_with(|| {
                a.instance
                    .as_ref()
                    .map(|i| i.value)
                    .cmp(&b.instance.as_ref().map(|i| i.value))
            })
        });

        Ok(result)
    }

    /// Fetch the last message of a given type, decoded into its concrete Rust
    /// struct and pretty-printed via `Debug`.
    pub fn last_message_debug_by_name(
        &self,
        msg_name: &str,
        system_id: u8,
        component_id: u8,
        instance: Option<(&str, i64)>,
    ) -> Result<Option<String>, DbError> {
        if let Some(s) =
            last_message_debug_common(self, msg_name, system_id, component_id, instance)?
        {
            return Ok(Some(s));
        }

        if let Some(s) =
            last_message_debug_ardupilotmega(self, msg_name, system_id, component_id, instance)?
        {
            return Ok(Some(s));
        }

        last_message_debug_rapid(self, msg_name, system_id, component_id, instance)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn timeseries_by_name(
        &self,
        msg_name: &str,
        field_name: &str,
        system_id: u8,
        component_id: u8,
        since: Option<chrono::DateTime<chrono::Utc>>,
        _until: Option<chrono::DateTime<chrono::Utc>>,
        instance: Option<(&str, i64)>,
    ) -> Result<Vec<(chrono::DateTime<chrono::Utc>, f64)>, DbError> {
        puffin::profile_function!(msg_name.to_owned() + "." + field_name);

        let conn = self.conn();
        let lower_case = msg_name.to_lowercase();

        let since = since.unwrap_or_default().timestamp_micros();

        let protocol = mavspec::definitions::protocol();
        let Some(field) = ["common", "ardupilotmega"]
            .iter()
            .find_map(|dn| {
                let dialect = protocol.get_dialect_by_name(dn).unwrap();
                dialect
                    .get_message_by_name(msg_name)
                    .and_then(|msg| msg.get_field_by_name(field_name))
            })
            .or_else(|| {
                rapid_dialect::definitions::protocol()
                    .get_dialect_by_canonical_name("rapid")
                    .unwrap()
                    .get_message_by_name(msg_name)
                    .and_then(|msg| msg.get_field_by_name(field_name))
            })
        else {
            unreachable!();
        };

        let extra_where = match instance {
            Some((col, _)) => format!(" AND {col} = ?4"),
            None => String::new(),
        };

        // Look Ma, SQL injection
        let query = format!(
            "SELECT received_at, {field_name} FROM messages_{lower_case}
                    WHERE system_id=?1 AND component_id=?2 AND received_at >= ?3{extra_where}
                    ORDER BY received_at ASC"
        );

        let mut stmt = conn.prepare_cached(&query)?;
        let row_mapper = |row: &rusqlite::Row<'_>| {
            let timestamp = datetime_from_micros(0, row.get(0)?)?;
            let int: i64 = row.get(1)?;
            let value = match field.r#type() {
                MavType::Float => f64::from(f32::from_bits(int as u32)),
                MavType::Double => f64::from_bits(int as u64),
                _ => int as f64,
            };
            Ok((timestamp, value))
        };
        let timeseries = match instance {
            Some((_, value)) => stmt
                .query_map(
                    rusqlite::params![&system_id, &component_id, &since, &value],
                    row_mapper,
                )?
                .collect::<Result<Vec<_>, _>>()?,
            None => stmt
                .query_map(
                    rusqlite::params![&system_id, &component_id, &since],
                    row_mapper,
                )?
                .collect::<Result<Vec<_>, _>>()?,
        };

        Ok(timeseries)
    }
}

fn collect_instance_fields(dialect: &mavinspect::protocol::Dialect) -> HashMap<u32, String> {
    dialect
        .messages()
        .into_iter()
        .filter_map(|message: &mavinspect::protocol::Message| {
            let field = message.fields().iter().find(|f| f.instance())?;
            let colname = match field.name() {
                "index" => "index_",
                "type" => "type_",
                n => n,
            };
            Some((message.id(), colname.to_owned()))
        })
        .collect()
}

pub fn format_message_label(name: &str, instance: Option<&MessageInstance>) -> String {
    match instance {
        Some(i) => format!("{name}[{}={}]", i.field, i.value),
        None => name.to_owned(),
    }
}

fn lookup_msg_name(
    id: u32,
    protocol: &mavinspect::protocol::Protocol,
    rapid: &mavinspect::protocol::Protocol,
) -> String {
    ["common", "ardupilotmega"]
        .iter()
        .find_map(|dn| {
            protocol
                .get_dialect_by_name(dn)
                .unwrap()
                .get_message_by_id(id)
                .map(|m| m.name().to_owned())
        })
        .or_else(|| {
            rapid
                .get_dialect_by_canonical_name("rapid")
                .unwrap()
                .get_message_by_id(id)
                .map(|m| m.name().to_owned())
        })
        .unwrap_or_else(|| format!("UNKNOWN_{id}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valve_command_does_not_poison_command_long_reads() {
        let db = Db::init();

        let normal = mavspec::rust::dialects::common::messages::CommandLong {
            target_system: 1,
            target_component: 1,
            command: mavspec::rust::dialects::common::enums::MavCmd::ComponentArmDisarm,
            ..Default::default()
        };
        db.write_message(1, 1, &normal).unwrap();

        let valve = rapid_dialect::rapid::messages::CommandLong {
            target_system: 1,
            target_component: 1,
            command: rapid_dialect::rapid::enums::MavCmd::CommandValve,
            ..Default::default()
        };
        db.write_message(1, 1, &valve).unwrap();

        let rows = db
            .all_messages::<mavspec::rust::dialects::common::messages::CommandLong>(1, 1)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].1.command,
            mavspec::rust::dialects::common::enums::MavCmd::ComponentArmDisarm
        );
    }

    #[test]
    fn rapid_command_long_reads_decode_both_common_and_rapid_commands() {
        let db = Db::init();

        let normal = mavspec::rust::dialects::common::messages::CommandLong {
            target_system: 1,
            target_component: 1,
            command: mavspec::rust::dialects::common::enums::MavCmd::ComponentArmDisarm,
            ..Default::default()
        };
        db.write_message(1, 1, &normal).unwrap();

        let valve = rapid_dialect::rapid::messages::CommandLong {
            target_system: 1,
            target_component: 1,
            command: rapid_dialect::rapid::enums::MavCmd::CommandValve,
            ..Default::default()
        };
        db.write_message(1, 1, &valve).unwrap();

        let mut rows = db
            .all_messages::<rapid_dialect::rapid::messages::CommandLong>(1, 1)
            .unwrap();
        rows.sort_by_key(|(t, _)| *t);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].1.command,
            rapid_dialect::rapid::enums::MavCmd::ComponentArmDisarm
        );
        assert_eq!(
            rows[1].1.command,
            rapid_dialect::rapid::enums::MavCmd::CommandValve
        );
    }
}
