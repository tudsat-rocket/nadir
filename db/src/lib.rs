use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use mavspec::rust::spec::MessageSpec;

/// Window the rates in [`MessageSummary`] are averaged over.
const FREQ_WINDOW_SECS: i64 = 5;

/// (`system_id`, `component_id`, `message_id`, stored message type, `instance_value`)
///
/// Keyed by Rust type as well as by message ID, because a dialect extending `common` generates its
/// own type for every message it inherits: `common::CommandLong` and `rapid::CommandLong` are both
/// message 76, and a series holds one or the other. Queries that name a message match on the ID,
/// and so see every dialect's version of it at once.
type SeriesKey = (u8, u8, u32, TypeId, Option<i64>);

/// Every value is a `Vec<(DateTime<Utc>, M)>` for the single concrete `M` its key names.
type SeriesMap = HashMap<SeriesKey, Box<dyn Series>>;

#[derive(Clone)]
pub struct Db {
    series: Arc<Mutex<SeriesMap>>,
    instance_fields: Arc<HashMap<u32, String>>,
    msg_names: Arc<HashMap<u32, String>>,
    /// Message name to its ID and field names, unioned over the dialects defining it. Taken from
    /// the definitions rather than from the series, so a query naming a message that has not
    /// arrived yet still tells a bad field name from a good one.
    msg_defs: Arc<HashMap<String, (u32, Vec<String>)>>,
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("No {0} stored for this system")]
    NotFound(&'static str),
    #[error("Unknown message or field: {0}")]
    UnknownField(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageInstance {
    pub field: String,
    pub value: i64,
}

/// One [`MessageSummary`] while it is still being accumulated over a message's dialect types.
#[derive(Default)]
struct SummaryRow {
    count: usize,
    recent: usize,
    last: Option<DateTime<Utc>>,
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

/// The supertraits are what let a message be stored behind [`Any`] and handed back out by value.
pub trait MessageExt: MessageSpec + Clone + Debug + Send + Sync + 'static {
    /// Field names, in wire order. Indices into this are what [`Self::field_f64`] takes.
    fn rows() -> &'static [&'static str];

    /// Numeric value of the message's `instance="true"` field, if any.
    fn instance_value(&self) -> Option<i64> {
        None
    }

    /// Name of the message's `instance="true"` field, if any.
    fn instance_field() -> Option<&'static str> {
        None
    }

    /// Numeric value of the field at `index`, or `None` for an array field, which has no single
    /// number to plot.
    fn field_f64(&self, index: usize) -> Option<f64>;

    /// Appends a clone of self to its series. The dialect enums dispatch to the inner variant, so
    /// what lands in the store is always a concrete message type.
    fn store(&self, db: &Db, system_id: u8, component_id: u8, received_at: DateTime<Utc>);
}

/// The type-erased face of a stored series: everything a query addressing a message by name needs.
///
/// Windowing and field lookup read through it, so they are compiled once rather than once per
/// message type. The impl below is all that is still monomorphised over the several hundred of
/// them, and it dominates the crate's build, so keep its methods short.
trait Series: Any + Send + Sync {
    fn samples(&self) -> usize;

    /// Samples received after `cutoff`, for the message rates in [`MessageSummary`].
    fn count_since(&self, cutoff: DateTime<Utc>) -> usize;

    fn field_index(&self, field_name: &str) -> Option<usize>;

    /// One field within `(since, until]`, as plot points.
    fn points(
        &self,
        index: usize,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Vec<(DateTime<Utc>, f64)>;

    fn last_time(&self) -> Option<DateTime<Utc>>;

    fn last_debug(&self) -> Option<String>;
}

impl<M: MessageExt> Series for Vec<(DateTime<Utc>, M)> {
    fn samples(&self) -> usize {
        self.len()
    }

    fn count_since(&self, cutoff: DateTime<Utc>) -> usize {
        Db::window(self, Some(cutoff), None).len()
    }

    fn field_index(&self, field_name: &str) -> Option<usize> {
        M::rows().iter().position(|row| *row == field_name)
    }

    fn points(
        &self,
        index: usize,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Vec<(DateTime<Utc>, f64)> {
        Db::window(self, since, until)
            .iter()
            .filter_map(|(t, msg)| Some((*t, msg.field_f64(index)?)))
            .collect()
    }

    fn last_time(&self) -> Option<DateTime<Utc>> {
        Some(self.last()?.0)
    }

    fn last_debug(&self) -> Option<String> {
        Some(format!("{:#?}", self.last()?.1))
    }
}

impl Db {
    pub fn init() -> Self {
        let protocol = mavspec::definitions::protocol();
        let dialects = [
            protocol.get_dialect_by_name("common").unwrap(),
            protocol.get_dialect_by_name("ardupilotmega").unwrap(),
            rapid_dialect::definitions::protocol()
                .get_dialect_by_canonical_name("rapid")
                .unwrap(),
        ];

        Self {
            series: Arc::new(Mutex::new(HashMap::new())),
            instance_fields: Arc::new(
                dialects
                    .into_iter()
                    .flat_map(Self::collect_instance_fields)
                    .collect(),
            ),
            msg_names: Arc::new(
                dialects
                    .into_iter()
                    .flat_map(|dialect| {
                        dialect
                            .messages()
                            .into_iter()
                            .map(|message| (message.id(), message.name().to_owned()))
                    })
                    .collect(),
            ),
            msg_defs: Arc::new(dialects.into_iter().fold(
                HashMap::new(),
                |mut defs: HashMap<String, (u32, Vec<String>)>, dialect| {
                    for message in dialect.messages() {
                        let (_, fields) = defs
                            .entry(message.name().to_owned())
                            .or_insert_with(|| (message.id(), Vec::new()));

                        for field in message.fields() {
                            if !fields.iter().any(|known| known == field.name()) {
                                fields.push(field.name().to_owned());
                            }
                        }
                    }

                    defs
                },
            )),
        }
    }

    fn collect_instance_fields(dialect: &mavinspect::protocol::Dialect) -> HashMap<u32, String> {
        dialect
            .messages()
            .into_iter()
            .filter_map(|message: &mavinspect::protocol::Message| {
                let field = message.fields().iter().find(|f| f.instance())?;
                Some((message.id(), field.name().to_owned()))
            })
            .collect()
    }

    /// Appends one message to its series. Called by the generated [`MessageExt::store`] impls,
    /// which are what resolve `M` to a concrete type.
    pub fn push<M: MessageExt>(
        &self,
        system_id: u8,
        component_id: u8,
        received_at: DateTime<Utc>,
        msg: M,
    ) {
        let key = (
            system_id,
            component_id,
            msg.id(),
            TypeId::of::<M>(),
            msg.instance_value(),
        );

        let mut series = self.series.lock().unwrap();
        let stored: &mut dyn Any = &mut **series
            .entry(key)
            .or_insert_with(|| Box::new(Vec::<(DateTime<Utc>, M)>::new()));

        stored
            .downcast_mut::<Vec<(DateTime<Utc>, M)>>()
            .expect("a series key names the type that series holds")
            .push((received_at, msg));
    }

    /// Every series holding an `M` for this system, one per instance value.
    fn slices<M: MessageExt>(
        series: &SeriesMap,
        system_id: u8,
        component_id: u8,
    ) -> impl Iterator<Item = (&SeriesKey, &[(DateTime<Utc>, M)])> {
        let type_id = TypeId::of::<M>();

        series
            .iter()
            .filter(move |((sys, comp, _, ty, _), _)| {
                *sys == system_id && *comp == component_id && *ty == type_id
            })
            .filter_map(|(key, stored)| {
                let stored: &dyn Any = &**stored;
                let rows = stored.downcast_ref::<Vec<(DateTime<Utc>, M)>>()?;
                Some((key, rows.as_slice()))
            })
    }

    /// The part of a series within `(since, until]`.
    ///
    /// Binary search, which holds because a series is appended in receive order: live ingest is
    /// sequential, and a recording is read in file order.
    fn window<M>(
        rows: &[(DateTime<Utc>, M)],
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> &[(DateTime<Utc>, M)] {
        let start = since.map_or(0, |since| rows.partition_point(|(t, _)| *t <= since));
        let end = until.map_or(rows.len(), |until| {
            rows.partition_point(|(t, _)| *t <= until)
        });

        rows.get(start..end.max(start)).unwrap_or_default()
    }

    pub fn write_message<M: MessageExt>(&self, system_id: u8, component_id: u8, msg: &M) {
        self.write_message_at(system_id, component_id, msg, Utc::now());
    }

    pub fn write_message_at<M: MessageExt>(
        &self,
        system_id: u8,
        component_id: u8,
        msg: &M,
        received_at: DateTime<Utc>,
    ) {
        msg.store(self, system_id, component_id, received_at);
    }

    #[allow(
        clippy::unwrap_in_result,
        reason = "a poisoned store is not recoverable"
    )]
    pub fn last_message<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
    ) -> Result<M, DbError> {
        let series = self.series.lock().unwrap();

        Self::slices::<M>(&series, system_id, component_id)
            .filter_map(|(_, rows)| rows.last())
            .max_by_key(|(t, _)| *t)
            .map(|(_, msg)| msg.clone())
            .ok_or_else(|| DbError::NotFound(std::any::type_name::<M>()))
    }

    #[allow(
        clippy::unwrap_in_result,
        reason = "a poisoned store is not recoverable"
    )]
    pub fn last_message_filtered<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
        instance: Option<(&str, i64)>,
    ) -> Result<M, DbError> {
        let Some((_, value)) = instance else {
            return self.last_message(system_id, component_id);
        };

        let series = self.series.lock().unwrap();

        Self::slices::<M>(&series, system_id, component_id)
            .find(|((.., instance), _)| *instance == Some(value))
            .and_then(|(_, rows)| rows.last())
            .map(|(_, msg)| msg.clone())
            .ok_or_else(|| DbError::NotFound(std::any::type_name::<M>()))
    }

    pub fn all_messages<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
    ) -> Vec<(DateTime<Utc>, M)> {
        self.messages_since(system_id, component_id, None, None)
    }

    /// The oldest `limit` messages newer than `since`, or all of them if no limit is given.
    pub fn messages_since<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
        since: Option<DateTime<Utc>>,
        limit: Option<usize>,
    ) -> Vec<(DateTime<Utc>, M)> {
        let series = self.series.lock().unwrap();
        let limit = limit.unwrap_or(usize::MAX);

        let mut windows = Self::slices::<M>(&series, system_id, component_id)
            .map(|(_, rows)| Self::window(rows, since, None))
            .filter(|rows| !rows.is_empty());

        let Some(first) = windows.next() else {
            return Vec::new();
        };

        // The common case is a message with no instance field, and so a single series, already in
        // receive order and needing no merge.
        let Some(second) = windows.next() else {
            return first.iter().take(limit).cloned().collect();
        };

        let mut merged: Vec<(DateTime<Utc>, M)> = first
            .iter()
            .chain(second)
            .chain(windows.flatten())
            .cloned()
            .collect();
        merged.sort_by_key(|(t, _)| *t);
        merged.truncate(limit);

        merged
    }

    /// Number of stored messages of this type, over every instance of it.
    pub fn message_count<M: MessageExt + Default>(&self, system_id: u8, component_id: u8) -> usize {
        let series = self.series.lock().unwrap();
        Self::slices::<M>(&series, system_id, component_id)
            .map(|(_, rows)| rows.len())
            .sum()
    }

    /// One row per `(message_id, instance_value)` pair stored for the given system/component,
    /// sorted by message ID and instance value.
    pub fn message_summary(&self, system_id: u8, component_id: u8) -> Vec<MessageSummary> {
        let cutoff = Utc::now() - chrono::TimeDelta::seconds(FREQ_WINDOW_SECS);
        let series = self.series.lock().unwrap();

        // A message two dialects define is stored once per dialect type, and reported as one row.
        let mut rows: HashMap<(u32, Option<i64>), SummaryRow> = HashMap::new();
        for ((sys, comp, msg_id, _, instance_value), stored) in series.iter() {
            if *sys != system_id || *comp != component_id {
                continue;
            }
            let Some(last) = stored.last_time() else {
                continue;
            };

            let row = rows.entry((*msg_id, *instance_value)).or_default();
            row.count += stored.samples();
            row.recent += stored.count_since(cutoff);
            row.last = row.last.max(Some(last));
        }

        drop(series);

        #[allow(
            clippy::cast_precision_loss,
            reason = "a sample count is bounded by realistic message rates; precision loss is irrelevant for display"
        )]
        let mut result: Vec<MessageSummary> = rows
            .into_iter()
            .filter_map(|((msg_id, instance_value), row)| {
                Some(MessageSummary {
                    msg_id,
                    name: self
                        .msg_names
                        .get(&msg_id)
                        .cloned()
                        .unwrap_or_else(|| format!("UNKNOWN_{msg_id}")),
                    instance: instance_value.zip(self.instance_fields.get(&msg_id)).map(
                        |(value, field)| MessageInstance {
                            field: field.clone(),
                            value,
                        },
                    ),
                    count: row.count,
                    freq_hz: row.recent as f32 / FREQ_WINDOW_SECS as f32,
                    last: row.last?,
                })
            })
            .collect();

        result.sort_by_key(|row| (row.msg_id, row.instance.as_ref().map(|i| i.value)));

        result
    }

    /// Fetch the last message of a given name, pretty-printed via `Debug`. `None` when no dialect
    /// defines it.
    #[allow(
        clippy::unwrap_in_result,
        reason = "a poisoned store is not recoverable"
    )]
    pub fn last_message_debug_by_name(
        &self,
        msg_name: &str,
        system_id: u8,
        component_id: u8,
        instance: Option<(&str, i64)>,
    ) -> Result<Option<String>, DbError> {
        let Some((msg_id, _)) = self.msg_defs.get(msg_name) else {
            return Ok(None);
        };

        let series = self.series.lock().unwrap();

        Self::matching(&series, *msg_id, system_id, component_id, instance)
            .filter_map(|(_, stored)| Some((stored.last_time()?, stored)))
            .max_by_key(|(last, _)| *last)
            .and_then(|(_, stored)| stored.last_debug())
            .map(Some)
            .ok_or(DbError::NotFound("message"))
    }

    /// One field of one message type over time, for plotting.
    ///
    /// A message several dialects define is stored under whichever of their types decoded it, so
    /// one line's points can be spread across them, as they are across the instances of a
    /// multi-instance message. A plot is an aggregation rather than a typed read, so unlike
    /// `all_messages` those series simply merge.
    #[allow(
        clippy::too_many_arguments,
        clippy::unwrap_in_result,
        reason = "a poisoned store is not recoverable"
    )]
    pub fn timeseries_by_name(
        &self,
        msg_name: &str,
        field_name: &str,
        system_id: u8,
        component_id: u8,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
        instance: Option<(&str, i64)>,
    ) -> Result<Vec<(DateTime<Utc>, f64)>, DbError> {
        let (msg_id, fields) = self
            .msg_defs
            .get(msg_name)
            .ok_or_else(|| DbError::UnknownField(msg_name.to_owned()))?;

        if !fields.iter().any(|field| field == field_name) {
            return Err(DbError::UnknownField(field_name.to_owned()));
        }

        let series = self.series.lock().unwrap();
        let mut points = Vec::new();
        let mut contributions = 0;

        for (_, stored) in Self::matching(&series, *msg_id, system_id, component_id, instance) {
            // A shared message can have a different field set in each dialect; the ones without
            // this field contribute nothing rather than failing the whole line.
            let Some(index) = stored.field_index(field_name) else {
                continue;
            };

            let found = stored.points(index, since, until);
            if !found.is_empty() {
                contributions += 1;
                points.extend(found);
            }
        }

        if contributions > 1 {
            points.sort_by_key(|(t, _)| *t);
        }

        Ok(points)
    }

    /// Every series of one message for this system, one per dialect type and instance value.
    fn matching<'a>(
        series: &'a SeriesMap,
        msg_id: u32,
        system_id: u8,
        component_id: u8,
        instance: Option<(&str, i64)>,
    ) -> impl Iterator<Item = (&'a SeriesKey, &'a dyn Series)> {
        series
            .iter()
            .filter(move |((sys, comp, id, _, value), _)| {
                *sys == system_id
                    && *comp == component_id
                    && *id == msg_id
                    && instance.is_none_or(|(_, wanted)| *value == Some(wanted))
            })
            .map(|(key, stored)| (key, &**stored))
    }
}

pub fn format_message_label(name: &str, instance: Option<&MessageInstance>) -> String {
    match instance {
        Some(i) => format!("{name}[{}={}]", i.field, i.value),
        None => name.to_owned(),
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

#[cfg(test)]
mod tests {
    use super::*;

    use mavspec::rust::dialects::common::messages as common;
    use rapid_dialect::rapid::messages as rapid;

    #[test]
    fn the_summary_counts_every_instance_and_dialect_of_a_message() {
        let db = Db::init();

        for (id, pressure1) in [(0_u8, 100_u16), (1, 200), (0, 150)] {
            db.write_message(
                1,
                1,
                &rapid::PressureVessel {
                    id,
                    pressure1,
                    ..Default::default()
                },
            );
        }
        // Both are message 76, stored under two types and reported as one row.
        db.write_message(1, 1, &common::CommandLong::default());
        db.write_message(1, 1, &rapid::CommandLong::default());
        db.write_message(2, 1, &common::Attitude::default());

        let summary = db.message_summary(1, 1);
        let row = |name: &str, instance| {
            summary
                .iter()
                .find(|row| row.name == name && row.instance.as_ref().map(|i| i.value) == instance)
                .map(|row| (row.count, row.freq_hz))
        };

        assert_eq!(row("PRESSURE_VESSEL", Some(0)), Some((2, 0.4)));
        assert_eq!(row("PRESSURE_VESSEL", Some(1)), Some((1, 0.2)));
        assert_eq!(row("COMMAND_LONG", None), Some((2, 0.4)));

        // Another system's messages stay out of it, and the rows are sorted by id then instance.
        assert!(row("ATTITUDE", None).is_none());
        assert!(
            summary
                .windows(2)
                .all(|w| (w[0].msg_id, w[0].instance.as_ref().map(|i| i.value))
                    < (w[1].msg_id, w[1].instance.as_ref().map(|i| i.value)))
        );
    }

    #[test]
    fn a_dialects_own_type_reads_back_only_its_own_messages() {
        let db = Db::init();

        db.write_message(
            1,
            1,
            &common::CommandLong {
                command: mavspec::rust::dialects::common::enums::MavCmd::ComponentArmDisarm,
                ..Default::default()
            },
        );

        db.write_message(
            1,
            1,
            &rapid::CommandLong {
                command: rapid_dialect::rapid::enums::MavCmd::CommandValve,
                ..Default::default()
            },
        );

        // Both are message 76, but they are separate Rust types and so separate series. Callers
        // that want every command read both, as the commands pane does.
        let common_rows = db.all_messages::<common::CommandLong>(1, 1);
        assert_eq!(common_rows.len(), 1);
        assert_eq!(
            common_rows[0].1.command,
            mavspec::rust::dialects::common::enums::MavCmd::ComponentArmDisarm
        );

        let rapid_rows = db.all_messages::<rapid::CommandLong>(1, 1);
        assert_eq!(rapid_rows.len(), 1);
        assert_eq!(
            rapid_rows[0].1.command,
            rapid_dialect::rapid::enums::MavCmd::CommandValve
        );
    }

    #[test]
    fn floats_round_trip_through_the_timeseries_path() {
        let db = Db::init();

        for roll in [0.0_f32, -1.5, f32::MAX, f32::NAN] {
            db.write_message(
                1,
                1,
                &common::Attitude {
                    roll,
                    ..Default::default()
                },
            );
        }

        let series = db
            .timeseries_by_name("ATTITUDE", "roll", 1, 1, None, None, None)
            .unwrap();

        let values: Vec<f32> = series.iter().map(|(_, v)| *v as f32).collect();
        assert_eq!(values.len(), 4);
        assert_eq!(values[0], 0.0);
        assert_eq!(values[1], -1.5);
        assert_eq!(values[2], f32::MAX);
        // A NaN reading survives rather than becoming a hole in the series.
        assert!(values[3].is_nan());
        assert!(
            db.last_message::<common::Attitude>(1, 1)
                .unwrap()
                .roll
                .is_nan()
        );
    }

    #[test]
    fn array_fields_survive_a_round_trip() {
        let db = Db::init();

        db.write_message(
            1,
            1,
            &common::CanFrame {
                id: 0x42,
                data: [1, 2, 3, 4, 5, 6, 7, 8],
                ..Default::default()
            },
        );

        let rows = db.all_messages::<common::CanFrame>(1, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.id, 0x42);
        assert_eq!(rows[0].1.data, [1, 2, 3, 4, 5, 6, 7, 8]);

        // An array has no single number to plot, so it yields no points rather than an error.
        let series = db
            .timeseries_by_name("CAN_FRAME", "data", 1, 1, None, None, None)
            .unwrap();
        assert!(series.is_empty());
    }

    #[test]
    fn instances_of_one_message_are_stored_apart() {
        let db = Db::init();

        let base = DateTime::from_timestamp_micros(1_000_000).unwrap();
        for (i, (id, pressure1)) in [(0_u8, 100_u16), (1, 200), (0, 150)]
            .into_iter()
            .enumerate()
        {
            db.write_message_at(
                1,
                1,
                &rapid::PressureVessel {
                    id,
                    pressure1,
                    ..Default::default()
                },
                base + chrono::TimeDelta::milliseconds(i as i64),
            );
        }

        let field = rapid::PressureVessel::instance_field().unwrap();
        let vessel = |id| {
            db.last_message_filtered::<rapid::PressureVessel>(1, 1, Some((field, id)))
                .unwrap()
                .pressure1
        };

        assert_eq!(vessel(0), 150);
        assert_eq!(vessel(1), 200);

        // Unfiltered, the newest across every instance.
        assert_eq!(
            db.last_message::<rapid::PressureVessel>(1, 1)
                .unwrap()
                .pressure1,
            150
        );

        let pressures = |instance| {
            db.timeseries_by_name("PRESSURE_VESSEL", "pressure1", 1, 1, None, None, instance)
                .unwrap()
                .into_iter()
                .map(|(_, v)| v)
                .collect::<Vec<_>>()
        };

        // A plot line for one instance sees only that instance; without one, every instance
        // merges in receive order.
        assert_eq!(pressures(Some((field, 0))), [100.0, 150.0]);
        assert_eq!(pressures(None), [100.0, 200.0, 150.0]);
    }

    #[test]
    fn messages_since_excludes_the_cursor_and_honours_the_limit() {
        let db = Db::init();

        let base = DateTime::from_timestamp_micros(1_000_000).unwrap();
        for i in 0..5 {
            db.write_message_at(
                1,
                1,
                &common::Attitude {
                    time_boot_ms: i,
                    ..Default::default()
                },
                base + chrono::TimeDelta::milliseconds(i64::from(i)),
            );
        }

        let boot_ms = |rows: Vec<(DateTime<Utc>, common::Attitude)>| {
            rows.into_iter()
                .map(|(_, m)| m.time_boot_ms)
                .collect::<Vec<_>>()
        };

        assert_eq!(boot_ms(db.all_messages(1, 1)), [0, 1, 2, 3, 4]);

        // Strictly newer than the cursor, so re-polling with the last timestamp yields no repeats.
        let cursor = base + chrono::TimeDelta::milliseconds(2);
        assert_eq!(boot_ms(db.messages_since(1, 1, Some(cursor), None)), [3, 4]);

        assert_eq!(boot_ms(db.messages_since(1, 1, None, Some(2))), [0, 1]);
    }

    #[test]
    fn the_debug_view_follows_the_instance_it_was_asked_for() {
        let db = Db::init();

        for (id, pressure1) in [(0_u8, 100_u16), (1, 200)] {
            db.write_message(
                1,
                1,
                &rapid::PressureVessel {
                    id,
                    pressure1,
                    ..Default::default()
                },
            );
        }

        let debug = |instance| {
            db.last_message_debug_by_name("PRESSURE_VESSEL", 1, 1, instance)
                .map(|found| found.is_some_and(|text| text.contains("pressure1: 200")))
        };

        let field = rapid::PressureVessel::instance_field().unwrap();
        assert!(debug(Some((field, 1))).unwrap());
        assert!(!debug(Some((field, 0))).unwrap());
        // Unfiltered, the newest across every instance.
        assert!(debug(None).unwrap());

        assert!(matches!(
            db.last_message_debug_by_name("ATTITUDE", 1, 1, None),
            Err(DbError::NotFound(_))
        ));
        assert!(matches!(
            db.last_message_debug_by_name("NOT_A_MESSAGE", 1, 1, None),
            Ok(None)
        ));
    }

    #[test]
    fn an_unknown_message_or_field_is_an_error_rather_than_a_panic() {
        let db = Db::init();

        assert!(matches!(
            db.timeseries_by_name("NOT_A_MESSAGE", "roll", 1, 1, None, None, None),
            Err(DbError::UnknownField(_))
        ));

        assert!(matches!(
            db.timeseries_by_name("ATTITUDE", "not_a_field", 1, 1, None, None, None),
            Err(DbError::UnknownField(_))
        ));
    }
}
